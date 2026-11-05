use super::*;
use igpu_core::{
    applications::{Candidate, LaunchKind, Management, Placement, Verification},
    launchers::{self, InstalledLauncher},
};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

#[derive(Clone)]
pub(super) struct Trial {
    pub profile: Profile,
    pub keep_name: String,
    pub process: ProcessId,
    pub verification: Option<Verification>,
    pub launcher_exit: Option<String>,
}

pub(super) struct TrialSetup {
    profile: Profile,
    previous: Vec<ProcessId>,
    matches: Vec<InstalledLauncher>,
    selected_launcher: Option<String>,
    create_launcher: bool,
}

pub(super) fn start_trial(
    mut profile: Profile,
    previous: &[ProcessId],
) -> anyhow::Result<(Trial, Child)> {
    use anyhow::{bail, Context};
    igpu_core::config::validate_profile(&profile)?;
    for id in previous {
        if igpu_core::collector::read_process_stat(
            &Path::new("/proc").join(id.pid.to_string()).join("stat"),
            id.pid,
        )
        .is_some_and(|p| p.0 == *id)
        {
            bail!("The previous application is still running. Close it normally, including any background instance, then choose Launch test again.");
        }
    }
    if launch::application_running(Path::new("/proc"), &profile.executable, std::process::id())
        || ((profile.firefox || launch::is_firefox(&profile.executable))
            && launch::firefox_running(Path::new("/proc"), std::process::id()))
    {
        bail!("This application is already running. Close it normally before testing a fresh GPU launch.");
    }
    launchers::executable_path(&profile.executable)
        .context("The selected executable is not installed or executable")?;
    profile.record = false;
    profile.mangohud = false;
    profile.verification = None;
    let keep_name = profile.name.clone();
    profile.name = format!("trial-{}", monotonic_ns());
    let payload = serde_json::to_vec(&profile)?;
    anyhow::ensure!(payload.len() <= 1024 * 1024, "Trial profile exceeds 1 MiB");
    let mut child = Command::new(cli_path())
        .arg("try-profile")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not start the trial wrapper")?;
    // Close the pipe before inspecting identity: the wrapper waits for EOF and
    // cannot launch from a partial profile. No profile data appears in argv.
    use std::io::Write;
    let sent = child
        .stdin
        .take()
        .context("Trial input pipe missing")
        .and_then(|mut pipe| {
            pipe.write_all(&payload)
                .context("Could not send trial settings")
        });
    if let Err(error) = sent {
        // Only this owned, not-yet-launched wrapper can be terminated here.
        if let Err(cleanup) = child.kill() {
            igpu_core::logging::event(
                "warn",
                "trial.wrapper_kill_failed",
                igpu_core::logging::error_fields(&cleanup.into()),
            );
        }
        if let Err(cleanup) = child.wait() {
            igpu_core::logging::event(
                "warn",
                "trial.wrapper_reap_failed",
                igpu_core::logging::error_fields(&cleanup.into()),
            );
        }
        return Err(error);
    }
    let process = match igpu_core::collector::read_process_stat(
        &Path::new("/proc").join(child.id().to_string()).join("stat"),
        child.id(),
    ) {
        Some(p) => p.0,
        None => {
            std::thread::spawn(move || {
                if let Err(error) = child.wait() {
                    igpu_core::logging::event(
                        "warn",
                        "trial.reap_failed",
                        igpu_core::logging::error_fields(&error.into()),
                    );
                }
            });
            bail!("Could not read the trial launch identity. The application may have exited or remain running; close it normally before retrying.");
        }
    };
    Ok((
        Trial {
            profile,
            keep_name,
            process,
            verification: None,
            launcher_exit: None,
        },
        child,
    ))
}

fn unique_name(executable: &str, profiles: &[Profile]) -> String {
    let stem = Path::new(executable)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(50)
        .collect();
    let base = format!(
        "{}-igpu",
        if stem.is_empty() {
            "application"
        } else {
            &stem
        }
    );
    for suffix in 0..10000 {
        let name = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !profiles.iter().any(|p| p.name == name) {
            return name;
        }
    }
    format!("application-igpu-{}", monotonic_ns())
}

fn state_text(state: Placement) -> &'static str {
    match state {
        Placement::TargetGpu => "iGPU activity verified",
        Placement::OtherGpu => "Other GPU active",
        Placement::BothGpus => "Both GPUs active",
        Placement::Waiting => "Waiting for sustained activity",
        Placement::Unavailable => "No readable activity accounting",
    }
}

fn percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.2}%"))
        .unwrap_or_else(|| "—".into())
}

impl Dashboard {
    #[cfg(feature = "screenshot")]
    pub(super) fn initialize_screenshot_workflow(&mut self) {
        let Ok(stage) = std::env::var("IGPU_MAN_SCREENSHOT_WORKFLOW") else {
            return;
        };
        self.tab = 4;
        if stage == "catalog" {
            self.browse_launchers = true;
        }
        if stage == "setup" {
            let (entry, profiles, target) = {
                let state = self.backend.lock().unwrap();
                (
                    state
                        .launchers
                        .iter()
                        .find(|e| e.profile.is_some())
                        .cloned(),
                    state.profiles.clone(),
                    state
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.gpus.iter().find(|g| g.integrated))
                        .map(|g| g.pci.clone()),
                )
            };
            if let (Some(entry), Some(target)) = (entry, target) {
                self.setup_from_launcher(&entry, &profiles, &target);
            }
        }
        if stage == "verify" {
            if let Ok(raw) = std::env::var("IGPU_MAN_SCREENSHOT_TRIAL") {
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    if let (Ok(profile), Ok(process)) = (
                        serde_json::from_value::<Profile>(value["profile"].clone()),
                        serde_json::from_value::<ProcessId>(value["process"].clone()),
                    ) {
                        let verification =
                            client::call("verify_trial", &json!({"process": process})).ok();
                        self.backend.lock().unwrap().trial = Some(Trial {
                            keep_name: profile.name.clone(),
                            profile,
                            process,
                            verification,
                            launcher_exit: None,
                        });
                        self.scroll_to_trial = true;
                    }
                }
            }
        }
    }
    fn setup_from_launcher(
        &mut self,
        entry: &InstalledLauncher,
        profiles: &[Profile],
        target: &str,
    ) {
        if let Some(mut profile) = entry.profile.clone() {
            profile.name = unique_name(&profile.executable, profiles);
            profile.gpu = target.into();
            self.trial_setup = Some(TrialSetup {
                profile,
                previous: vec![],
                matches: vec![entry.clone()],
                selected_launcher: Some(entry.id.clone()),
                create_launcher: true,
            });
            self.browse_launchers = false;
            self.scroll_to_trial = true;
        }
    }

    fn setup_from_candidate(
        &mut self,
        candidate: &Candidate,
        snapshot: &Snapshot,
        entries: &[InstalledLauncher],
        profiles: &[Profile],
        target: &str,
    ) {
        let app = snapshot
            .applications
            .iter()
            .find(|a| a.id == candidate.process);
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.profile.as_ref().is_some_and(|p| {
                    launchers::same_executable(&p.executable, &candidate.executable)
                        || app.is_some_and(|a| {
                            a.processes.iter().any(|member| {
                                launchers::same_executable(&p.executable, &member.executable)
                            })
                        })
                })
            })
            .cloned()
            .collect();
        let saved = candidate
            .profile
            .as_ref()
            .and_then(|name| profiles.iter().find(|p| &p.name == name));
        let mut profile = matches
            .first()
            .and_then(|e| e.profile.clone())
            .or_else(|| saved.cloned())
            .unwrap_or_else(|| Profile {
                executable: candidate.executable.clone(),
                firefox: launch::is_firefox(&candidate.executable),
                ..Profile::default()
            });
        profile.name = unique_name(&profile.executable, profiles);
        profile.gpu = target.into();
        let mut previous = app
            .map(|a| a.processes.iter().map(|p| p.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        previous.push(candidate.process.clone());
        let selected_launcher = matches.first().map(|e| e.id.clone());
        self.trial_setup = Some(TrialSetup {
            profile,
            previous,
            matches,
            selected_launcher,
            create_launcher: true,
        });
        self.scroll_to_trial = true;
    }

    pub(super) fn applications(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let (candidates, entries, profiles, rules, trial, pending, stale) = {
            let state = self.backend.lock().unwrap();
            (
                state.candidates.clone(),
                state.launchers.clone(),
                state.profiles.clone(),
                state.managed_elsewhere.clone(),
                state.trial.clone(),
                state.trial_pending,
                state
                    .last_success
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
                    || monotonic_ns().saturating_sub(snapshot.monotonic_ns) > 3_000_000_000,
            )
        };
        let target = snapshot
            .gpus
            .iter()
            .find(|g| g.integrated)
            .map(|g| g.pci.clone());
        theme::page_title(
            ui,
            "Placement experiments",
            "Try a fresh launch, inspect its GPU activity, then keep the settings if they work.",
        );
        if trial.is_none() && !pending {
            let recoverable = self.backend.lock().unwrap().pending_trials.clone();
            for registration in recoverable {
                let Some(profile) = registration.profile else {
                    continue;
                };
                theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                    ui.label(format!("Unkept launch: {}",profile.desktop.as_ref().map(|d|d.name.as_str()).unwrap_or(&profile.executable)));
                    ui.small("Resume with fresh verification, or discard its setup while leaving the app running.");
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!stale,egui::Button::new("Resume verification")).clicked() {
                            self.trial_setup=Some(TrialSetup {profile:profile.clone(),previous:vec![],matches:vec![],selected_launcher:None,create_launcher:true});
                            self.backend.lock().unwrap().trial=Some(Trial {keep_name:unique_name(&profile.executable,&profiles),profile:profile.clone(),process:registration.process.clone(),verification:None,launcher_exit:None});
                            self.scroll_to_trial=true;
                        }
                        if ui.add_enabled(!stale,egui::Button::new("Discard setup")).clicked() {self.rpc("discard_trial",json!({"process":registration.process}));}
                    });
                });
            }
        }
        let notice = self.backend.lock().unwrap().workflow_notice.clone();
        if !notice.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.colored_label(theme::IGPU, &notice);
                ui.horizontal(|ui| {
                    if ui.button("View profiles").clicked() {
                        self.tab = 2;
                    }
                    if ui.button("Dismiss").clicked() {
                        self.backend.lock().unwrap().workflow_notice.clear();
                    }
                });
            });
        }
        theme::muted(ui, "Candidates are ranked by recent dGPU activity. Treat this as a test list, not a prediction of gaming gains.");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(theme::action("Add installed application…", theme::DGPU))
                .clicked()
            {
                self.browse_launchers = !self.browse_launchers;
            }
            ui.checkbox(&mut self.show_inactive_apps, "Show idle / unmeasured apps");
            if pending {
                ui.spinner();
                ui.label("Starting test…");
            }
        });
        if target.is_none() {
            ui.colored_label(
                theme::WARNING,
                "No iGPU detected. Application tests are unavailable.",
            );
        }
        let can_start = target.is_some() && !stale && !pending && trial.is_none();
        if self.browse_launchers {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::section_title(ui, "Choose an installed app");
                ui.add(egui::TextEdit::singleline(&mut self.launcher_search).hint_text("Search installed apps…").desired_width(f32::INFINITY));
                if ui.button("Refresh apps").clicked() { self.queue(Action::RefreshLaunchers); }
                let query = self.launcher_search.to_lowercase();
                egui::ScrollArea::vertical().id_salt("launcher-catalog").max_height(220.0).show(ui, |ui| {
                    for entry in entries.iter().filter(|e| format!("{} {}", e.name, e.id).to_lowercase().contains(&query)) {
                        ui.push_id(&entry.id, |ui| {
                            egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width((ui.available_width() - 130.0).max(100.0));
                                        ui.strong(&entry.name).on_hover_text(&entry.id);
                                        if let Some(problem) = &entry.problem { theme::muted(ui, problem); }
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_enabled(can_start && entry.profile.is_some(), theme::action("Try on iGPU", theme::IGPU)).clicked() {
                                            self.setup_from_launcher(entry, &profiles, target.as_deref().unwrap());
                                        }
                                    });
                                });
                            });
                            ui.separator();
                        });
                    }
                    if entries.is_empty() { ui.label("No installed launchers loaded. Refresh after the collector connects."); }
                });
            });
        }
        ui.add_space(8.0);
        let visible: Vec<_> = candidates
            .iter()
            .filter(|c| {
                !matches!(
                    c.management,
                    Management::ManagedElsewhere | Management::Infrastructure
                ) && (self.show_inactive_apps
                    || matches!(
                        c.management,
                        Management::Candidate | Management::SavedProfile | Management::IgpuActive
                    ))
            })
            .collect();
        for candidate in visible {
            ui.push_id((candidate.process.pid, candidate.process.start_ticks), |ui| {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 160.0).max(100.0));
                            theme::section_title(ui, &candidate.name);
                        let igpu = snapshot.gpus.iter().any(|g| g.integrated && candidate.observed_gpus.contains(&g.pci));
                        let dgpu = snapshot.gpus.iter().any(|g| !g.integrated && candidate.observed_gpus.contains(&g.pci));
                        let placement = match (igpu,dgpu) { (true,true) => "Both GPUs active", (true,false) => "iGPU active", (false,true) => "dGPU active", _ => "No current measured activity" };
                        let mismatch = candidate.saved_target.as_ref().and_then(|gpu| launch::resolve_gpu(&snapshot.gpus, gpu).ok()).is_some_and(|gpu| candidate.observed_gpus.iter().any(|p| p != &gpu.pci));
                        let color = if mismatch || (igpu && dgpu) { theme::WARNING } else if igpu { theme::IGPU } else if dgpu { theme::DGPU } else { theme::MUTED };
                        theme::badge(ui, if mismatch { if candidate.launch_path.registration.is_some() { "Placement mismatch" } else { "Activity differs from profile" } } else { placement }, color);

                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(can_start, theme::action("Try on iGPU", theme::IGPU)).clicked() {
                                self.setup_from_candidate(candidate, snapshot, &entries, &profiles, target.as_deref().unwrap());
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.columns(3, |columns| {
                        theme::muted(&mut columns[0], "dGPU graphics");
                        columns[0].label(egui::RichText::new(percent(candidate.graphics_percent)).size(20.0).color(theme::DGPU)).on_hover_text(&candidate.source);
                        theme::muted(&mut columns[1], "dGPU video");
                        columns[1].label(egui::RichText::new(percent(candidate.video_percent)).size(20.0).color(theme::DGPU)).on_hover_text("Video includes decode/encode activity; it does not identify a particular stream.");
                        theme::muted(&mut columns[2], "Observation");
                        columns[2].label(egui::RichText::new(format!("{:.0}s / {:.0}s", candidate.active_seconds, candidate.observed_seconds)).size(20.0).color(theme::MUTED)).on_hover_text("Active time / observed time on the dGPU in the last minute.");
                    });
                    ui.add_space(6.0);
                        let path = &candidate.launch_path;
                        let color = if path.kind == LaunchKind::Unregistered && !path.matching_profiles.is_empty() { theme::WARNING } else { theme::MUTED };
                        ui.label(egui::RichText::new(path.kind.label()).color(color)).on_hover_text(&path.explanation);
                        egui::CollapsingHeader::new("Launch path / next launch")
                            .id_salt((candidate.process.pid, candidate.process.start_ticks))
                            .default_open(cfg!(feature = "screenshot") && std::env::var("IGPU_MAN_SCREENSHOT_WORKFLOW").as_deref() == Ok("audit") && !path.matching_profiles.is_empty())
                            .show(ui, |ui| {
                            theme::muted(ui, &path.explanation);
                            ui.small(format!("Other dGPU engines: {}", percent(candidate.other_percent)));
                            if let Some(profile) = &candidate.profile { ui.small(format!("Saved profile: {profile}")); }
                            if let Some(profile) = &path.registered_profile { ui.label(format!("Registered settings: {profile}")); }
                            if let Some(id) = &path.registration { ui.small(format!("Registered root: PID {} · start {} (descendants retain attribution)", id.pid, id.start_ticks)); }
                            if !path.matching_profiles.is_empty() {
                                ui.label("Available profiles (choose the intended target):");
                                for name in &path.matching_profiles {
                                    let command = format!("igpu-man launch '{}'", name.replace('\'', "'\\''"));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.monospace(&command);
                                        if ui.small_button("Copy").clicked() { ui.ctx().copy_text(command); }
                                    });
                                }
                                ui.small("Close the old instance normally before relaunching. Use the separate Sidecar launcher if created, and pin that entry for future launches. No running app is moved or terminated.");
                            } else if path.kind == LaunchKind::Unregistered {
                                ui.small("No exact executable match. A profile may start a wrapper; choose the intended profile manually in Launch settings.");
                            }
                            if ui.button("Open launch settings").clicked() { self.tab = 2; }
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!stale, egui::Button::new(egui::RichText::new("Managed elsewhere").small().color(theme::MUTED)).frame(false)).clicked() {
                            self.rpc("set_management", json!({"executable":candidate.executable,"managed_elsewhere":true}));
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }
        if candidates
            .iter()
            .all(|c| c.management != Management::Candidate)
        {
            theme::muted(ui, "No unmanaged dGPU candidates in this observation window. Enable idle / unmeasured apps to inspect additional clients.");
        }
        ui.collapsing(format!("Managed elsewhere ({})", rules.len()), |ui| {
            ui.label("Excluded from candidate ranking across restarts. Existing profiles and application settings are preserved.");
            for executable in &rules {
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(executable);
                    if ui.add_enabled(!stale, egui::Button::new("Undo")).clicked() { self.rpc("set_management", json!({"executable": executable, "managed_elsewhere": false})); }
                });
            }
        });
        ui.collapsing("Desktop infrastructure and collection coverage", |ui| {
            if let Some(line) = snapshot.diagnostics.kwin_support.as_deref().and_then(|s| s.lines().find(|l| l.contains("OpenGL renderer string:"))) { ui.label(format!("KWin: {}", line.trim())); }
            for c in candidates.iter().filter(|c| c.management == Management::Infrastructure) { ui.label(format!("{} · dGPU graphics {}", c.name, percent(c.graphics_percent))); }
            ui.label(format!("{} accessible user processes; {} permission failures. Client totals do not cover the entire GPU. Desktop infrastructure is excluded from application routing tests.", snapshot.coverage.processes_inspected, snapshot.coverage.permission_denied));
        });
        ui.add_space(12.0);
        if let Some(trial) = trial {
            self.trial_verification(ui, trial, &profiles, stale, snapshot);
        } else if !pending {
            self.trial_editor(ui, snapshot, &entries, &profiles, stale);
        }
        ui.separator();
        ui.collapsing("Inspect processes and raw metrics", |ui| {
            self.application_details(ui, snapshot)
        });
    }

    fn trial_editor(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        entries: &[InstalledLauncher],
        profiles: &[Profile],
        stale: bool,
    ) {
        let mut launch_action = None;
        let Some(setup) = &mut self.trial_setup else {
            return;
        };
        let mut cancel = false;
        let panel = theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("STEP 1 · REVIEW").size(11.0).color(theme::IGPU));
            theme::section_title(ui, "Try a fresh launch");
            theme::muted(ui, "Close the existing app normally before testing. Sidecar leaves running apps alone.");
            let mut selected = setup.selected_launcher.clone();
            egui::ComboBox::from_id_salt("trial-import").width(360.0).selected_text(setup.profile.desktop.as_ref().map(|d| d.name.as_str()).unwrap_or("Manual executable / choose installed launcher"))
                .show_ui(ui, |ui| {
                    let matching: Vec<_> = setup.matches.iter().map(|e| e.id.as_str()).collect();
                    for entry in entries.iter().filter(|e| e.profile.is_some()) {
                        ui.selectable_value(&mut selected, Some(entry.id.clone()), format!("{}{}", if matching.contains(&entry.id.as_str()) { "Match: " } else { "" }, entry.name));
                    }
                });
            if selected != setup.selected_launcher {
                if let Some(entry) = entries.iter().find(|e| Some(&e.id) == selected.as_ref()) {
                    if let Some(mut profile) = entry.profile.clone() {
                        profile.name = setup.profile.name.clone(); profile.gpu = setup.profile.gpu.clone(); setup.profile = profile;
                    }
                }
                setup.selected_launcher = selected;
            }
            if setup.profile.desktop.is_none() { ui.colored_label(theme::WARNING, "No installed launcher matched. Review the executable and supply required arguments, or choose a launcher above."); }
            ui.horizontal(|ui| { ui.label("Profile name"); ui.text_edit_singleline(&mut setup.profile.name); });
            ui.horizontal(|ui| { ui.label("Executable"); ui.monospace(&setup.profile.executable); });
            ui.collapsing("Review launch arguments and settings", |ui| {
                if setup.profile.desktop.is_none() { ui.text_edit_singleline(&mut setup.profile.executable); }
                let mut remove = None;
                for (i,arg) in setup.profile.args.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Argument {}", i+1));
                        // Imported arguments keep their field-code insertion positions.
                        if setup.profile.desktop.is_some() { ui.monospace(arg.as_str()); }
                        else { ui.text_edit_singleline(arg); if ui.button("Remove").clicked() { remove = Some(i); } }
                    });
                }
                if let Some(index) = remove { setup.profile.args.remove(index); }
                if setup.profile.desktop.is_none() && ui.button("Add argument").clicked() { setup.profile.args.push(String::new()); }
                if let Some(cwd) = &setup.profile.working_directory { ui.label(format!("Working directory: {cwd}")); }
                if !setup.profile.environment.is_empty() { ui.label(format!("{} environment assignments from the installed launcher will be preserved. Explicit GPU selection takes precedence.", setup.profile.environment.len())); }
                ui.checkbox(&mut setup.profile.vulkan_only, "Expose only this Vulkan device");
            });
            ui.checkbox(&mut setup.create_launcher, "Create a separate Plasma launcher when kept");
            let conflict = profiles.iter().any(|p| p.name == setup.profile.name);
            let valid = igpu_core::config::validate_profile(&setup.profile).is_ok() && !conflict;
            if conflict { ui.colored_label(theme::WARNING, "That profile name exists. Choose a new name; Keep preserves existing profiles."); }
            let previous_live = setup.previous.iter().any(|id| snapshot.applications.iter().any(|a| a.id == *id || a.processes.iter().any(|p| p.id == *id)));
            ui.horizontal(|ui| {
                if ui.add_enabled(valid && !stale && !previous_live, theme::action("Launch test", theme::IGPU)).clicked() {
                    self.backend.lock().unwrap().trial_pending = true;
                    launch_action = Some(Action::StartTrial { profile: Box::new(setup.profile.clone()), previous: setup.previous.clone() });
                }
                if previous_live { ui.label("Waiting for the existing application to close…"); }
                if ui.button("Cancel setup").clicked() { cancel = true; }
            });
        });
        if let Some(action) = launch_action {
            self.queue(action);
        }
        if self.scroll_to_trial {
            ui.scroll_to_rect(panel.response.rect, Some(egui::Align::TOP));
            self.scroll_to_trial = false;
        }
        if cancel {
            self.trial_setup = None;
        }
    }

    fn trial_verification(
        &mut self,
        ui: &mut egui::Ui,
        mut trial: Trial,
        profiles: &[Profile],
        stale: bool,
        snapshot: &Snapshot,
    ) {
        let mut discard = false;
        let keeping = self.backend.lock().unwrap().trial_keep_pending;
        let panel = theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("STEP 2 · VERIFY").size(11.0).color(theme::IGPU));
            theme::section_title(ui, "Observe rendering and decoding");
            ui.label(format!("Testing {} on {}. Use the app; play a video if decoder verification matters.", trial.profile.desktop.as_ref().map(|d| d.name.as_str()).unwrap_or(&trial.profile.executable), theme::gpu_label(&snapshot.gpus, &trial.profile.gpu)));
            if let Some(verification) = &trial.verification {
                egui::Grid::new("trial-placement").num_columns(2).show(ui, |ui| {
                    for (label,state) in [("Rendering", verification.rendering), ("Video decoding", verification.decoding), ("Other engines", verification.other_engines)] {
                        ui.label(label); theme::badge(ui, state_text(state), match state { Placement::TargetGpu => theme::IGPU, Placement::OtherGpu | Placement::BothGpus => theme::WARNING, _ => theme::MUTED }); ui.end_row();
                    }
                });
                ui.label(&verification.explanation);
                if !verification.application_seen {
                    if let Some(exit) = &trial.launcher_exit { ui.colored_label(theme::WARNING, format!("Launcher exited ({exit}). No verified application is currently tracked. It may have reused an existing instance.")); }
                }
            } else { ui.label("Waiting for fresh collector verification…"); }
            ui.add_space(8.0); ui.separator();
            ui.label(egui::RichText::new("STEP 3 · KEEP").size(11.0).color(theme::IGPU));
            theme::section_title(ui, "Save the tested settings");
            ui.horizontal(|ui| { ui.label("Profile name"); ui.text_edit_singleline(&mut trial.keep_name); });
            let create_launcher = self.trial_setup.as_mut().map(|s| {
                ui.checkbox(&mut s.create_launcher, "Create a separate Plasma launcher"); s.create_launcher
            }).unwrap_or(true);
            let mut proposed = trial.profile.clone(); proposed.name = trial.keep_name.clone();
            let valid = igpu_core::config::validate_profile(&proposed).is_ok() && !profiles.iter().any(|p| p.name == trial.keep_name);
            if !valid { ui.colored_label(theme::WARNING, "Choose a valid, unused profile name."); }
            let verified = trial.verification.as_ref().is_some_and(|v| v.can_keep);
            ui.horizontal(|ui| {
                if ui.add_enabled(verified && valid && !stale && !keeping, theme::action("Keep profile", theme::IGPU)).clicked() {
                    self.backend.lock().unwrap().trial_keep_pending = true;
                    self.rpc("keep_trial", json!({"process": trial.process, "name": trial.keep_name, "create_launcher": create_launcher, "cli_path": cli_path()}));
                }
                if ui.add_enabled(!keeping, egui::Button::new("Discard setup")).clicked() { discard = true; }
                if keeping { ui.spinner(); ui.label("Saving profile…"); }
            });
            ui.small("Keep saves a custom profile. Use Applications to set up ordinary iGPU launches. Discard leaves the running app alone.");
        });
        if self.scroll_to_trial {
            ui.scroll_to_rect(panel.response.rect, Some(egui::Align::TOP));
            self.scroll_to_trial = false;
        }
        let mut state = self.backend.lock().unwrap();
        if discard {
            drop(state);
            self.rpc("discard_trial", json!({"process":trial.process}));
            self.trial_setup = None;
        } else if let Some(current) = &mut state.trial {
            if current.process == trial.process {
                current.keep_name = trial.keep_name;
            }
        }
    }
}

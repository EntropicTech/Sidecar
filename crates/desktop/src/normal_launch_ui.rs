//! Reversible future-launch setup; observed activity remains independent.
use super::*;
use igpu_core::{
    applications::Placement,
    normal_launch::{Preview, Setup},
};

pub(super) struct Editor {
    profile: String,
    desktop_id: String,
    default_browser: bool,
    choices: Vec<(String, String)>,
}
impl Dashboard {
    pub(super) fn begin_normal_setup(&mut self, profile: &Profile) {
        let mut state = self.backend.lock().unwrap();
        let entries: Vec<_> = state
            .launchers
            .iter()
            .filter(|entry| {
                entry.profile.as_ref().is_some_and(|p| {
                    igpu_core::launchers::same_executable(&p.executable, &profile.executable)
                })
            })
            .collect();
        let id = profile
            .desktop
            .as_ref()
            .map(|d| d.id.clone())
            .filter(|id| entries.iter().any(|e| &e.id == id))
            .or_else(|| (entries.len() == 1).then(|| entries[0].id.clone()))
            .unwrap_or_default();
        let choices = entries
            .iter()
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();
        state.normal_preview = None;
        self.normal_setup = Some(Editor {
            profile: profile.name.clone(),
            desktop_id: id,
            default_browser: profile.firefox || launch::is_firefox(&profile.executable),
            choices,
        });
    }
    pub(super) fn normal_rpc(&self, method: &'static str, payload: Value) {
        self.backend.lock().unwrap().normal_busy = true;
        self.queue(Action::Rpc(method, payload));
    }

    pub(super) fn normal_launches(
        &mut self,
        ui: &mut egui::Ui,
        profiles: &[Profile],
        snapshot: Option<&Snapshot>,
    ) {
        let (statuses, preview, busy, candidates, verification, checked) = {
            let state = self.backend.lock().unwrap();
            (
                state.normal_status.clone(),
                state.normal_preview.clone(),
                state.normal_busy,
                state
                    .candidates
                    .iter()
                    .filter(|c| {
                        state.normal_status.iter().any(|s| {
                            c.launch_path.matching_profiles.contains(&s.profile)
                                || c.launch_path.registered_profile.as_deref() == Some(&s.profile)
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
                state.normal_verification.clone(),
                state.normal_checked,
            )
        };
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            theme::section_title(ui, "Normal-launch settings");
            if ui
                .add_enabled(!busy, egui::Button::new("Check settings"))
                .clicked()
            {
                self.normal_rpc("normal_launch_status", json!({}));
            }
        });
        ui.small("These settings survive reboot. Verify activity separately after a fresh app launch in Applications.");
        if let Some(checked) = checked {
            ui.small(format!("Settings last checked {} seconds ago. Check settings after changing desktop configuration.",checked.elapsed().as_secs()));
        }
        for status in &statuses {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&status.profile);
                    theme::badge(ui, &status.phase, if status.phase == "active" && status.conflicts.is_empty() {theme::IGPU} else {theme::WARNING});
                    ui.label(&status.desktop_id);
                });
                let label = |v:Option<bool>| match v {Some(true)=>"Configured",Some(false)=>"Needs attention",None=>"Unchanged"};
                egui::Grid::new(("normal-status",&status.profile)).num_columns(2).show(ui, |ui| {
                    for (name,state) in [("Usual launcher",Some(status.launcher_ok)),("Previous login changes",status.login_ok),("Browser links / HTML",status.browser_defaults_ok),("Plasma restore exclusion",status.restore_excluded)] {
                        if state.is_none() && name != "Usual launcher" { continue; }
                        ui.label(name); ui.colored_label(if state == Some(false) {theme::WARNING} else {theme::MUTED},label(state)); ui.end_row();
                    }
                });
                for conflict in &status.conflicts { ui.colored_label(theme::WARNING,conflict); }
                let running:Vec<_> = candidates.iter().filter(|c|c.launch_path.registered_profile.as_deref() == Some(&status.profile)).collect();
                if running.is_empty() {
                    if candidates.iter().any(|c|c.launch_path.matching_profiles.contains(&status.profile)) {
                        ui.colored_label(theme::WARNING,"Placement now: app observed without this managed launch. Close it normally and reopen using its usual launcher; inspect Applications for GPU activity.");
                    } else {ui.label("Placement now: no matching managed launch observed. Open the app normally, then check Applications.");}
                } else {
                    for candidate in running {
                        if snapshot.is_some_and(|s|monotonic_ns().saturating_sub(s.monotonic_ns) <= 5_000_000_000) {
                            if let Some(process) = &candidate.launch_path.registration {
                                if let Some((_, evidence)) = verification.as_ref().filter(|(id,_)|id == process) {
                                    for (label, placement) in [("Rendering",evidence.rendering),("Video decoding",evidence.decoding)] {
                                        let text = match placement {Placement::TargetGpu=>"iGPU activity verified",Placement::OtherGpu=>"Other GPU active",Placement::BothGpus=>"Both GPUs active",Placement::Waiting=>"Waiting for sustained activity",Placement::Unavailable=>"Accounting unavailable"};
                                        ui.colored_label(if placement == Placement::TargetGpu {theme::IGPU} else {theme::WARNING}, format!("Last check · {label}: {text}"));
                                    }
                                    ui.small("Play a video and check again to verify decoding. Missing decoder activity alone does not prove software decoding.");
                                }
                                ui.horizontal(|ui| {
                                    ui.label(format!("Managed process {} · use Applications for live engine activity",candidate.process.pid));
                                    if ui.add_enabled(!busy,egui::Button::new("Verify rendering & decoding")).clicked() { self.rpc("verify_trial",json!({"process":process})); }
                                });
                            }
                        } else { ui.colored_label(theme::WARNING,"Current placement unavailable: collector sample is stale."); }
                    }
                }
                ui.horizontal(|ui| {
                    if status.phase == "installing" && ui.add_enabled(!busy,egui::Button::new("Finish interrupted setup")).clicked() { self.normal_rpc("normal_launch_resume",json!({"profile":status.profile})); }
                    if ui.add_enabled(!busy,egui::Button::new("Undo normal-launch changes")).clicked() { self.normal_rpc("normal_launch_undo",json!({"profile":status.profile})); }
                    if ui.button("View application activity").clicked() { self.tab = 1; }
                });
                ui.small("Undo restores the owned changes shown above. Running apps stay open; conflicting outside edits are preserved.");
            });
        }
        let Some(mut editor) = self.normal_setup.take() else {
            return;
        };
        if statuses.iter().any(|s| s.profile == editor.profile) {
            return;
        }
        let Some(profile) = profiles.iter().find(|p| p.name == editor.profile) else {
            self.normal_setup = Some(editor);
            ui.label("Loading saved profile…");
            return;
        };
        let mut close = false;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::section_title(ui, &format!("Normal launches · {}",profile.name));
            ui.label("Review the changes once; afterward use your usual menu, pinned shortcut or browser link.");
            let before = (editor.desktop_id.clone(),editor.default_browser);
            let choices=&editor.choices;
            egui::ComboBox::from_id_salt("normal-desktop-id").selected_text(if editor.desktop_id.is_empty() {"Choose the usual application launcher"} else {&editor.desktop_id}).show_ui(ui,|ui| {
                for (id,name) in choices { ui.selectable_value(&mut editor.desktop_id,id.clone(),format!("{name} ({id})")); }
            });
            if choices.is_empty() { ui.colored_label(theme::WARNING,"No supported matching launcher. Refresh installed applications, or import the original launcher as a profile first."); }
            if profile.firefox || launch::is_firefox(&profile.executable) { ui.checkbox(&mut editor.default_browser,"Use this launcher for web links and HTML files"); }
            if before != (editor.desktop_id.clone(),editor.default_browser) { self.backend.lock().unwrap().normal_preview = None; }
            let setup = Setup {profile:profile.name.clone(),desktop_id:editor.desktop_id.clone(),cli_path:cli_path(),login:false,default_browser:editor.default_browser};
            let applicable:Option<&Preview> = preview.as_ref().filter(|p|p.setup == setup);
            let igpu = snapshot.and_then(|s|launch::resolve_gpu(&s.gpus,&profile.gpu).ok()).is_some_and(|g|g.integrated);
            if !igpu {ui.colored_label(theme::WARNING,"Choose a saved iGPU profile before applying normal-launch routing.");}
            if let Some(preview) = applicable {
                ui.strong("Changes to review");
                for change in &preview.changes {ui.label(format!("• {change}"));}
                for warning in &preview.warnings {ui.colored_label(theme::WARNING,warning);}
                if ui.add_enabled(!busy && igpu,theme::action("Apply normal-launch settings",theme::IGPU)).clicked() {self.normal_rpc("normal_launch_apply",json!({"setup":setup,"token":preview.token}));}
            } else if ui.add_enabled(!busy && igpu && !editor.desktop_id.is_empty(),theme::action("Preview changes",theme::IGPU)).clicked() {self.normal_rpc("normal_launch_preview",serde_json::to_value(setup).unwrap());}
            if ui.add_enabled(!busy,egui::Button::new("Cancel setup")).clicked() {close=true;}
            if busy {ui.spinner();}
        });
        if !close {
            self.normal_setup = Some(editor);
        }
    }
}

#[cfg(feature = "screenshot")]
impl Dashboard {
    pub(super) fn initialize_normal_screenshot(&mut self) {
        let Ok(stage) = std::env::var("IGPU_MAN_SCREENSHOT_NORMAL") else {
            return;
        };
        self.tab = 2;
        if let Ok(status) = client::call("normal_launch_status", &json!({})) {
            let mut state = self.backend.lock().unwrap();
            state.normal_status = status;
            state.normal_checked = Some(Instant::now());
        }
        if stage == "preview" {
            let profile = self.backend.lock().unwrap().profiles.first().cloned();
            if let Some(profile) = profile {
                self.begin_normal_setup(&profile);
                if let Some(editor) = &self.normal_setup {
                    let setup = Setup {
                        profile: editor.profile.clone(),
                        desktop_id: editor.desktop_id.clone(),
                        cli_path: cli_path(),
                        login: false,
                        default_browser: editor.default_browser,
                    };
                    if let Ok(preview) = client::call("normal_launch_preview", &setup) {
                        self.backend.lock().unwrap().normal_preview = Some(preview);
                    }
                }
            }
        }
    }
}

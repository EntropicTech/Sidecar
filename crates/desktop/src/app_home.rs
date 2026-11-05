//! The primary task: choose an installed app, use the iGPU, observe, undo.
use super::*;
use igpu_core::{
    applications::{Candidate, Management, Placement},
    normal_launch::AppSetup,
};

fn app_name(profile: Option<&Profile>, fallback: &str) -> String {
    profile
        .and_then(|p| p.desktop.as_ref().map(|d| d.name.clone()))
        .or_else(|| {
            profile.and_then(|p| {
                std::path::Path::new(&p.executable)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
        })
        .unwrap_or_else(|| fallback.trim_end_matches(".desktop").into())
}
// Visual-only helpers: no icon discovery, timers or extra collector requests.
fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(egui::Stroke::new(1.0_f32, theme::tint(theme::BORDER, 150)))
        .corner_radius(8)
        .inner_margin(12)
}
fn eyebrow(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(
        egui::RichText::new(text.into())
            .size(11.0)
            .strong()
            .color(theme::MUTED),
    );
}
fn avatar(ui: &mut egui::Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 7.0, theme::tint(theme::IGPU, 20));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        name.chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string(),
        egui::FontId::proportional(16.0),
        theme::IGPU,
    );
}
fn activity(ui: &mut egui::Ui, candidate: Option<&Candidate>, stale: bool, inline: bool) {
    let evidence = candidate.and_then(|c| c.verification.as_ref());
    let item = |ui: &mut egui::Ui, label: &str, placement: Option<Placement>| {
        let (text, color, hint) = if stale {
            (
                "Stale",
                theme::WARNING,
                "The collector is disconnected or stale. Last readings cannot verify placement.",
            )
        } else if candidate.is_none() {
            (
                "Not observed",
                theme::MUTED,
                "Open the app normally to check its GPU activity.",
            )
        } else if evidence.is_none() {
            ("Reopen to verify", theme::WARNING, "No matching managed-launch evidence. Close and reopen the app normally when ready.")
        } else {
            match placement {
                Some(Placement::TargetGpu) => ("iGPU confirmed", theme::IGPU, "Sustained recent activity measured on the iGPU."),
                Some(Placement::OtherGpu) => ("Other GPU active", theme::WARNING, "Recent activity measured on another GPU."),
                Some(Placement::BothGpus) => ("Both GPUs active", theme::WARNING, "Recent activity measured on both GPUs."),
                Some(Placement::Waiting) => ("Waiting", theme::MUTED, "Waiting for sustained engine activity. Use the app; play a video for decoder evidence."),
                _ => ("Unavailable", theme::MUTED, "No readable engine evidence. Missing decoder activity does not prove software decoding."),
            }
        };
        if inline {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{label}:"))
                        .small()
                        .color(theme::MUTED),
                );
                ui.label(egui::RichText::new(text).small().color(color))
                    .on_hover_text(hint);
            });
        } else {
            ui.vertical(|ui| {
                ui.set_width(132.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(egui::RichText::new(label).size(11.0).color(theme::MUTED));
                ui.label(egui::RichText::new(text).color(color))
                    .on_hover_text(hint);
            });
        }
    };
    if inline {
        ui.horizontal_wrapped(|ui| {
            item(ui, "Rendering", evidence.map(|e| e.rendering));
            item(ui, "Video decoding", evidence.map(|e| e.decoding));
        });
    } else {
        ui.horizontal(|ui| {
            item(ui, "Rendering", evidence.map(|e| e.rendering));
            item(ui, "Video decoding", evidence.map(|e| e.decoding));
        });
    }
}
impl Dashboard {
    fn choose_app(&mut self, id: &str) {
        self.app_setup = Some(id.into());
        self.backend.lock().unwrap().app_preview = None;
        self.normal_rpc(
            "app_route_preview",
            serde_json::to_value(AppSetup {
                desktop_id: id.into(),
                cli_path: cli_path(),
            })
            .unwrap(),
        );
    }
    pub(super) fn app_home(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        profiles: &[Profile],
        stale: bool,
    ) {
        let (statuses, candidates, entries, matches, busy, preview, notice, rules) = {
            let state = self.backend.lock().unwrap();
            (
                state.normal_status.clone(),
                state.candidates.clone(),
                if self.browse_launchers {
                    state
                        .launchers
                        .iter()
                        .map(|e| {
                            (
                                e.id.clone(),
                                e.name.clone(),
                                e.problem.clone(),
                                e.profile.is_some(),
                                state.managed_elsewhere.iter().any(|rule| {
                                    state
                                        .launcher_matches
                                        .get(rule)
                                        .is_some_and(|ids| ids.contains(&e.id))
                                }),
                            )
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                },
                state
                    .candidates
                    .iter()
                    .filter_map(|c| {
                        state
                            .launcher_matches
                            .get(&c.executable)
                            .map(|ids| (c.executable.clone(), ids.clone()))
                    })
                    .collect::<BTreeMap<_, _>>(),
                state.normal_busy,
                state.app_preview.clone(),
                state.workflow_notice.clone(),
                state.managed_elsewhere.clone(),
            )
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 118.0).max(120.0));
                ui.label(egui::RichText::new("Applications").size(28.0).strong());
                ui.label(
                    egui::RichText::new("Choose where your everyday apps run.").color(theme::MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!busy, theme::action("+ Add app", theme::IGPU))
                    .clicked()
                {
                    self.browse_launchers = !self.browse_launchers;
                }
            });
        });
        ui.add_space(10.0);
        if !notice.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme::IGPU, &notice);
                if ui.small_button("Dismiss").clicked() {
                    self.backend.lock().unwrap().workflow_notice.clear();
                }
            });
        }
        let igpu = snapshot.gpus.iter().filter(|g| g.integrated).count() == 1;
        if !igpu {
            ui.colored_label(theme::WARNING,"A single iGPU is needed for quick setup. Check GPU Activity or use Launch Settings.");
        }
        if statuses.is_empty() && !self.browse_launchers {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Make room on your main GPU")
                        .size(20.0)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(
                        "Choose a browser, chat app or another side app to use the iGPU.",
                    )
                    .color(theme::MUTED),
                );
                ui.add_space(6.0);
                if ui
                    .add_enabled(!busy, theme::action("Choose your first app", theme::IGPU))
                    .clicked()
                {
                    self.browse_launchers = true;
                }
                ui.add_space(8.0);
                ui.small("Keep using your usual shortcut. Undo is always available.");
                ui.add_space(12.0);
            });
        }
        if !statuses.is_empty() {
            eyebrow(
                ui,
                format!(
                    "SET TO USE iGPU  ·  {} {}",
                    statuses.len(),
                    if statuses.len() == 1 { "APP" } else { "APPS" }
                ),
            );
            ui.add_space(4.0);
        }
        for status in &statuses {
            let profile = profiles.iter().find(|p| p.name == status.profile);
            let name = app_name(profile, &status.desktop_id);
            let running = candidates
                .iter()
                .find(|c| c.launch_path.registered_profile.as_deref() == Some(&status.profile))
                .or_else(|| {
                    candidates
                        .iter()
                        .find(|c| c.launch_path.matching_profiles.contains(&status.profile))
                });
            ui.push_id(&status.profile, |ui| {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let wide = ui.available_width() >= 850.0;
                    ui.horizontal(|ui| {
                        avatar(ui, &name);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.set_width((ui.available_width() - if wide { 548.0 } else { 250.0 }).max(100.0));
                            ui.label(egui::RichText::new(&name).size(16.0).strong());
                            let healthy = status.phase == "active" && status.launcher_ok && status.conflicts.is_empty();
                            ui.label(egui::RichText::new(if healthy { "Normal launches: iGPU" } else { "Shortcut needs attention" })
                                .small().color(if healthy { theme::MUTED } else { theme::WARNING }));
                        });
                        if wide { activity(ui, running, stale, false); }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.menu_button("Details", |ui| {
                                ui.set_max_width(400.0);
                                ui.small(format!("Launcher: {}", status.desktop_id));
                                if status.phase == "undoing" { ui.colored_label(theme::WARNING, "Undo was interrupted. Choose Undo again to finish safely."); }
                                ui.label("Rendering and video decoding are checked separately. Missing decoder activity is inconclusive.");
                                ui.label("Use the app to check rendering; play a video to check decoding. Direct commands and some restored sessions can bypass the shortcut.");
                                if ui.add_enabled(!busy, egui::Button::new("Check shortcut")).clicked() {
                                    self.normal_rpc("normal_launch_status", json!({}));
                                    ui.close_menu();
                                }
                            });
                            if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new("Undo").color(theme::MUTED)).frame(false))
                                .on_hover_text("Restore the original launcher. Running apps stay open.").clicked() { self.normal_rpc("normal_launch_undo", json!({"profile":status.profile})); }
                            if ui.add_enabled(!busy && !stale && status.launcher_ok, egui::Button::new("Open app")).clicked() {
                                if let Err(error) = launch_cli(&["launch", "--normal", &status.profile]) { self.backend.lock().unwrap().error = error; }
                            }
                        });
                    });
                    if !wide { activity(ui, running, stale, true); }
                    if status.login_ok.is_some() || status.phase == "removing_login" {
                        ui.colored_label(theme::WARNING, "An earlier version changed app login startup. Remove those changes while keeping the iGPU shortcut.");
                        if ui.add_enabled(!busy, egui::Button::new("Remove previous login changes")).clicked() { self.normal_rpc("normal_launch_remove_login", json!({"profile":status.profile})); }
                    }
                    if status.phase == "installing" && ui.add_enabled(!busy, egui::Button::new("Finish setup")).clicked() { self.normal_rpc("normal_launch_resume", json!({"profile":status.profile})); }
                    if status.phase == "undoing" { ui.colored_label(theme::WARNING, "Undo was interrupted. Choose Undo again to finish."); }
                    // Recovery and conflicts remain visible outside the Details menu.
                    for conflict in &status.conflicts { ui.colored_label(theme::WARNING, conflict); }
                });
            });
            ui.add_space(6.0);
        }
        let can_apply = igpu && !stale && !busy;
        if self.browse_launchers {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Add an app").size(19.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Done").clicked() { self.browse_launchers = false; }
                        if ui.add_enabled(!busy, egui::Button::new("Refresh").frame(false)).clicked() { self.queue(Action::RefreshLaunchers); }
                    });
                });
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut self.launcher_search).hint_text("Search installed apps…").desired_width(f32::INFINITY).margin(egui::vec2(12.0, 10.0)));
                ui.add_space(6.0);
                let query = self.launcher_search.to_lowercase();
                egui::ScrollArea::vertical().id_salt("app-home-catalog").max_height(320.0).show(ui, |ui| {
                    let mut found = false;
                    for (id, name, problem, supported, excluded) in entries.iter().filter(|(id,name,_,_,_)| !statuses.iter().any(|s|s.desktop_id == *id) && format!("{name} {id}").to_lowercase().contains(&query)) {
                        found = true;
                        ui.push_id(id, |ui| {
                            egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 9)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    avatar(ui, name);
                                    ui.vertical(|ui| {
 ui.set_width((ui.available_width() - 116.0).max(80.0));
                                        ui.strong(name);
                                        if *excluded { ui.label(egui::RichText::new("Managed elsewhere").small().color(theme::MUTED)); }
                                        if let Some(problem) = problem { ui.label(egui::RichText::new(problem).small().color(theme::MUTED)); }
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_enabled(can_apply && *supported && !excluded, theme::action("Use iGPU", theme::IGPU)).clicked() { self.choose_app(id); }
                                    });
                                });
                            });
                        });
                        ui.separator();
                    }
                    if !found { ui.add_space(12.0); ui.label(egui::RichText::new("No matching apps. Try another name or refresh the list.").color(theme::MUTED)); ui.add_space(12.0); }
                });
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Only the shortcut changes. Running apps stay open; nothing is added to login startup.").small().color(theme::MUTED));
            });
        }
        let suggestions: Vec<_> = candidates
            .iter()
            .filter(|c| {
                if matches.get(&c.executable).is_none_or(|ids| ids.is_empty()) {
                    return false;
                }

                matches!(
                    c.management,
                    Management::Candidate | Management::SavedProfile
                ) && !statuses
                    .iter()
                    .any(|s| c.launch_path.matching_profiles.contains(&s.profile))
                    && snapshot
                        .gpus
                        .iter()
                        .any(|g| !g.integrated && c.observed_gpus.contains(&g.pci))
            })
            .take(8)
            .collect();
        if !suggestions.is_empty() {
            ui.add_space(14.0);
            eyebrow(ui, "ON YOUR MAIN GPU");
            ui.label(egui::RichText::new("Side apps you could move. Measured activity is not a prediction of gaming gains.").small().color(theme::MUTED));
            for candidate in suggestions {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        avatar(ui, &candidate.name);
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 122.0).max(80.0));
                            ui.strong(&candidate.name);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Graphics {}  ·  Video {}",
                                    candidate
                                        .graphics_percent
                                        .map(|v| format!("{v:.1}%"))
                                        .unwrap_or_else(|| "unavailable".into()),
                                    candidate
                                        .video_percent
                                        .map(|v| format!("{v:.1}%"))
                                        .unwrap_or_else(|| "unavailable".into())
                                ))
                                .small()
                                .color(theme::MUTED),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ids = matches.get(&candidate.executable);
                            if let Some(ids) = ids.filter(|ids| ids.len() == 1) {
                                if ui
                                    .add_enabled(can_apply, theme::action("Use iGPU", theme::IGPU))
                                    .clicked()
                                {
                                    self.choose_app(&ids[0]);
                                }
                            } else if ui
                                .add_enabled(can_apply, egui::Button::new("Choose app…"))
                                .clicked()
                            {
                                self.launcher_search = candidate.name.clone();
                                self.browse_launchers = true;
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(
                                    egui::RichText::new("Managed elsewhere")
                                        .small()
                                        .color(theme::MUTED),
                                )
                                .frame(false),
                            )
                            .on_hover_text(
                                "Exclude this app when another workflow controls its GPU.",
                            )
                            .clicked()
                        {
                            self.rpc(
                                "set_management",
                                json!({"executable":candidate.executable,"managed_elsewhere":true}),
                            );
                        }
                    });
                });
                ui.add_space(8.0);
            }
        }
        if !rules.is_empty() {
            ui.collapsing("Managed elsewhere", |ui| {
                for executable in &rules {
                    ui.horizontal(|ui| {
                        ui.label(
                            std::path::Path::new(executable)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                        );
                        if ui.button("Undo exclusion").clicked() {
                            self.rpc(
                                "set_management",
                                json!({"executable":executable,"managed_elsewhere":false}),
                            );
                        }
                    });
                }
            });
        }
        let Some(id) = self.app_setup.clone() else {
            return;
        };
        if statuses.iter().any(|s| s.desktop_id == id) {
            self.app_setup = None;
            self.browse_launchers = false;
            return;
        }
        let mut close = false;
        let dialog=egui::Modal::new(egui::Id::new("simple-app-setup")).frame(egui::Frame::popup(ui.style()).fill(theme::PANEL).corner_radius(14).inner_margin(24)).show(ui.ctx(),|ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 12.0;
            if let Some(preview)=preview.as_ref().filter(|p|p.setup.desktop_id==id) {
                eyebrow(ui, "USE iGPU FOR NORMAL LAUNCHES");
                ui.label(egui::RichText::new(&preview.application).size(24.0).strong());
                ui.label("Its usual shortcut will request the iGPU whenever you open it.");
                ui.label("If the app is already open, close and reopen it when ready. Sidecar will check its GPU activity here.");
                ui.small("This does not start the app at login. Undo restores the original shortcut.");
                ui.collapsing("What changes?",|ui|{for warning in &preview.warnings {ui.label(warning);}ui.small(format!("Launcher: {}",preview.setup.desktop_id));});
                ui.add_space(8.0);
                if busy {ui.spinner();ui.label("Updating shortcut…");}
                ui.horizontal(|ui|{
                    if ui.add_enabled(!busy,theme::action("Use iGPU",theme::IGPU)).clicked(){self.normal_rpc("app_route_apply",json!({"setup":preview.setup,"token":preview.token}));}
                    if ui.add_enabled(!busy,egui::Button::new("Cancel")).clicked(){close=true;}
                });
            }else if busy {ui.spinner();ui.label("Checking the app's shortcut…");}
            else {ui.label("Setup could not be prepared. See the error above, refresh the app list and retry.");if ui.button("Close").clicked(){close=true;}}
        });
        if dialog.should_close() && !busy {
            close = true;
        }
        if close {
            self.app_setup = None;
            self.backend.lock().unwrap().app_preview = None;
        }
    }
}

#[cfg(feature = "screenshot")]
impl Dashboard {
    pub(super) fn initialize_app_home_screenshot(&mut self) {
        let Ok(stage) = std::env::var("IGPU_MAN_SCREENSHOT_APP_HOME") else {
            return;
        };
        self.tab = 1;
        if let Ok(status) = client::call("normal_launch_status", &json!({})) {
            self.backend.lock().unwrap().normal_status = status;
        }
        if stage == "catalog" {
            self.browse_launchers = true;
        }
        if stage == "preview" {
            let entry = self
                .backend
                .lock()
                .unwrap()
                .launchers
                .iter()
                .find(|e| e.profile.is_some())
                .cloned();
            if let Some(entry) = entry {
                let setup = AppSetup {
                    desktop_id: entry.id,
                    cli_path: cli_path(),
                };
                self.app_setup = Some(setup.desktop_id.clone());
                if let Ok(preview) = client::call("app_route_preview", &setup) {
                    self.backend.lock().unwrap().app_preview = Some(preview);
                }
            }
        }
    }
}

//! Optional custom launch settings. Ordinary routing stays in Applications.
use super::*;

impl Dashboard {
    pub(super) fn profiles(
        &mut self,
        ui: &mut egui::Ui,
        profiles: &[Profile],
        snapshot: Option<&Snapshot>,
    ) {
        ui.label(
            egui::RichText::new("Launch Settings")
                .size(28.0)
                .strong()
                .color(theme::TEXT),
        );
        theme::muted(
            ui,
            "Custom profiles for explicit launches. For everyday app routing, use Applications.",
        );
        ui.add_space(10.0);
        let editor_id = ui.make_persistent_id("profile-editor");
        ui.horizontal(|ui| {
            theme::section_title(ui, "Saved profiles");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::action("Open profile editor", theme::IGPU))
                    .clicked()
                {
                    theme::open_section(ui.ctx(), editor_id);
                }
            });
        });
        ui.add_space(4.0);
        if profiles.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::muted(ui, "No custom profiles yet. Open the editor to choose an executable, arguments and GPU.");
            });
        }
        for profile in profiles {
            let managed = self
                .backend
                .lock()
                .unwrap()
                .normal_status
                .iter()
                .any(|s| s.profile == profile.name);
            let gpus = snapshot.map(|s| s.gpus.as_slice()).unwrap_or(&[]);
            let color = launch::resolve_gpu(gpus, &profile.gpu)
                .map(theme::gpu_color)
                .unwrap_or(theme::MUTED);
            let executable = std::path::Path::new(&profile.executable)
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| profile.executable.as_str().into());
            ui.push_id(&profile.name, |ui| {
                theme::card().inner_margin(12).corner_radius(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let wide = ui.available_width() >= 850.0;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.set_width((ui.available_width() - if wide { 480.0 } else { 174.0 }).max(100.0));
                            ui.label(egui::RichText::new(&profile.name).size(16.0).strong().color(theme::TEXT));
                            if wide {
                                ui.label(egui::RichText::new(executable.as_ref()).small().color(theme::MUTED)).on_hover_text(&profile.executable);
                            } else {
                                ui.label(egui::RichText::new(theme::gpu_label(gpus, &profile.gpu)).small().color(color)).on_hover_text(&profile.gpu);
                            }
                        });
                        if wide {
                            ui.vertical(|ui| {
                                ui.set_width(280.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(egui::RichText::new(if managed { "Target · normal launches" } else { "Launch target" }).size(11.0).color(theme::MUTED));
                                ui.label(egui::RichText::new(theme::gpu_label(gpus, &profile.gpu)).small().color(color)).on_hover_text(&profile.gpu);
                            });
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(theme::action("Launch", color)).clicked() {
                                let result = if (profile.firefox || launch::is_firefox(&profile.executable)) && launch::firefox_running(std::path::Path::new("/proc"), std::process::id()) {
                                    Err("Firefox is already running. Fully exit it before launching a different GPU profile.".into())
                                } else { launch_cli(&["launch", &profile.name]) };
                                if let Err(error) = result { self.backend.lock().unwrap().error = error; }
                            }
                            ui.menu_button("More", |ui| {
                                ui.set_max_width(420.0);
                                ui.label(egui::RichText::new(&profile.executable).small().color(theme::MUTED));
                                if managed { ui.colored_label(theme::IGPU, "Used by a normal launcher. Undo routing before editing or deleting."); }
                                ui.separator();
                                if ui.add_enabled(!managed, egui::Button::new("Edit profile")).on_hover_text("Undo normal-launch management before editing a managed profile").clicked() {
                                    self.profile_extras = Some(profile.clone());
                                    self.name = profile.name.clone(); self.executable = profile.executable.clone(); self.args = profile.args.clone();
                                    self.gpu = profile.gpu.clone(); self.firefox = profile.firefox; self.vulkan_only = profile.vulkan_only;
                                    self.record = profile.record; self.mangohud = profile.mangohud;
                                    theme::open_section(ui.ctx(), editor_id);
                                    ui.close_menu();
                                }
                                if ui.button("Create separate launcher").clicked() {
                                    self.rpc("create_launcher", json!({"name":profile.name,"cli_path":cli_path()}));
                                    ui.close_menu();
                                }
                                if ui.add_enabled(!managed, egui::Button::new("Configure normal launches…")).clicked() {
                                    self.begin_normal_setup(profile);
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.add_enabled(!managed, egui::Button::new(egui::RichText::new("Delete profile").color(theme::DANGER))).on_hover_text("Undo normal-launch management first").clicked() {
                                    self.rpc("delete_profile", json!({"name": profile.name})); ui.close_menu();
                                }
                            });
                        });
                    });
                });
            });
            ui.add_space(6.0);
        }
        let conflicts: Vec<_> = self
            .backend
            .lock()
            .unwrap()
            .normal_status
            .iter()
            .flat_map(|s| s.conflicts.iter().cloned())
            .collect();
        for conflict in conflicts {
            ui.colored_label(theme::WARNING, conflict);
        }
        egui::CollapsingHeader::new("Normal-launch routing")
            .id_salt("advanced-normal-routing")
            .default_open(
                cfg!(feature = "screenshot") && std::env::var("IGPU_MAN_SCREENSHOT_NORMAL").is_ok(),
            )
            .open(self.normal_setup.as_ref().map(|_| true))
            .show(ui, |ui| self.normal_launches(ui, profiles, snapshot));
        ui.add_space(6.0);
        theme::collapsible(
            ui,
            editor_id,
            "Profile editor",
            !self.name.is_empty(),
            |ui| {
                theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::muted(ui, "Settings apply on the next launch. Running applications keep their current GPU.");
        egui::Grid::new("profile-fields")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Profile name");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width((ui.available_width() - 8.0).max(100.0)));
                ui.end_row();
                ui.label("Executable");
                ui.add(egui::TextEdit::singleline(&mut self.executable).desired_width((ui.available_width() - 8.0).max(100.0)));
                ui.end_row();
                ui.label("Arguments");
                ui.vertical(|ui| {
                    let mut remove = None;
                    for (index, argument) in self.args.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.push_id(index, |ui| {
                                ui.text_edit_singleline(argument);
                            });
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    if let Some(index) = remove {
                        self.args.remove(index);
                    }
                    if ui.button("Add argument").clicked() {
                        self.args.push(String::new());
                    }
                });
                ui.end_row();
                ui.label("GPU");
                egui::ComboBox::from_id_salt("target-gpu")
                    .selected_text(theme::gpu_label(snapshot.map(|s|s.gpus.as_slice()).unwrap_or(&[]), &self.gpu))
                    .show_ui(ui, |ui| {
                        if let Some(snapshot) = snapshot {
                            for gpu in &snapshot.gpus {
                                ui.selectable_value(
                                    &mut self.gpu,
                                    gpu.pci.clone(),
                                    theme::gpu_label(&snapshot.gpus, &gpu.pci),
                                );
                            }
                        }
                    });
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.collapsing("Rendering & recording options", |ui| {
        ui.checkbox(
            &mut self.firefox,
            "Firefox adapter (explicit render device; requires fresh launch)",
        );
        ui.checkbox(&mut self.vulkan_only, "Expose only the selected Vulkan GPU");
        ui.checkbox(
            &mut self.record,
            "Start a telemetry recording with this launch",
        );
        ui.add_enabled_ui(self.record, |ui| {
            ui.checkbox(
                &mut self.mangohud,
                "MangoHud frame log (first 120 seconds, native games; no gamescope)",
            );
        });
        });
        if !self.record { self.mangohud = false; }
        if ui.add(theme::action("Save profile", theme::IGPU)).clicked() {
            let profile = Profile {
                name: self.name.clone(),
                executable: self.executable.clone(),
                args: self.args.clone(),
                gpu: self.gpu.clone(),
                firefox: self.firefox,
                vulkan_only: self.vulkan_only,
                record: self.record,
                mangohud: self.mangohud,
                ..self.profile_extras.clone().unwrap_or_default()
            };
            self.rpc("save_profile", serde_json::to_value(profile).unwrap());
        }
        });
            },
        );
        ui.add_space(6.0);
        ui.collapsing("Steam launch commands", |ui| {
            if let Some(snapshot) = snapshot {
                for gpu in &snapshot.gpus {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(theme::gpu_label(&snapshot.gpus, &gpu.pci))
                                .small()
                                .color(theme::gpu_color(gpu)),
                        );
                        if ui.button("Copy launch command").clicked() {
                            ui.ctx().copy_text(launch::steam_command(gpu));
                        }
                    });
                }
            }
        });
    }
}

#[cfg(feature = "screenshot")]
impl Dashboard {
    pub(super) fn initialize_profiles_screenshot(&mut self) {
        if std::env::var("IGPU_MAN_SCREENSHOT_ADVANCED").as_deref() == Ok("editor") {
            self.tab = 2;
            self.name = "custom-example".into();
        }
    }
}

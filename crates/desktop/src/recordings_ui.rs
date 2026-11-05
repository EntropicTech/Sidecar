//! Selected recordings, identity-bound notes and optional analysis tools.
use super::*;

impl Dashboard {
    pub(super) fn recordings(&mut self, ui: &mut egui::Ui, recordings: &[Recording]) {
        theme::page_title(
            ui,
            "Saved recordings",
            "Optional experiments to compare placement and frame consistency.",
        );
        if self.backend.lock().unwrap().recording_busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Recording work in progress; live monitoring continues.");
            });
        }
        theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::section_title(ui, "Capture a run");
        theme::muted(ui, "Use the same scene and settings. Give repeated runs the same condition label.");
        ui.add(egui::TextEdit::singleline(&mut self.recording_label).hint_text("Condition label").desired_width(f32::INFINITY).margin(egui::vec2(10.0, 8.0)));
        ui.horizontal_wrapped(|ui| {

            if ui
                .add(theme::action("Start recording", theme::MARKER))
                .clicked()
            {
                self.rpc("start", json!({"label": self.recording_label}));
            }
            if ui
                .add(egui::Button::new("Stop recording"))
                .clicked()
            {
                self.rpc("stop", json!({}));
            }
            if ui
                .add(egui::Button::new("Mark stutter"))
                .clicked()
            {
                self.rpc("mark", json!({"label": "stutter happened"}));
            }
        });
        ui.collapsing("Experiment guide", |ui| {
            theme::muted(ui, "Compare browser idle, video on the dGPU and video on the iGPU. Rotate conditions, warm up for 60 seconds and capture 120 seconds per run; repeat each condition three times.");
            if ui.button("Generate experiment schedule").clicked() { self.rpc("experiment", json!({})); }
        });
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            theme::section_title(ui, "Your recordings");
            ui.label(
                egui::RichText::new(format!("{} saved", recordings.len()))
                    .small()
                    .color(theme::MUTED),
            );
        });
        theme::muted(ui, "Select one run for notes and files, or two or more to compare. Inspect opens its timeline separately.");
        ui.add_space(4.0);
        self.notes.retain(recordings);
        if recordings.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::muted(
                    ui,
                    "No recordings yet. Capture a run when you want to measure a change.",
                );
            });
        }
        for recording in recordings {
            ui.push_id((recording.id, recording.started_ns), |ui| {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut selected = self.selected.contains(&recording.id);
                        if ui
                            .checkbox(&mut selected, "")
                            .on_hover_text("Select for notes or comparison")
                            .changed()
                        {
                            self.selected.retain(|&id| id != recording.id);
                            if selected {
                                self.selected.push(recording.id);
                            }
                        }
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 180.0).max(100.0));
                            ui.strong(&recording.label);
                            ui.label(
                                egui::RichText::new(format!(
                                    "#{} · {} · {} samples",
                                    recording.id, recording.state, recording.samples
                                ))
                                .small()
                                .color(
                                    if recording.state == "interrupted" {
                                        theme::WARNING
                                    } else if recording.state == "recording" {
                                        theme::MARKER
                                    } else {
                                        theme::MUTED
                                    },
                                ),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(theme::action("Inspect", theme::DGPU)).clicked() {
                                self.rpc("detail", json!({"id":recording.id}));
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Delete…").small().color(theme::MUTED),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.pending_delete = Some(recording.id);
                            }
                        });
                    });
                });
            });
            ui.add_space(8.0);
        }
        if let Some(id) = self.pending_delete {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        theme::DANGER,
                        format!("Permanently delete recording #{id}?"),
                    );
                    if ui.add(theme::action("Delete", theme::DANGER)).clicked() {
                        self.rpc("delete_recording", json!({"id": id}));
                        self.selected.retain(|&n| n != id);
                        self.pending_delete = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_delete = None;
                    }
                });
            });
        }
        if self.selected.len() >= 2
            && ui
                .add(theme::action("Compare selected recordings", theme::DGPU))
                .clicked()
        {
            self.rpc("compare", json!({"ids": self.selected}));
        }
        if self.selected.len() == 1 {
            let id = self.selected[0];
            ui.add_space(12.0);
            theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::section_title(ui, &format!("Notes & files · recording #{id}"));
            if let Some(recording) = recordings.iter().find(|r| r.id == id) {
                ui.label("Playback quality / experiment notes");
                ui.add(egui::TextEdit::multiline(self.notes.edit(recording)).desired_width(f32::INFINITY).desired_rows(3).hint_text("Playback quality, changed settings or anything the counters cannot measure…"));
                if ui.button("Save notes").clicked() {
                    if let Some(payload) = self.notes.payload(id) {
                        self.rpc("notes", payload);
                    }
                }
            }
            ui.add_space(8.0);
            ui.collapsing("Export bundle", |ui| {
                theme::muted(ui, "Export to a new absolute directory. Existing destinations are preserved.");
                ui.add(egui::TextEdit::singleline(&mut self.export_path).hint_text("/absolute/path/to/new-bundle").desired_width(f32::INFINITY));
            if ui.button("Export bundle").clicked() {
                self.rpc("export", json!({"id": id, "destination": self.export_path}));
            }
            });
            ui.collapsing("Import MangoHud log", |ui| {
                theme::muted(ui, "Import an existing frame log. External logs need an alignment anchor for a correlated timeline.");
                ui.add(egui::TextEdit::singleline(&mut self.import_path).hint_text("/absolute/path/to/frames.csv").desired_width(f32::INFINITY));
            theme::muted(ui, "Optional elapsed=0 monotonic anchor (ns)");
            ui.add(egui::TextEdit::singleline(&mut self.import_anchor).hint_text("Leave empty for an unaligned summary").desired_width(f32::INFINITY));
            if ui.button("Import frame log").clicked() {
                let anchor = if self.import_anchor.trim().is_empty() {
                    Ok(None)
                } else {
                    self.import_anchor.parse::<u64>().map(Some)
                };
                match anchor {
                    Ok(anchor) => {
                        if self
                            .recording_sender
                            .try_send(recording_worker::Job::Import {
                                id,
                                path: self.import_path.clone().into(),
                                anchor,
                            })
                            .is_err()
                        {
                            self.backend.lock().unwrap().error =
                                "Recording operations busy; retry after completion.".into();
                        }
                    }
                    Err(_) => {
                        self.backend.lock().unwrap().error =
                            "Alignment anchor must be an unsigned integer.".into()
                    }
                }
            }
                    });
            });
        }
        let (detail, comparison, samples, frames) = {
            let backend = self.backend.lock().unwrap();
            (
                backend.detail.clone(),
                backend.comparison.clone(),
                backend.samples.clone(),
                backend.frames.clone(),
            )
        };
        if let Some(detail) = detail {
            ui.add_space(16.0);
            let _inspected = theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.heading(format!(
                    "Recording #{} · {}",
                    detail["recording"]["id"],
                    detail["recording"]["label"].as_str().unwrap_or("")
                ));
                if let Some(logs) = detail["frame_logs"].as_array() {
                    if !logs.is_empty() {
                        ui.small(
                            "Frametime trace shows bucket maxima; statistics use every frame.",
                        );
                    }
                    for log in logs {
                        ui.label(format!(
                            "{} · {}",
                            log["filename"].as_str().unwrap_or("frame log"),
                            if log["aligned"].as_bool() == Some(true) {
                                "estimated alignment; see uncertainty"
                            } else {
                                "unaligned — summary only"
                            }
                        ));
                        summary_table(ui, &log["summary"], &format!("summary-{}", log["filename"]));
                    }
                }
                let markers: Vec<Marker> =
                    serde_json::from_value(detail["recording"]["markers"].clone())
                        .unwrap_or_default();
                timeline(ui, &samples, &frames, &markers);
            });
            #[cfg(feature = "screenshot")]
            if std::env::var("IGPU_MAN_SCREENSHOT_ADVANCED").as_deref() == Ok("statistics") {
                ui.scroll_to_rect(_inspected.response.rect, Some(egui::Align::TOP));
            }
        }
        if let Some(comparison) = comparison {
            ui.add_space(16.0);
            theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading("Comparison");
            if let Some(warnings) = comparison["mismatch_warnings"].as_array() {
                for warning in warnings {
                    ui.colored_label(
                        theme::WARNING,
                        warning.as_str().unwrap_or("Metadata mismatch"),
                    );
                }
            }
            if let Some(recordings) = comparison["recordings"].as_array() {
                for recording in recordings {
                    ui.strong(format!(
                        "#{} {}",
                        recording["recording"]["id"],
                        recording["recording"]["label"].as_str().unwrap_or("")
                    ));
                    if let Some(logs) = recording["frame_logs"].as_array() {
                        for (index, log) in logs.iter().enumerate() {
                            summary_table(
                                ui,
                                &log["summary"],
                                &format!("comparison-{}-{index}", recording["recording"]["id"]),
                            );
                        }
                    }
                }
            }
            ui.label("Run-to-run variation by condition (min–max)");
            if let Some(conditions) = comparison["run_variation_by_label"].as_object() {
                for (label, range) in conditions {
                    ui.label(format!(
                        "{label} · {} runs · average FPS {}–{} · p99 {}–{} ms",
                        range["runs"],
                        number(&range["average_fps_range"]["min"], 1),
                        number(&range["average_fps_range"]["max"], 1),
                        number(&range["p99_ms_range"]["min"], 2),
                        number(&range["p99_ms_range"]["max"], 2)
                    ));
                }
            }
            theme::muted(ui, "Observed differences are not a causal finding or a statistical significance claim. Check playback-quality notes and metadata.");
            });
        }
    }
}

fn number(value: &Value, precision: usize) -> String {
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .map(|value| format!("{value:.precision$}"))
        .unwrap_or_else(|| "Unavailable".into())
}
fn count(value: &Value) -> String {
    value
        .as_u64()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
fn summary_table(ui: &mut egui::Ui, summary: &Value, id: &str) {
    let metrics: [(String, String); 6] = [
        ("Frames".into(), count(&summary["frames"])),
        ("Average FPS".into(), number(&summary["average_fps"], 2)),
        ("Median · ms".into(), number(&summary["median_ms"], 2)),
        ("p95 · ms".into(), number(&summary["p95_ms"], 2)),
        ("p99 · ms".into(), number(&summary["p99_ms"], 2)),
        (
            "Over threshold".into(),
            count(&summary["frames_over_threshold"]),
        ),
    ];
    ui.push_id(id, |ui| {
        let columns = if ui.available_width() >= 800.0 { 6 } else { 3 };
        for row in metrics.chunks(columns) {
            ui.columns(columns, |columns| {
                for ((label, value), column) in row.iter().zip(columns) {
                    egui::Frame::new()
                        .fill(theme::BACKGROUND)
                        .corner_radius(8)
                        .inner_margin(10)
                        .show(column, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new(label).small().color(theme::MUTED));
                            ui.label(
                                egui::RichText::new(value)
                                    .size(if value == "Unavailable" { 13.0 } else { 22.0 })
                                    .color(theme::DGPU),
                            );
                        }).response.on_hover_text(if label == "Over threshold" {
                            let threshold = summary["threshold_ms"].as_f64().map(|n|n.to_string()).unwrap_or_else(||"unavailable".into());
                            format!("Frames above {threshold} ms. A threshold exceedance alone does not diagnose stutter.")
                        } else { label.clone() });
                }
            });
            ui.add_space(4.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_statistics_are_unavailable_while_measured_zero_is_preserved() {
        assert_eq!(number(&Value::Null, 2), "Unavailable");
        assert_eq!(number(&json!("invalid"), 2), "Unavailable");
        assert_eq!(number(&json!(0), 2), "0.00");
        assert_eq!(count(&Value::Null), "Unavailable");
        assert_eq!(count(&json!(0)), "0");
    }
}

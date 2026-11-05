//! Focused device telemetry; implementation details stay available on demand.
use super::*;

fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(egui::Stroke::new(1.0_f32, theme::tint(theme::BORDER, 150)))
        .corner_radius(12)
        .inner_margin(18)
}
fn reading(ui: &mut egui::Ui, gpu: &Gpu, key: &str, label: &str, size: f32) {
    let color = theme::gpu_color(gpu);
    ui.label(egui::RichText::new(label).small().color(theme::MUTED));
    let metric = gpu.metrics.get(key);
    let value = metric
        .map(metric_text)
        .unwrap_or_else(|| "Unavailable".into());
    let available = metric.is_some_and(|m| m.value.is_some());
    let response = ui.label(
        egui::RichText::new(value)
            .size(if available { size } else { 13.0 })
            .strong()
            .color(
                metric
                    .map(|m| theme::metric_color(m, color))
                    .unwrap_or(theme::MUTED),
            ),
    );
    if let Some(metric) = metric {
        response.on_hover_text(format!(
            "Source: {}\nStatus: {:?}",
            metric.source, metric.status
        ));
    }
}
impl Dashboard {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, stale: bool) {
        let inset = ((ui.available_width() - 1040.0) / 2.0).max(12.0);
        let width = (ui.available_width() - 2.0 * inset).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(inset);
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.add_space(12.0);
                ui.label(egui::RichText::new("GPU Activity").size(28.0).strong());
                ui.label(egui::RichText::new("A live view of both GPUs. Find individual apps in Applications.").color(theme::MUTED));
                if stale {
                    ui.colored_label(theme::WARNING, "Last known readings · collector disconnected or stale");
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Chart scale").small().color(theme::MUTED));
                    ui.selectable_value(&mut self.auto_scale_graphs, false, "Fixed 0–100%");
                    ui.selectable_value(&mut self.auto_scale_graphs, true, "Auto-scale")
                        .on_hover_text("Each GPU has its own zero-based axis, fitted to the visible history. Compare the percentage readings when axes differ.");
                });
                if self.auto_scale_graphs {
                    ui.label(egui::RichText::new("Independent axes · compare percentages, not line heights.").small().color(theme::MUTED));
                }
                ui.add_space(14.0);
                let count = if width >= 760.0 { snapshot.gpus.len().clamp(1, 2) } else { 1 };
                ui.columns(count, |columns| {
                    for (index, gpu) in snapshot.gpus.iter().enumerate() {
                        let ui = &mut columns[index % count];
                        ui.push_id(&gpu.pci, |ui| {
                            self.gpu_card(ui, gpu, snapshot.monotonic_ns);
                        });
                        ui.add_space(12.0);
                    }
                });
                if snapshot.gpus.is_empty() {
                    ui.colored_label(theme::WARNING, "No accessible AMD GPUs. Run igpu-man doctor for details.");
                }
                // Actionable failures stay visible; raw diagnostic output is secondary.
                if snapshot.coverage.permission_denied > 0 {
                    ui.colored_label(theme::WARNING, format!("Partial application coverage · {} permission failures. Device charts and observed app activity have different coverage.", snapshot.coverage.permission_denied));
                }
                for recommendation in &snapshot.recommendations {
                    ui.label(egui::RichText::new(recommendation).color(theme::WARNING));
                }
                ui.add_space(4.0);
                ui.collapsing("Collection & display details", |ui| {
                    ui.label(egui::RichText::new(format!("{} accessible user processes · {} unique DRM clients · {} permission failures · {:.2} ms per sample",
                        snapshot.coverage.processes_inspected, snapshot.coverage.clients_observed, snapshot.coverage.permission_denied, snapshot.collector_duration_ms)).small().color(theme::MUTED));
                    for note in &snapshot.diagnostics.notes {
                        ui.label(egui::RichText::new(note).small().color(theme::MUTED));
                    }
                    ui.label(egui::RichText::new("Monitor connection does not establish an app's renderer. Cross-GPU transfers are not directly measured.").small().color(theme::MUTED));
                    ui.collapsing("KWin diagnostics", |ui| {
                        ui.label(egui::RichText::new(snapshot.diagnostics.kwin_support.as_deref().unwrap_or("KWin diagnostics unavailable")).monospace().small().color(theme::MUTED));
                    });
                });
            });
        });
    }

    fn gpu_card(&self, ui: &mut egui::Ui, gpu: &Gpu, now: u64) {
        let color = theme::gpu_color(gpu);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(if gpu.integrated {
                    "INTEGRATED · iGPU"
                } else {
                    "DEDICATED · dGPU"
                })
                .size(11.0)
                .strong()
                .color(theme::MUTED),
            );
            let name = gpu.display_name.as_deref().unwrap_or(&gpu.name);
            ui.label(
                egui::RichText::new(name.strip_prefix("AMD ").unwrap_or(name))
                    .size(21.0)
                    .strong()
                    .color(color),
            );
            ui.add_space(12.0);
            ui.columns(3, |columns| {
                reading(&mut columns[0], gpu, "gpu_busy", "Usage", 32.0);
                reading(&mut columns[1], gpu, "power", "Power", 23.0);
                reading(&mut columns[2], gpu, "temperature", "Temperature", 23.0);
            });
            ui.add_space(10.0);
            let empty = VecDeque::new();
            chart(
                ui,
                self.history.get(&gpu.pci).unwrap_or(&empty),
                color,
                now,
                self.auto_scale_graphs,
            );
            ui.add_space(8.0);
            for output in gpu.outputs.iter().filter(|o| o.connected) {
                let mut text = output.connector.clone();
                if let Some(mode) = &output.current_mode {
                    let mode = if output.refresh_hz.is_some() {
                        mode.split('@').next().unwrap_or(mode)
                    } else {
                        mode
                    };
                    text.push_str(&format!(" · {mode}"));
                }
                if let Some(hz) = output.refresh_hz {
                    text.push_str(&format!(" · {hz:.0} Hz"));
                }
                ui.label(egui::RichText::new(text).small().color(theme::MUTED));
            }
            ui.collapsing("Device details", |ui| {
                for text in [
                    format!("PCI: {}", gpu.pci),
                    format!("DRM: {}", gpu.card),
                    format!("Reported device: {}", gpu.name),
                ] {
                    ui.label(egui::RichText::new(text).small().color(theme::MUTED));
                }
                if let Some(node) = &gpu.render_node {
                    ui.label(egui::RichText::new(node).small().color(theme::MUTED));
                }
                if let Some(source) = &gpu.display_name_source {
                    ui.label(
                        egui::RichText::new(format!("Display label: {source}"))
                            .small()
                            .color(theme::MUTED),
                    );
                }
                for (name, metric) in &gpu.metrics {
                    if !["gpu_busy", "power", "temperature"].contains(&name.as_str()) {
                        metric_row(ui, name, metric);
                    }
                }
                for output in gpu.outputs.iter().filter(|o| o.connected) {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · mode {} · refresh {} · scale {}",
                            output.connector,
                            output.current_mode.as_deref().unwrap_or("unavailable"),
                            output
                                .refresh_hz
                                .map(|v| format!("{v:.2} Hz"))
                                .unwrap_or_else(|| "unavailable".into()),
                            output
                                .scale
                                .map(|v| format!("{v:.2}"))
                                .unwrap_or_else(|| "unavailable".into())
                        ))
                        .small()
                        .color(theme::MUTED),
                    );
                }
            });
        });
    }
}

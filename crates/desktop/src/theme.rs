use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use igpu_core::model::{Availability, Gpu, Metric};

pub const BACKGROUND: Color32 = Color32::from_rgb(15, 21, 30);
pub const PANEL: Color32 = Color32::from_rgb(20, 28, 39);
pub const SURFACE: Color32 = Color32::from_rgb(26, 37, 51);
pub const BORDER: Color32 = Color32::from_rgb(53, 71, 94);
pub const TEXT: Color32 = Color32::from_rgb(231, 239, 250);
pub const MUTED: Color32 = Color32::from_rgb(159, 179, 202);
pub const IGPU: Color32 = Color32::from_rgb(255, 166, 96);
pub const DGPU: Color32 = Color32::from_rgb(133, 181, 255);
pub const WARNING: Color32 = Color32::from_rgb(250, 204, 117);
pub const DANGER: Color32 = Color32::from_rgb(255, 139, 157);
pub const MARKER: Color32 = Color32::from_rgb(203, 170, 255);

pub fn tint(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.faint_bg_color = SURFACE;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.code_bg_color = BACKGROUND;
    visuals.hyperlink_color = DGPU;
    visuals.warn_fg_color = WARNING;
    visuals.error_fg_color = DANGER;
    visuals.selection.bg_fill = Color32::from_rgb(43, 76, 115);
    visuals.selection.stroke = Stroke::new(1.0_f32, DGPU);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 59, 81);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(39, 59, 81);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, DGPU);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, TEXT);
    visuals.widgets.active.bg_fill = Color32::from_rgb(45, 80, 115);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, TEXT);
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(11.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(14);
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(21.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    ctx.set_style(style);
}

pub fn gpu_color(gpu: &Gpu) -> Color32 {
    if gpu.integrated {
        IGPU
    } else {
        DGPU
    }
}

pub fn action(label: &str, color: Color32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).strong().color(color))
        .fill(tint(color, 28))
        .stroke(Stroke::new(1.0_f32, tint(color, 150)))
}

pub fn badge(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(tint(color, 22))
        .stroke(Stroke::new(1.0_f32, tint(color, 80)))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(label).small().strong().color(color));
        });
}

pub fn metric_color(metric: &Metric, accent: Color32) -> Color32 {
    match metric.status {
        Availability::PermissionDenied | Availability::Stale => WARNING,
        Availability::Unavailable | Availability::WarmingUp => MUTED,
        Availability::Idle | Availability::Available => accent,
    }
}

pub fn plot_background(painter: &egui::Painter, outer: egui::Rect, plot: egui::Rect, ceiling: f64) {
    painter.rect_filled(outer, 7.0, BACKGROUND);
    painter.rect_stroke(
        outer,
        7.0,
        Stroke::new(1.0_f32, BORDER),
        egui::StrokeKind::Inside,
    );
    for step in 0..=4 {
        let y = egui::lerp(plot.bottom()..=plot.top(), step as f32 / 4.0);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0_f32, if step == 0 { BORDER } else { tint(BORDER, 125) }),
        );
        painter.text(
            egui::pos2(plot.left() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            super::graphs::tick_label(ceiling * step as f64 / 4.0),
            FontId::monospace(10.0),
            MUTED,
        );
        let x = egui::lerp(plot.left()..=plot.right(), step as f32 / 4.0);
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            Stroke::new(1.0_f32, tint(BORDER, 80)),
        );
    }
}

pub fn trace(
    painter: &egui::Painter,
    points: &[egui::Pos2],
    color: Color32,
    baseline: Option<f32>,
) {
    if points.is_empty() {
        return;
    }
    if let Some(bottom) = baseline {
        let mut mesh = egui::Mesh::default();
        for pair in points.windows(2) {
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(pair[0], tint(color, 65));
            mesh.colored_vertex(pair[1], tint(color, 65));
            mesh.colored_vertex(egui::pos2(pair[1].x, bottom), tint(color, 4));
            mesh.colored_vertex(egui::pos2(pair[0].x, bottom), tint(color, 4));
            mesh.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    if points.len() > 1 {
        painter.add(egui::Shape::line(
            points.to_vec(),
            Stroke::new(3.0_f32, tint(color, 12)),
        ));
        painter.add(egui::Shape::line(
            points.to_vec(),
            Stroke::new(1.35_f32, color),
        ));
    }
}

/// Shared framing for secondary tools; layout only, with no background work.
pub fn page(ui: &mut egui::Ui, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    let inset = ((ui.available_width() - width) / 2.0).max(12.0);
    let width = (ui.available_width() - inset * 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(inset);
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.visuals_mut().override_text_color = Some(MUTED);
            ui.add_space(12.0);
            content(ui);
            ui.add_space(12.0);
        });
    });
}
/// Compact main-page layout with the same gutter as Applications.
pub fn full_width_page(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let width = (ui.available_width() - 32.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.visuals_mut().override_text_color = Some(MUTED);
            ui.add_space(6.0);
            content(ui);
            ui.add_space(12.0);
        });
    });
}
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, tint(BORDER, 150)))
        .corner_radius(12)
        .inner_margin(18)
}
pub fn page_title(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(RichText::new("ADVANCED").size(11.0).strong().color(MUTED));
    ui.label(RichText::new(title).size(28.0).strong().color(TEXT));
    muted(ui, description);
    ui.add_space(14.0);
}
pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}
pub fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(18.0).strong().color(TEXT));
}
pub fn gpu_label(gpus: &[Gpu], target: &str) -> String {
    gpus.iter()
        .find(|g| g.pci == target || g.alias == target)
        .map(|g| {
            format!(
                "{} · {}",
                if g.integrated { "iGPU" } else { "dGPU" },
                g.display_name.as_deref().unwrap_or(&g.name)
            )
        })
        .unwrap_or_else(|| target.to_owned())
}
pub fn open_section(ctx: &egui::Context, id: egui::Id) {
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
    state.set_open(true);
    state.store(ctx);
    ctx.request_repaint();
}

/// Explicit identity lets a menu open the same editor regardless of nesting.
pub fn collapsible<R>(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    default_open: bool,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<egui::InnerResponse<R>> {
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open,
    );
    let mut toggle = false;
    let mut header = state.show_header(ui, |ui| {
        toggle = ui.selectable_label(false, title).clicked();
    });
    if toggle {
        header.toggle();
    }
    header.body(content).2
}

#[cfg(test)]
mod section_tests {
    use super::*;
    #[test]
    fn opening_an_editor_from_a_nested_menu_uses_its_parent_identity() {
        let ctx = egui::Context::default();
        for step in 0..3 {
            let mut displayed = false;
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(step as f64),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let editor = ui.make_persistent_id("fixture-editor");
                        if step == 1 {
                            ui.push_id("nested-menu", |ui| open_section(ui.ctx(), editor));
                        }
                        displayed = collapsible(ui, editor, "Editor", false, |ui| {
                            ui.label("Draft");
                        })
                        .is_some();
                        assert_eq!(
                            egui::collapsing_header::CollapsingState::load(ctx, editor)
                                .unwrap()
                                .is_open(),
                            step > 0
                        );
                    });
                },
            );
            if step == 2 {
                assert!(displayed);
            }
        }
    }
}

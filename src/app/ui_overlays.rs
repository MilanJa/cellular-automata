//! Small floating toolbars drawn over the viewport: the brush (always) and the seed-image
//! settings (while an image is loaded). They live on the grid they act on, not in the sidebar.

use super::App;
use super::theme;
use crate::sim::seed_image::SeedMode;

const MARGIN: f32 = 12.0;

fn overlay_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, theme::EDITOR_STROKE))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 6))
}

/// The brush toolbar in the viewport's bottom-left corner.
pub fn brush(app: &mut App, ui: &egui::Ui, viewport: egui::Rect) {
    egui::Area::new(egui::Id::new("brush-overlay"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(viewport.left_bottom() + egui::vec2(MARGIN, -MARGIN))
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            overlay_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("✎ brush").weak()).on_hover_text("Drag to paint, right button erases");
                    ui.add(egui::Slider::new(&mut app.state.brush_radius, 0.5..=32.0).logarithmic(true))
                        .on_hover_text("Radius in cells");
                    ui.separator();
                    for v in app.state.brush_value.iter_mut() {
                        ui.add(egui::DragValue::new(v).speed(0.01));
                    }
                    if ui.small_button("on()").on_hover_text("Paint (1, 0, 0, 1)").clicked() {
                        app.state.brush_value = [1.0, 0.0, 0.0, 1.0];
                    }
                });
            });
        });
}

/// Seed-image settings in the viewport's top-left corner, only while an image is loaded.
pub fn seed_image(app: &mut App, ui: &egui::Ui, viewport: egui::Rect) {
    if app.seed_image.is_none() {
        return;
    }
    let mut reapply = false;
    let mut forget = false;
    egui::Area::new(egui::Id::new("seed-image-overlay"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::LEFT_TOP)
        .fixed_pos(viewport.left_top() + egui::vec2(MARGIN, MARGIN))
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            overlay_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🖼 seed image").weak());
                    let is_lum = matches!(app.state.seed_mode, SeedMode::Luminance { .. });
                    if ui.selectable_label(is_lum, "brightness → on/off").clicked() && !is_lum {
                        app.state.seed_mode = SeedMode::Luminance { threshold: 0.5 };
                        reapply = true;
                    }
                    if ui.selectable_label(!is_lum, "RGBA → channels").clicked() && is_lum {
                        app.state.seed_mode = SeedMode::Channels;
                        reapply = true;
                    }
                    if let SeedMode::Luminance { threshold } = &mut app.state.seed_mode {
                        ui.label("threshold");
                        if ui.add(egui::Slider::new(threshold, 0.0..=1.0)).changed() {
                            reapply = true;
                        }
                    }
                    if ui.button("Re-apply").on_hover_text("Write the image into the grid again").clicked() {
                        reapply = true;
                    }
                    if ui.small_button("✕").on_hover_text("Forget this image").clicked() {
                        forget = true;
                    }
                });
            });
        });
    if reapply {
        app.apply_seed_image();
    }
    if forget {
        app.seed_image = None;
    }
}

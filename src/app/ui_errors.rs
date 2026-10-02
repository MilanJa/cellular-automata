//! The bottom panel: the latest notice (saves, imports, failures), then either the shader
//! error list or the "compiled" status with statistics.

use super::theme;
use super::{App, NoticeLevel};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    if let Some(notice) = &app.notice {
        let (color, icon) = match notice.level {
            NoticeLevel::Info => (theme::OK, "ℹ"),
            NoticeLevel::Error => (theme::ERROR, "⚠"),
        };
        let text = format!("{icon} {}", notice.text);
        let mut dismiss = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(text).color(color));
            if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                dismiss = true;
            }
        });
        if dismiss {
            app.notice = None;
        }
    }
    if app.state.errors.is_empty() {
        let (msg, color) = if crate::util::lock(&app.sim).has_pipelines() {
            ("✔ shaders compiled", theme::OK)
        } else {
            ("no pipeline yet", theme::WARN)
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(msg).color(color));
            super::ui_topbar::stats_readout(app, ui);
        });
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for e in app.state.errors.clone() {
            let rich = egui::RichText::new(super::state::error_text(&e)).color(theme::ERROR).monospace();
            let label = egui::Label::new(rich).sense(egui::Sense::click());
            if ui.add(label).on_hover_text("Click to jump to this line").clicked() {
                app.pending_cursor = Some((e.file, e.line));
            }
            if let Some(h) = &e.hint {
                ui.label(egui::RichText::new(format!("   ↳ {h}")).weak());
            }
        }
    });
}

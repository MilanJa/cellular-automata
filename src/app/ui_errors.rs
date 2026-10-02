use super::theme;
use super::App;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.state.errors.is_empty() {
        let (msg, color) = if app.sim.lock().unwrap().has_pipelines() {
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
        for d in app.state.errors.clone() {
            let rich = egui::RichText::new(d.text()).color(theme::ERROR).monospace();
            match d.location() {
                Some(loc) => {
                    let label = egui::Label::new(rich).sense(egui::Sense::click());
                    if ui.add(label).on_hover_text("Click to jump to this line").clicked() {
                        app.pending_cursor = Some(loc);
                    }
                }
                None => {
                    ui.label(rich);
                }
            }
            if let super::state::Diagnostic::Shader(e) = &d
                && let Some(h) = &e.hint
            {
                ui.label(egui::RichText::new(format!("   ↳ {h}")).weak());
            }
        }
    });
}

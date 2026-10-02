use super::App;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.state.errors.is_empty() {
        let msg = if app.sim.lock().unwrap().has_pipelines() {
            "✔ shaders compiled"
        } else {
            "no pipeline yet"
        };
        ui.label(egui::RichText::new(msg).weak());
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for d in app.state.errors.clone() {
            let rich = egui::RichText::new(d.text()).color(egui::Color32::from_rgb(230, 110, 110)).monospace();
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

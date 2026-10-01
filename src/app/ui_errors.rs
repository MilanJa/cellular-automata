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
        for e in app.state.errors.clone() {
            let text = format!("{}:{}:{}  {}", e.file.label(), e.line, e.column, e.message);
            let label = egui::Label::new(
                egui::RichText::new(text).color(egui::Color32::from_rgb(230, 110, 110)).monospace(),
            )
            .sense(egui::Sense::click());
            if ui.add(label).on_hover_text("Click to jump to this line").clicked() {
                app.pending_cursor = Some((e.file, e.line));
            }
        }
    });
}

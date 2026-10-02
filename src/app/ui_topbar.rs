use super::state::PresetSource;
use super::App;
use crate::preset::builtin::{load_builtin, BUILTINS};
use crate::preset::Preset;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let label = if app.state.modified {
            format!("{} *", app.state.preset_name)
        } else {
            app.state.preset_name.clone()
        };
        let mut to_load: Option<(Preset, PresetSource)> = None;
        let mut load_error: Option<String> = None;
        egui::ComboBox::from_id_salt("preset").selected_text(label).width(260.0).show_ui(ui, |ui| {
            ui.label(egui::RichText::new("Built-in").small().weak());
            for (i, b) in BUILTINS.iter().enumerate() {
                let p = load_builtin(b);
                let selected = app.state.source == PresetSource::Builtin(i);
                if ui.selectable_label(selected, &p.meta.name).clicked() {
                    to_load = Some((p, PresetSource::Builtin(i)));
                }
            }
            if !app.state.disk_presets.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("./presets").small().weak());
                for (name, path) in app.state.disk_presets.clone() {
                    let selected = app.state.source == PresetSource::Disk(path.clone());
                    if ui.selectable_label(selected, &name).clicked() {
                        match Preset::load_dir(&path) {
                            Ok(p) => to_load = Some((p, PresetSource::Disk(path))),
                            Err(e) => load_error = Some(format!("load failed: {e:#}")),
                        }
                    }
                }
            }
        });
        if let Some((p, src)) = to_load {
            app.load_preset(p, src);
        }
        if let Some(msg) = load_error {
            app.report(msg);
        }
        if ui.button("Save").on_hover_text("Save to this preset's folder (built-ins: Save As)").clicked() {
            app.save();
        }
        if ui.button("Save as…").clicked() {
            app.save_as();
        }
        if ui.button("Rescan").on_hover_text("Rescan ./presets").clicked() {
            app.rescan_presets();
        }

        ui.separator();
        let play_label = if app.state.playing { "⏸ Pause" } else { "▶ Play" };
        if ui.button(play_label).on_hover_text("Space").clicked() {
            app.state.playing = !app.state.playing;
        }
        if ui.add_enabled(!app.state.playing, egui::Button::new("⏭ Step")).clicked() {
            app.state.step_once = true;
        }
        ui.label("steps/frame");
        if ui.add(egui::DragValue::new(&mut app.state.steps_per_frame).range(1..=256)).changed() {
            app.state.modified = true;
        }
        if ui.button("↺ Reset").on_hover_text("Apply grid settings and re-initialise").clicked() {
            app.apply_settings_and_reset();
        }

        ui.separator();
        let frame = app.sim.lock().unwrap().frame();
        let steps_per_sec = app.rate.update(std::time::Instant::now(), frame);
        let dt = ui.input(|i| i.stable_dt).max(1e-6);
        ui.label(format!("step {frame}   {steps_per_sec:.0} steps/s   {:.0} fps", 1.0 / dt));
    });
}

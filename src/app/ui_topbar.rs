use super::state::PresetSource;
use super::App;
use crate::platform;
use crate::preset::builtin::{load_builtin, BUILTINS, TEMPLATES};
use crate::preset::Preset;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let label = if app.state.modified {
            format!("{} *", app.state.preset_name)
        } else {
            app.state.preset_name.clone()
        };
        let mut to_load: Option<(Preset, PresetSource)> = None;
        let mut to_load_saved: Option<platform::SavedLocation> = None;
        egui::ComboBox::from_id_salt("preset").selected_text(label).width(260.0).show_ui(ui, |ui| {
            ui.label(egui::RichText::new("Built-in").small().weak());
            for (i, b) in BUILTINS.iter().enumerate() {
                let p = load_builtin(b);
                let selected = app.state.source == PresetSource::Builtin(i);
                if ui.selectable_label(selected, &p.meta.name).clicked() {
                    to_load = Some((p, PresetSource::Builtin(i)));
                }
            }
            if !app.state.saved_presets.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(platform::SAVED_SECTION_LABEL).small().weak());
                for (name, location) in app.state.saved_presets.clone() {
                    let selected = app.state.source == PresetSource::Saved(location.clone());
                    if ui.selectable_label(selected, &name).clicked() {
                        to_load_saved = Some(location);
                    }
                }
            }
        });
        if let Some((p, src)) = to_load {
            app.load_preset(p, src);
        }
        if let Some(location) = to_load_saved {
            app.load_saved(location);
        }
        let mut new_template: Option<usize> = None;
        ui.menu_button("+ New", |ui| {
            ui.label(egui::RichText::new("Start from a commented template").small().weak());
            for (i, t) in TEMPLATES.iter().enumerate() {
                let name = load_builtin(t).meta.name;
                if ui.button(name).clicked() {
                    new_template = Some(i);
                    ui.close();
                }
            }
        });
        if let Some(i) = new_template {
            app.load_template(i);
        }
        let save_hint = if platform::is_web() {
            "Save to this browser (built-ins and templates: Save As)"
        } else {
            "Save to this preset's folder (built-ins and templates: Save As)"
        };
        if ui.button("Save").on_hover_text(save_hint).clicked() {
            app.save();
        }
        if ui.button("Save as…").clicked() {
            app.save_as();
        }
        if ui.button("Export").on_hover_text("Download / write a single-file preset bundle").clicked() {
            app.export();
        }
        if ui.button("Import").on_hover_text("Load a preset bundle file").clicked() {
            app.import();
        }
        if !platform::is_web() && ui.button("Rescan").on_hover_text("Rescan ./presets").clicked() {
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
        let steps_per_sec = app.rate.update(web_time::Instant::now(), frame);
        let dt = ui.input(|i| i.stable_dt).max(1e-6);
        ui.label(format!("step {frame}   {steps_per_sec:.0} steps/s   {:.0} fps", 1.0 / dt));
    });
}

/// The browser's Save prompt (a small window). Does nothing when no dialog is open.
pub fn save_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = app.save_dialog.clone() else { return };
    let mut name = dialog.name;
    let mut action: Option<bool> = None; // Some(true) = save, Some(false) = cancel
    egui::Window::new("Save preset")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Name");
            let resp = ui.text_edit_singleline(&mut name);
            if dialog.confirm_overwrite {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 170, 90),
                    "A preset with this name already exists. Save again to overwrite it.",
                );
            }
            ui.horizontal(|ui| {
                let label = if dialog.confirm_overwrite { "Overwrite" } else { "Save" };
                if ui.button(label).clicked() || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    action = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(false);
                }
            });
        });
    if let Some(d) = &mut app.save_dialog
        && d.name != name
    {
        d.name = name;
        d.confirm_overwrite = false;
    }
    match action {
        Some(true) => {
            #[cfg(target_arch = "wasm32")]
            app.save_dialog_confirmed();
            #[cfg(not(target_arch = "wasm32"))]
            {
                app.save_dialog = None;
            }
        }
        Some(false) => app.save_dialog = None,
        None => {}
    }
}

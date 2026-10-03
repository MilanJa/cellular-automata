use super::App;
use super::theme;
use crate::app::modulation::{Modulation, Wave};
use crate::shader::params::{ParamSpec, ParamType, ParamValue};

fn slider_f32(ui: &mut egui::Ui, v: &mut f32, range: Option<(f64, f64)>) -> bool {
    match range {
        Some((lo, hi)) => ui.add(egui::Slider::new(v, lo as f32..=hi as f32)).changed(),
        None => ui.add(egui::DragValue::new(v).speed(0.01)).changed(),
    }
}

fn sliders(ui: &mut egui::Ui, a: &mut [f32], range: Option<(f64, f64)>) -> bool {
    let mut changed = false;
    for x in a.iter_mut() {
        changed |= slider_f32(ui, x, range);
    }
    changed
}

fn param_widget(ui: &mut egui::Ui, spec: &ParamSpec, value: &mut ParamValue) -> bool {
    match value {
        ParamValue::F32(v) => slider_f32(ui, v, spec.range),
        ParamValue::I32(v) => match spec.range {
            Some((lo, hi)) => ui.add(egui::Slider::new(v, lo as i32..=hi as i32)).changed(),
            None => ui.add(egui::DragValue::new(v)).changed(),
        },
        ParamValue::Bool(b) => ui.checkbox(b, "").changed(),
        ParamValue::Vec2(a) => sliders(ui, a, spec.range),
        ParamValue::Vec3(a) => {
            if spec.color {
                ui.color_edit_button_rgb(a).changed()
            } else {
                sliders(ui, a, spec.range)
            }
        }
        ParamValue::Vec4(a) => {
            if spec.color {
                ui.color_edit_button_rgba_unmultiplied(a).changed()
            } else {
                sliders(ui, a, spec.range)
            }
        }
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    theme::section(ui, "Params");
    ui.horizontal(|ui| {
        if ui.button("Mutate").on_hover_text("Nudge every value a little").clicked() {
            app.mutate();
        }
        if ui
            .add_enabled(!app.state.undo.is_empty(), egui::Button::new("Undo"))
            .on_hover_text("Restore the values from before the last Mutate")
            .clicked()
        {
            app.undo_mutate();
        }
    });
    if app.state.specs.is_empty() {
        ui.label(
            egui::RichText::new("Declare params in a shader with `// @param name: f32 = 0.5 range 0 .. 1`").weak(),
        );
    }
    let mut changed = false;
    // Computed once per frame by `push_params`; `None` while nothing is modulated.
    let live = app.live_values.clone();
    egui::Grid::new("params").num_columns(2).striped(true).show(ui, |ui| {
        for spec in app.state.specs.clone() {
            ui.label(&spec.name);
            ui.horizontal(|ui| {
                if let Some(v) = app.state.values.get_mut(&spec.name) {
                    changed |= param_widget(ui, &spec, v);
                }
                if matches!(spec.ty, ParamType::F32 | ParamType::I32) {
                    changed |= modulation_button(app, ui, &spec, live.as_ref());
                }
            });
            ui.end_row();
        }
    });
    if changed {
        app.state.modified = true;
        app.push_params();
    }

    theme::section(ui, "Layer B");
    ui.label(egui::RichText::new("A second automaton running alongside; shaders read it with other(x, y).").weak());
    ui.horizontal(|ui| {
        let current = app.layer_b_name().unwrap_or_else(|| "none".to_string());
        let mut pick: Option<Option<crate::preset::Preset>> = None;
        egui::ComboBox::from_id_salt("layer-b").selected_text(current).width(220.0).show_ui(ui, |ui| {
            if ui.selectable_label(app.layer_b.is_none(), "none").clicked() {
                pick = Some(None);
            }
            for b in crate::preset::builtin::BUILTINS.iter() {
                if ui.selectable_label(false, b.name()).clicked() {
                    pick = Some(Some(crate::preset::builtin::load_builtin(b)));
                }
            }
            for (name, location) in app.state.saved_presets.clone() {
                if ui.selectable_label(false, &name).clicked() {
                    match crate::platform::load_saved(&location) {
                        Ok(p) => pick = Some(Some(p)),
                        Err(e) => app.report(format!("load failed: {e:#}")),
                    }
                }
            }
        });
        if let Some(choice) = pick {
            app.set_layer_b(choice);
            app.state.modified = true; // layer B is saved with the preset
        }
    });
}

/// MIDI learn / binding controls at the top of a param's "~" menu. Returns true if a binding was
/// removed (the param keeps its current value, but the preset changed).
fn midi_row(app: &mut App, ui: &mut egui::Ui, spec: &ParamSpec) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("MIDI").small().weak());
        let learning = app.state.midi_map.learning() == Some(spec.name.as_str());
        if learning {
            ui.label(egui::RichText::new("move a knob...").color(theme::WARN));
            if ui.small_button("Cancel").clicked() {
                app.state.midi_map.cancel_learn();
            }
            return;
        }
        if let Some(key) = app.state.midi_map.bindings.get(&spec.name).copied() {
            ui.label(key.label());
            if ui.small_button("Unbind").clicked() {
                app.state.midi_map.bindings.remove(&spec.name);
                changed = true;
            }
        }
        if ui.small_button("Learn").on_hover_text("Bind the next knob you move to this value").clicked() {
            if app.midi.is_none() {
                app.toggle_midi();
            }
            if app.midi.is_some() {
                app.state.midi_map.learn(&spec.name);
            }
        }
        if app.midi.is_none() && !app.state.midi_map.bindings.contains_key(&spec.name) {
            ui.label(egui::RichText::new("(Learn also enables MIDI)").weak().small());
        }
    });
    changed
}

/// The "~" button next to a numeric param: opens the LFO popover; lit while a modulation is active.
fn modulation_button(
    app: &mut App,
    ui: &mut egui::Ui,
    spec: &ParamSpec,
    live: Option<&std::collections::BTreeMap<String, ParamValue>>,
) -> bool {
    let active = app.state.modulations.contains_key(&spec.name);
    let bound = app.state.midi_map.bindings.contains_key(&spec.name);
    let audio_on = app.audio.is_some();
    let mut changed = false;
    let label = if active {
        egui::RichText::new("~").strong().color(theme::ACCENT)
    } else if bound {
        egui::RichText::new("~").strong().color(theme::WARN)
    } else {
        egui::RichText::new("~").weak()
    };
    // Keep the menu open while its buttons are clicked (egui closes menus on any button click
    // by default); only a click outside dismisses it.
    let config =
        egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new(label).config(config).ui(ui, |ui| {
        ui.set_min_width(300.0);
        changed |= midi_row(app, ui, spec);
        ui.separator();
        match app.state.modulations.get_mut(&spec.name) {
            None => {
                ui.label(egui::RichText::new("Modulate this value over time").small().weak());
                if ui.button("Enable").clicked() {
                    app.state.modulations.insert(spec.name.clone(), Modulation::default());
                    changed = true;
                }
            }
            Some(m) => {
                egui::Grid::new(("lfo", &spec.name)).num_columns(2).show(ui, |ui| {
                    ui.label("wave");
                    // Inline buttons rather than a ComboBox: a nested popup would close this menu.
                    ui.horizontal_wrapped(|ui| {
                        for w in Wave::ALL {
                            changed |= ui.selectable_value(&mut m.wave, w, w.label()).changed();
                        }
                    });
                    ui.end_row();
                    ui.label("audio");
                    ui.horizontal_wrapped(|ui| {
                        for w in Wave::AUDIO {
                            changed |= ui.selectable_value(&mut m.wave, w, w.label()).changed();
                        }
                        if !audio_on {
                            ui.label(egui::RichText::new("(enable the microphone in the top bar)").weak().small());
                        }
                    });
                    ui.end_row();
                    if !m.wave.is_audio() {
                        ui.label("freq (Hz)");
                        changed |= ui.add(egui::Slider::new(&mut m.freq, 0.01..=5.0).logarithmic(true)).changed();
                        ui.end_row();
                    }
                    ui.label("amount");
                    changed |= ui.add(egui::Slider::new(&mut m.amount, 0.0..=1.0)).changed();
                    ui.end_row();
                    if !m.wave.is_audio() {
                        ui.label("phase");
                        changed |= ui.add(egui::Slider::new(&mut m.phase, 0.0..=1.0)).changed();
                        ui.end_row();
                        ui.label("clock");
                        changed |= ui.checkbox(&mut m.follow_sim, "follow simulation (pauses with it)").changed();
                        ui.end_row();
                    }
                });
                if ui.button("Remove").clicked() {
                    app.state.modulations.remove(&spec.name);
                    changed = true;
                    ui.close();
                }
            }
        }
    });
    if active && let Some(v) = live.and_then(|l| l.get(&spec.name)) {
        let text = match v {
            ParamValue::F32(x) => format!("{x:.3}"),
            ParamValue::I32(x) => x.to_string(),
            _ => String::new(),
        };
        ui.label(egui::RichText::new(text).weak().monospace());
    }
    changed
}

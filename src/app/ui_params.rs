use super::theme;
use super::App;
use crate::app::modulation::{Modulation, Wave};
use crate::preset::{InitPattern, Mode};
use crate::shader::params::{ParamSpec, ParamType, ParamValue};
use crate::sim::seed_image::SeedMode;

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
            if spec.color { ui.color_edit_button_rgb(a).changed() } else { sliders(ui, a, spec.range) }
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
            egui::RichText::new("Declare params in a shader with `// @param name: f32 = 0.5 range 0 .. 1`")
                .weak(),
        );
    }
    let mut changed = false;
    let live = if app.state.modulations.is_empty() { None } else { Some(app.effective_values()) };
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

    theme::section(ui, "Grid");
    let before = app.state.pending.clone();
    let p = &mut app.state.pending;
    egui::Grid::new("grid-settings").num_columns(2).show(ui, |ui| {
        ui.label("mode");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut p.mode, Mode::TwoD, "2D");
            ui.selectable_value(&mut p.mode, Mode::OneD, "1D (space-time)");
        });
        ui.end_row();
        ui.label("size");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut p.width).range(1..=4096).speed(4));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut p.height).range(1..=4096).speed(4));
        });
        ui.end_row();
        ui.label("init");
        ui.horizontal(|ui| {
            let is_random = matches!(p.init, InitPattern::Random { .. });
            if ui.selectable_label(is_random, "random").clicked() && !is_random {
                p.init = InitPattern::Random { density: 0.3 };
            }
            if ui.selectable_label(p.init == InitPattern::Single, "single").clicked() {
                p.init = InitPattern::Single;
            }
            if ui.selectable_label(p.init == InitPattern::Blank, "blank").clicked() {
                p.init = InitPattern::Blank;
            }
        });
        ui.end_row();
        if let InitPattern::Random { density } = &mut p.init {
            ui.label("density");
            ui.add(egui::Slider::new(density, 0.0..=1.0));
            ui.end_row();
        }
        ui.label("seed");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut p.seed));
            if ui.button("🎲").on_hover_text("New random seed").clicked() {
                p.seed = p.seed.wrapping_mul(1664525).wrapping_add(1013904223);
            }
        });
        ui.end_row();
    });
    if app.state.pending != before {
        app.state.modified = true;
    }
    ui.horizontal(|ui| {
        if ui.button("Apply grid settings & reset").clicked() {
            app.apply_settings_and_reset();
        }
        ui.checkbox(&mut app.state.auto_reseed, "auto-reseed when stuck")
            .on_hover_text("Reset with a new seed after the grid has been static or periodic for 2 s");
    });

    theme::section(ui, "Audio");
    ui.horizontal(|ui| {
        let label = if app.audio.is_some() { "Disable microphone" } else { "Enable microphone" };
        if ui.button(label).on_hover_text("Audio levels become modulation sources in each slider's ~ menu").clicked() {
            app.toggle_audio();
        }
        if let Some(input) = &app.audio {
            ui.label(egui::RichText::new(&input.device_name).weak());
        }
    });
    if app.audio.is_some() {
        let l = app.audio_levels;
        egui::Grid::new("audio-meters").num_columns(2).show(ui, |ui| {
            for (name, v) in [("level", l.level), ("low", l.low), ("mid", l.mid), ("high", l.high)] {
                ui.label(name);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 10.0), egui::Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(rect, 2.0, theme::METER_BG);
                let w = rect.width() * v.clamp(0.0, 1.0);
                painter.rect_filled(
                    egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height())),
                    2.0,
                    theme::ACCENT,
                );
                ui.end_row();
            }
        });
    }

    theme::section(ui, "MIDI");
    ui.horizontal(|ui| {
        let label = if app.midi.is_some() { "Disable MIDI" } else { "Enable MIDI" };
        if ui.button(label).on_hover_text("Bind controller knobs to values with Learn in a slider's ~ menu").clicked() {
            app.toggle_midi();
        }
        if let Some(rx) = &app.midi {
            let ports = if rx.port_names.is_empty() { "waiting for permission...".to_string() } else { rx.port_names.join(", ") };
            ui.label(egui::RichText::new(ports).weak());
        }
    });
    if app.midi.is_some() {
        ui.horizontal(|ui| {
            ui.label("last");
            match app.last_cc {
                Some(key) => ui.label(egui::RichText::new(key.label()).monospace()),
                None => ui.label(egui::RichText::new("nothing yet").weak()),
            };
            if let Some(name) = app.state.midi_map.learning() {
                ui.label(egui::RichText::new(format!("learning {name}: move a knob")).color(theme::WARN));
            }
        });
        let bound: Vec<(String, String)> = app
            .state
            .midi_map
            .bindings
            .iter()
            .filter(|(n, _)| app.state.specs.iter().any(|s| &s.name == *n))
            .map(|(n, k)| (n.clone(), k.label()))
            .collect();
        if !bound.is_empty() {
            egui::Grid::new("midi-bindings").num_columns(2).show(ui, |ui| {
                for (name, key) in bound {
                    ui.label(name);
                    ui.label(egui::RichText::new(key).monospace());
                    ui.end_row();
                }
            });
        }
    }

    theme::section(ui, "Layer B");
    ui.label(
        egui::RichText::new("A second automaton running alongside; shaders read it with other(x, y).")
            .weak(),
    );
    ui.horizontal(|ui| {
        let current = app.layer_b_name().unwrap_or_else(|| "none".to_string());
        let mut pick: Option<Option<crate::preset::Preset>> = None;
        egui::ComboBox::from_id_salt("layer-b").selected_text(current).width(220.0).show_ui(ui, |ui| {
            if ui.selectable_label(app.layer_b.is_none(), "none").clicked() {
                pick = Some(None);
            }
            for b in crate::preset::builtin::BUILTINS {
                let p = crate::preset::builtin::load_builtin(b);
                if ui.selectable_label(false, &p.meta.name).clicked() {
                    pick = Some(Some(p));
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
        }
    });

    theme::section(ui, "Seed image");
    ui.label(egui::RichText::new("Image > Seed grid from image…, or drop a PNG on the window.").weak());
    let mut reapply = false;
    ui.horizontal(|ui| {
        let is_lum = matches!(app.state.seed_mode, SeedMode::Luminance { .. });
        if ui.selectable_label(is_lum, "brightness -> on/off").clicked() && !is_lum {
            app.state.seed_mode = SeedMode::Luminance { threshold: 0.5 };
            reapply = true;
        }
        if ui.selectable_label(!is_lum, "RGBA -> channels").clicked() && is_lum {
            app.state.seed_mode = SeedMode::Channels;
            reapply = true;
        }
    });
    if let SeedMode::Luminance { threshold } = &mut app.state.seed_mode {
        ui.horizontal(|ui| {
            ui.label("threshold");
            if ui.add(egui::Slider::new(threshold, 0.0..=1.0)).changed() {
                reapply = true;
            }
        });
    }
    if ui
        .add_enabled(app.seed_image.is_some(), egui::Button::new("Re-apply image"))
        .on_hover_text("Write the last image into the grid again with the settings above")
        .clicked()
    {
        reapply = true;
    }
    if reapply && app.seed_image.is_some() {
        app.apply_seed_image();
    }

    theme::section(ui, "Brush");
    ui.label(egui::RichText::new("Drag on the grid to paint; right button erases.").weak());
    egui::Grid::new("brush").num_columns(2).show(ui, |ui| {
        ui.label("radius");
        ui.add(egui::Slider::new(&mut app.state.brush_radius, 0.5..=32.0).logarithmic(true));
        ui.end_row();
        ui.label("value");
        ui.horizontal(|ui| {
            for v in app.state.brush_value.iter_mut() {
                ui.add(egui::DragValue::new(v).speed(0.01));
            }
            if ui.small_button("on()").on_hover_text("(1, 0, 0, 1)").clicked() {
                app.state.brush_value = [1.0, 0.0, 0.0, 1.0];
            }
        });
        ui.end_row();
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
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
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
                            ui.label(egui::RichText::new("(enable the microphone below)").weak().small());
                        }
                    });
                    ui.end_row();
                    if !m.wave.is_audio() {
                        ui.label("freq (Hz)");
                        changed |= ui
                            .add(egui::Slider::new(&mut m.freq, 0.01..=5.0).logarithmic(true))
                            .changed();
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
                        changed |= ui
                            .checkbox(&mut m.follow_sim, "follow simulation (pauses with it)")
                            .changed();
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

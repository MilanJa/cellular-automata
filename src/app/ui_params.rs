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
    ui.horizontal(|ui| {
        ui.heading("Params");
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

    ui.separator();
    ui.heading("Grid");
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

    ui.separator();
    ui.heading("Seed image");
    ui.label(egui::RichText::new("Image → Seed grid from image…, or drop a PNG on the window.").weak());
    let mut reapply = false;
    ui.horizontal(|ui| {
        let is_lum = matches!(app.state.seed_mode, SeedMode::Luminance { .. });
        if ui.selectable_label(is_lum, "brightness → on/off").clicked() && !is_lum {
            app.state.seed_mode = SeedMode::Luminance { threshold: 0.5 };
            reapply = true;
        }
        if ui.selectable_label(!is_lum, "RGBA → channels").clicked() && is_lum {
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

    ui.separator();
    ui.heading("Brush");
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

/// The "~" button next to a numeric param: opens the LFO popover; lit while a modulation is active.
fn modulation_button(
    app: &mut App,
    ui: &mut egui::Ui,
    spec: &ParamSpec,
    live: Option<&std::collections::BTreeMap<String, ParamValue>>,
) -> bool {
    let active = app.state.modulations.contains_key(&spec.name);
    let mut changed = false;
    let label = if active {
        egui::RichText::new("~").strong().color(egui::Color32::from_rgb(120, 200, 255))
    } else {
        egui::RichText::new("~").weak()
    };
    ui.menu_button(label, |ui| {
        ui.set_min_width(220.0);
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
                    egui::ComboBox::from_id_salt(("lfo-wave", &spec.name))
                        .selected_text(m.wave.label())
                        .show_ui(ui, |ui| {
                            for w in Wave::ALL {
                                changed |= ui.selectable_value(&mut m.wave, w, w.label()).changed();
                            }
                        });
                    ui.end_row();
                    ui.label("freq (Hz)");
                    changed |= ui
                        .add(egui::Slider::new(&mut m.freq, 0.01..=5.0).logarithmic(true))
                        .changed();
                    ui.end_row();
                    ui.label("amount");
                    changed |= ui.add(egui::Slider::new(&mut m.amount, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("phase");
                    changed |= ui.add(egui::Slider::new(&mut m.phase, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("clock");
                    changed |= ui
                        .checkbox(&mut m.follow_sim, "follow simulation (pauses with it)")
                        .changed();
                    ui.end_row();
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

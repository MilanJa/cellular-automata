use super::App;
use crate::preset::{InitPattern, Mode};
use crate::shader::params::{ParamSpec, ParamValue};

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
    ui.heading("Params");
    if app.state.specs.is_empty() {
        ui.label(
            egui::RichText::new("Declare params in a shader with `// @param name: f32 = 0.5 range 0 .. 1`")
                .weak(),
        );
    }
    let mut changed = false;
    egui::Grid::new("params").num_columns(2).striped(true).show(ui, |ui| {
        for spec in app.state.specs.clone() {
            ui.label(&spec.name);
            if let Some(v) = app.state.values.get_mut(&spec.name) {
                ui.horizontal(|ui| {
                    changed |= param_widget(ui, &spec, v);
                });
            }
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
    if ui.button("Apply grid settings & reset").clicked() {
        app.apply_settings_and_reset();
    }
}

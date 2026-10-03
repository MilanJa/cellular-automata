//! Top-bar popovers for things that are not about the shader code: the grid settings behind
//! the transport, and the microphone and MIDI toggles with their status.

use super::App;
use super::theme;
use crate::preset::{InitPattern, Mode};

/// A menu that stays open while its sliders and buttons are used; only a click outside closes it.
fn popover(ui: &mut egui::Ui, label: egui::RichText, hover: &str, content: impl FnOnce(&mut egui::Ui)) {
    let config =
        egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new(label)
        .config(config)
        .ui(ui, |ui| {
            ui.set_min_width(300.0);
            content(ui);
        })
        .0
        .on_hover_text(hover);
}

fn on_off(name: &str, on: bool, color: egui::Color32) -> egui::RichText {
    let text = format!("{} {name}", if on { "●" } else { "○" });
    if on { egui::RichText::new(text).color(color).strong() } else { egui::RichText::new(text).weak() }
}

/// Grid settings (mode, size, init, seed), applied with Reset.
pub fn grid(app: &mut App, ui: &mut egui::Ui) {
    let dirty = app.state.pending != app.state.applied;
    let label = if dirty { egui::RichText::new("Grid *").color(theme::WARN) } else { egui::RichText::new("Grid") };
    popover(ui, label, "Grid size, mode and initial pattern (take effect on Reset)", |ui| {
        theme::section_label(ui, "Grid");
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
        ui.separator();
        ui.horizontal(|ui| {
            let apply = if dirty {
                egui::Button::new(egui::RichText::new("Apply & reset").strong()).fill(theme::ACCENT_FILL)
            } else {
                egui::Button::new("Apply & reset")
            };
            if ui.add(apply).clicked() {
                app.apply_settings_and_reset();
            }
            ui.checkbox(&mut app.state.auto_reseed, "auto-reseed when stuck")
                .on_hover_text("Reset with a new seed after the grid has been static or periodic for 2 s");
        });
    });
}

/// Microphone toggle with live level meters.
pub fn audio(app: &mut App, ui: &mut egui::Ui) {
    let on = app.audio.is_some();
    popover(ui, on_off("mic", on, theme::ACCENT), "Microphone: audio levels become modulation sources", |ui| {
        theme::section_label(ui, "Audio");
        ui.horizontal(|ui| {
            let label = if on { "Disable microphone" } else { "Enable microphone" };
            if ui.button(label).clicked() {
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
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 10.0), egui::Sense::hover());
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
        ui.label(egui::RichText::new("Pick level / low / mid / high in a slider's ~ menu.").weak().small());
    });
}

/// MIDI toggle with the last knob moved, learn status and the current bindings.
pub fn midi(app: &mut App, ui: &mut egui::Ui) {
    let on = app.midi.is_some();
    let learning = app.state.midi_map.learning().map(str::to_string);
    let label = match &learning {
        Some(name) => egui::RichText::new(format!("● MIDI: learning {name}")).color(theme::WARN).strong(),
        None => on_off("MIDI", on, theme::WARN),
    };
    popover(ui, label, "MIDI controllers: bind knobs to values with Learn in a slider's ~ menu", |ui| {
        theme::section_label(ui, "MIDI");
        ui.horizontal(|ui| {
            let label = if on { "Disable MIDI" } else { "Enable MIDI" };
            if ui.button(label).clicked() {
                app.toggle_midi();
            }
            if let Some(rx) = &app.midi {
                let ports = if rx.port_names.is_empty() {
                    "waiting for permission...".to_string()
                } else {
                    rx.port_names.join(", ")
                };
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
            });
            if let Some(name) = &learning {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("learning {name}: move a knob")).color(theme::WARN));
                    if ui.small_button("Cancel").clicked() {
                        app.state.midi_map.cancel_learn();
                    }
                });
            }
        }
        let bound: Vec<(String, String)> = app
            .state
            .midi_map
            .bindings
            .iter()
            .filter(|(n, _)| app.state.specs.iter().any(|s| &s.name == *n))
            .map(|(n, k)| (n.clone(), k.label()))
            .collect();
        if !bound.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("Bindings").small().weak());
            egui::Grid::new("midi-bindings").num_columns(2).show(ui, |ui| {
                for (name, key) in bound {
                    ui.label(name);
                    ui.label(egui::RichText::new(key).monospace());
                    ui.end_row();
                }
            });
        }
        ui.label(egui::RichText::new("Press Learn in a slider's ~ menu, then move a knob.").weak().small());
    });
}

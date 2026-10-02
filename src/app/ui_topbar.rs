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
        if ui.button("Share").on_hover_text("Copy a link that opens this exact scene in the web version").clicked() {
            let ctx = ui.ctx().clone();
            app.share(&ctx);
        }
        if !platform::is_web() && ui.button("Rescan").on_hover_text("Rescan ./presets").clicked() {
            app.rescan_presets();
        }
        let mut export_scale: Option<u32> = None;
        let mut seed_requested = false;
        let mut record_requested = false;
        ui.menu_button("Image", |ui| {
            ui.label(egui::RichText::new("Save the grid as a PNG, pixels per cell:").small().weak());
            for s in [1u32, 2, 4] {
                if ui.button(format!("{s}×")).clicked() {
                    export_scale = Some(s);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Seed grid from image…").on_hover_text("Or drop a PNG onto the window").clicked() {
                seed_requested = true;
                ui.close();
            }
            if ui.button("Record animation…").on_hover_text("Capture frames into a looping animated PNG").clicked() {
                record_requested = true;
                ui.close();
            }
        });
        if record_requested {
            app.open_record_dialog();
        }
        if let Some(s) = export_scale {
            app.export_image(s);
        }
        if seed_requested {
            app.request_seed_image();
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
        if let Some((done, total)) = app.recording_progress() {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), format!("● recording {done}/{total}"));
            if ui.small_button("Stop").clicked() {
                app.stop_recording();
            }
        } else if app.export_pending() {
            ui.label(egui::RichText::new("exporting image…").weak());
        }
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

/// Population, change rate, a sparkline of the population history and the stuck badge.
pub fn stats_readout(app: &App, ui: &mut egui::Ui) {
    let Some(last) = app.state.stats.back() else { return };
    let cells = {
        let c = app.state.pending.clone();
        (c.width as f32 * c.height as f32).max(1.0)
    };
    ui.separator();
    ui.label(format!(
        "pop {} ({:.1}%)   \u{394} {:.2}%",
        last.population,
        100.0 * last.population as f32 / cells,
        100.0 * last.changed as f32 / cells
    ));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 16.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, egui::Color32::from_gray(30));
    let max = app.state.stats.iter().map(|s| s.population).max().unwrap_or(1).max(1) as f32;
    let n = app.state.stats.len();
    if n >= 2 {
        let pts: Vec<egui::Pos2> = app
            .state
            .stats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let x = rect.left() + rect.width() * i as f32 / (n - 1) as f32;
                let y = rect.bottom() - (rect.height() - 2.0) * s.population as f32 / max - 1.0;
                egui::pos2(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 200, 255))));
    }
    if let Some(stuck) = app.state.stuck {
        let text = match stuck {
            crate::sim::stats::Stuck::Static => "stuck: static".to_string(),
            crate::sim::stats::Stuck::Periodic(p) => format!("stuck: period {p}"),
        };
        ui.colored_label(egui::Color32::from_rgb(230, 170, 90), text);
    }
}

/// Rewind strip under the viewport: snapshot slider, step label and snapshot interval.
pub fn timeline(app: &mut App, ui: &mut egui::Ui) {
    let (len, cap, interval, selected_meta) = {
        let sim = app.sim.lock().unwrap();
        let len = sim.history_len();
        let idx = app.scrub.unwrap_or(len.saturating_sub(1)).min(len.saturating_sub(1));
        (len, sim.history_capacity(), sim.snapshot_interval(), sim.history_meta(idx))
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("⏪").weak());
        if len < 2 {
            ui.label(egui::RichText::new("history fills as the simulation runs").weak());
        } else {
            // While playing the slider follows the newest snapshot; dragging it pauses and rewinds.
            let newest = len - 1;
            let mut pos = if app.state.playing { newest } else { app.scrub.unwrap_or(newest).min(newest) };
            let slider = egui::Slider::new(&mut pos, 0..=newest).show_value(false);
            let resp = ui.add_sized([ui.available_width() - 330.0, 18.0], slider);
            if resp.changed() {
                app.scrub_to(pos);
            }
            if let Some(m) = selected_meta {
                ui.label(format!("step {}", m.step));
            }
            ui.label(egui::RichText::new(format!("{len}/{cap} snapshots")).weak());
        }
        ui.label(egui::RichText::new("every").weak());
        let mut iv = interval;
        egui::ComboBox::from_id_salt("snapshot-interval")
            .selected_text(format!("{iv} steps"))
            .width(90.0)
            .show_ui(ui, |ui| {
                for v in [1u32, 2, 5, 10, 30, 100] {
                    ui.selectable_value(&mut iv, v, format!("{v} steps"));
                }
            });
        if iv != interval {
            app.sim.lock().unwrap().set_snapshot_interval(iv);
        }
    });
    if app.state.playing {
        app.scrub = None;
    }
}

/// The Record dialog: frame count, scale and playback rate.
pub fn record_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut settings) = app.record_dialog else { return };
    let (w, h) = {
        let sim = app.sim.lock().unwrap();
        let c = sim.config();
        (c.width, c.height)
    };
    let mut action: Option<bool> = None;
    egui::Window::new("Record animation")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(
                    "Frames are captured as fast as the GPU returns them while the simulation plays, \
                     then written as a looping animated PNG.",
                )
                .weak(),
            );
            egui::Grid::new("record-settings").num_columns(2).show(ui, |ui| {
                ui.label("pixels per cell");
                ui.horizontal(|ui| {
                    for s in [1u32, 2, 4] {
                        ui.selectable_value(&mut settings.scale, s, format!("{s}×"));
                    }
                });
                ui.end_row();
                let cap = crate::sim::record::max_frames(w * settings.scale, h * settings.scale, crate::sim::record::FRAME_BUDGET_BYTES).min(3600);
                settings.frames = settings.frames.clamp(1, cap);
                ui.label("frames");
                ui.add(egui::Slider::new(&mut settings.frames, 1..=cap).logarithmic(true));
                ui.end_row();
                ui.label("playback fps");
                ui.add(egui::Slider::new(&mut settings.fps, 1..=60));
                ui.end_row();
                ui.label("image size");
                ui.label(format!("{} × {} px, {:.0} MB buffered", w * settings.scale, h * settings.scale,
                    (w * settings.scale) as f64 * (h * settings.scale) as f64 * 4.0 * settings.frames as f64 / 1e6));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Start").clicked() {
                    action = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(false);
                }
            });
        });
    match action {
        Some(true) => app.start_recording(settings),
        Some(false) => app.record_dialog = None,
        None => app.record_dialog = Some(settings),
    }
}

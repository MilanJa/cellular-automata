use super::App;
use super::state::PresetSource;
use super::theme;
use crate::platform;
use crate::preset::Preset;
use crate::preset::builtin::{BUILTINS, TEMPLATES, load_builtin};
use crate::util::lock;

pub const SAVE_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);

/// Menu bar: File / Image / View menus, the preset picker, transport controls and the status
/// readout pinned to the right.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::containers::menu::MenuBar::new().ui(ui, |ui| {
        file_menu(app, ui);
        image_menu(app, ui);
        view_menu(app, ui);
        ui.separator();
        preset_picker(app, ui);
        ui.separator();
        transport(app, ui);
        status(app, ui);
    });
}

fn file_menu(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.menu_button("File", |ui| {
        ui.menu_button("New from template", |ui| {
            ui.label(egui::RichText::new("Commented starting points").small().weak());
            for (i, t) in TEMPLATES.iter().enumerate() {
                if ui.button(t.name()).clicked() {
                    app.load_template(i);
                    ui.close();
                }
            }
        });
        ui.separator();
        let save_hint = if platform::is_web() {
            "Save to this browser (built-ins and templates: Save As)"
        } else {
            "Save to this preset's folder (built-ins and templates: Save As)"
        };
        let save = egui::Button::new("Save").shortcut_text(ctx.format_shortcut(&SAVE_SHORTCUT));
        if ui.add(save).on_hover_text(save_hint).clicked() {
            app.save();
        }
        if ui.button("Save as…").clicked() {
            app.save_as();
        }
        let is_saved = matches!(app.state.source, PresetSource::Saved(_));
        if ui
            .add_enabled(is_saved, egui::Button::new("Delete saved preset…"))
            .on_hover_text("Remove this preset from where it is saved; the scene stays open")
            .clicked()
        {
            app.delete_saved_preset();
        }
        ui.separator();
        if ui.button("Export bundle…").on_hover_text("Download / write a single-file preset bundle").clicked() {
            app.export();
        }
        if ui.button("Import bundle…").on_hover_text("Load a preset bundle file").clicked() {
            app.import();
        }
        if ui.button("Copy share link").on_hover_text("A link that opens this exact scene in the web version").clicked()
        {
            app.share(&ctx);
        }
        if !platform::is_web() {
            ui.separator();
            if ui.button("Rescan presets").on_hover_text("Rescan ./presets").clicked() {
                app.rescan_presets();
            }
        }
    });
}

fn image_menu(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button("Image", |ui| {
        ui.label(egui::RichText::new("Save the grid as a PNG").small().weak());
        for (s, label) in
            [(1u32, "Export PNG, 1 px per cell"), (2, "Export PNG, 2 px per cell"), (4, "Export PNG, 4 px per cell")]
        {
            if ui.button(label).clicked() {
                app.export_image(s);
            }
        }
        ui.separator();
        if ui.button("Seed grid from image…").on_hover_text("Or drop a PNG onto the window").clicked() {
            app.request_seed_image();
        }
        if ui.button("Record animation…").on_hover_text("Capture frames into a looping animated PNG").clicked() {
            app.open_record_dialog();
        }
    });
}

fn view_menu(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button("View", |ui| {
        if ui
            .selectable_label(app.explorer.is_some(), "Rule explorer")
            .on_hover_text("A grid of random Life-like rules running live; click one to load it")
            .clicked()
        {
            app.toggle_explorer();
        }
    });
}

fn preset_picker(app: &mut App, ui: &mut egui::Ui) {
    let label = if app.state.modified { format!("{} *", app.state.preset_name) } else { app.state.preset_name.clone() };
    let mut to_load: Option<(Preset, PresetSource)> = None;
    let mut to_load_saved: Option<platform::SavedLocation> = None;
    egui::ComboBox::from_id_salt("preset").selected_text(label).width(240.0).show_ui(ui, |ui| {
        ui.label(egui::RichText::new("Built-in").small().weak());
        for (i, b) in BUILTINS.iter().enumerate() {
            let selected = app.state.source == PresetSource::Builtin(i);
            if ui.selectable_label(selected, b.name()).clicked() {
                to_load = Some((load_builtin(b), PresetSource::Builtin(i)));
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
}

fn transport(app: &mut App, ui: &mut egui::Ui) {
    let play = if app.state.playing {
        egui::Button::new("⏸ Pause")
    } else {
        egui::Button::new(egui::RichText::new("▶ Play").color(theme::ACCENT).strong())
    };
    if ui.add(play).on_hover_text("Space").clicked() {
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
}

fn status(app: &mut App, ui: &mut egui::Ui) {
    let frame = lock(&app.sim).frame();
    let steps_per_sec = app.rate.update(web_time::Instant::now(), frame);
    let dt = ui.input(|i| i.stable_dt).max(1e-6);
    // Status lives at the right edge, so the controls keep a stable position.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(
            egui::RichText::new(format!("step {frame}   {steps_per_sec:.0} steps/s   {:.0} fps", 1.0 / dt)).weak(),
        );
        if let Some((done, total)) = app.recording_progress() {
            if ui.small_button("Stop").clicked() {
                app.stop_recording();
            }
            ui.colored_label(theme::ERROR, format!("● recording {done}/{total}"));
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
                ui.colored_label(theme::WARN, "A preset with this name already exists. Save again to overwrite it.");
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
        let c = &app.state.applied;
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
    painter.rect_filled(rect, 2.0, theme::METER_BG);
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
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, theme::ACCENT)));
    }
    if let Some(stuck) = app.state.stuck {
        let text = match stuck {
            crate::sim::stats::Stuck::Static => "stuck: static".to_string(),
            crate::sim::stats::Stuck::Periodic(p) => format!("stuck: period {p}"),
        };
        ui.colored_label(theme::WARN, text);
    }
}

/// Rewind strip under the viewport: snapshot slider, step label and snapshot interval.
pub fn timeline(app: &mut App, ui: &mut egui::Ui) {
    let (len, cap, interval, selected_meta) = {
        let sim = lock(&app.sim);
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
            // Leave room for the labels on the right; never collapse below a usable width.
            let resp = ui.add_sized([(ui.available_width() - 330.0).max(80.0), 18.0], slider);
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
        egui::ComboBox::from_id_salt("snapshot-interval").selected_text(format!("{iv} steps")).width(90.0).show_ui(
            ui,
            |ui| {
                for v in [1u32, 2, 5, 10, 30, 100] {
                    ui.selectable_value(&mut iv, v, format!("{v} steps"));
                }
            },
        );
        if iv != interval {
            lock(&app.sim).set_snapshot_interval(iv);
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
        let sim = lock(&app.sim);
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
                let cap = crate::sim::record::max_frames(
                    w * settings.scale,
                    h * settings.scale,
                    crate::sim::record::FRAME_BUDGET_BYTES,
                )
                .min(3600);
                settings.frames = settings.frames.clamp(1, cap);
                ui.label("frames");
                ui.add(egui::Slider::new(&mut settings.frames, 1..=cap).logarithmic(true));
                ui.end_row();
                ui.label("playback fps");
                ui.add(egui::Slider::new(&mut settings.fps, 1..=60));
                ui.end_row();
                ui.label("image size");
                ui.label(format!(
                    "{} × {} px, {:.0} MB buffered",
                    w * settings.scale,
                    h * settings.scale,
                    (w * settings.scale) as f64 * (h * settings.scale) as f64 * 4.0 * settings.frames as f64 / 1e6
                ));
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

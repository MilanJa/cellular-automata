//! The egui application: wires the editors, params, transport and presets to the simulation.

pub mod modulation;
pub mod state;
mod ui_editor;
mod ui_errors;
mod ui_params;
mod ui_topbar;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::platform::{self, SavedLocation};
use crate::preset::builtin::{load_builtin, BUILTINS, TEMPLATES};
use crate::preset::Preset;
use crate::shader::params::{pack_params, ParamValue};
use crate::shader::validate::ShaderFile;
use crate::app::modulation::modulated_values;
use crate::sim::paint::{pointer_to_cell, Stroke};
use crate::sim::Simulation;
use crate::viewport::show_viewport;
use state::{
    build_shaders, diagnostics_from, prepare_preset_load, preset_to_state, resolve_values,
    state_to_preset, AppState, Diagnostic, PresetSource, RateMeter,
};

const DEFAULT_BUILTIN: usize = 2; // Game of Life

/// The in-app "save to browser" prompt (the web has no folder picker).
#[derive(Debug, Clone, Default)]
pub(crate) struct SaveDialog {
    pub name: String,
    /// Set once the user has seen the "already exists" notice; the next Save overwrites.
    pub confirm_overwrite: bool,
}

pub struct App {
    sim: Arc<Mutex<Simulation>>,
    state: AppState,
    /// Set when an error entry is clicked; the matching editor moves its cursor there.
    pending_cursor: Option<(ShaderFile, usize)>,
    /// Steps-per-second meter for the status line.
    pub(crate) rate: RateMeter,
    pub(crate) save_dialog: Option<SaveDialog>,
    /// Strokes gathered this frame; handed to the viewport callback.
    strokes: Vec<Stroke>,
    /// Viewport rect from the previous frame, for mapping pointer positions.
    last_viewport: Option<egui::Rect>,
    /// Wall clock for modulations that do not follow simulation time.
    launched: web_time::Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_preset(cc, None)
    }

    /// `start` is a built-in id (e.g. `rule30`) or, on the desktop, a preset folder;
    /// `None` loads Game of Life.
    pub fn with_preset(cc: &eframe::CreationContext<'_>, start: Option<&str>) -> Self {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("wgpu render state (the app requires the wgpu backend)");
        let mut start_error = None;
        let default = || (load_builtin(&BUILTINS[DEFAULT_BUILTIN]), PresetSource::Builtin(DEFAULT_BUILTIN));
        let (preset, source) = match start {
            None => default(),
            Some(id) => match BUILTINS.iter().position(|b| b.id == id) {
                Some(i) => (load_builtin(&BUILTINS[i]), PresetSource::Builtin(i)),
                None => {
                    let location = SavedLocation::Folder(std::path::PathBuf::from(id));
                    match platform::load_saved(&location) {
                        Ok(p) => (p, PresetSource::Saved(location)),
                        Err(e) => {
                            start_error = Some(format!("could not load preset `{id}`: {e:#}"));
                            default()
                        }
                    }
                }
            },
        };
        let (editor, config, spf, toml_params) = preset_to_state(&preset);
        let sim = Simulation::new(rs.device.clone(), rs.queue.clone(), rs.target_format, config.clone());
        let state = AppState::from_parts(
            editor,
            config,
            spf,
            Vec::new(),
            BTreeMap::new(),
            preset.meta.name.clone(),
            source,
        );
        let mut app = App {
            sim: Arc::new(Mutex::new(sim)),
            state,
            pending_cursor: None,
            rate: RateMeter::new(web_time::Instant::now(), 0),
            save_dialog: None,
            strokes: Vec::new(),
            last_viewport: None,
            launched: web_time::Instant::now(),
        };
        app.state.saved_presets = platform::list_saved();
        app.state.modulations = preset.meta.modulation.clone();
        app.apply_shaders_with_toml(&toml_params);
        if let Some(msg) = start_error {
            app.report(msg);
        }
        app
    }

    /// Switches to `preset` only if its shaders compile; otherwise reports the errors and leaves
    /// the current editors, grid and params exactly as they were.
    pub(crate) fn load_preset(&mut self, preset: Preset, source: PresetSource) {
        let loaded = match prepare_preset_load(&preset) {
            Ok(l) => l,
            Err(mut errors) => {
                for e in &mut errors {
                    e.message = format!("[{}] {}", preset.meta.name, e.message);
                }
                self.state.errors = diagnostics_from(errors);
                return;
            }
        };
        let mut sim = self.sim.lock().unwrap();
        if let Err(errors) = sim.set_pipelines(&loaded.rule, &loaded.render) {
            drop(sim);
            self.state.errors = diagnostics_from(errors);
            return;
        }
        sim.reconfigure(loaded.config.clone());
        let config = sim.config().clone();
        drop(sim);
        self.state.editor = loaded.editor;
        self.state.pending = config;
        self.state.steps_per_frame = loaded.steps_per_frame;
        self.state.preset_name = loaded.name;
        self.state.source = source;
        self.state.values = resolve_values(&loaded.specs, &loaded.toml_params, &BTreeMap::new());
        self.state.specs = loaded.specs;
        self.state.modulations = loaded.modulations;
        self.state.errors.clear();
        self.state.modified = false;
        self.state.started = web_time::Instant::now();
        self.push_params();
    }

    /// Starts a new, unsaved preset from one of the embedded templates.
    pub(crate) fn load_template(&mut self, index: usize) {
        let Some(t) = TEMPLATES.get(index) else { return };
        let mut preset = load_builtin(t);
        preset.meta.name = format!("Untitled ({})", preset.meta.name);
        self.load_preset(preset, PresetSource::Template(index));
        self.state.modified = true;
    }

    pub(crate) fn load_saved(&mut self, location: SavedLocation) {
        match platform::load_saved(&location) {
            Ok(p) => self.load_preset(p, PresetSource::Saved(location)),
            Err(e) => self.report(format!("load failed: {e:#}")),
        }
    }

    pub(crate) fn apply_shaders(&mut self) {
        self.apply_shaders_with_toml(&BTreeMap::new());
    }

    fn apply_shaders_with_toml(&mut self, toml_params: &BTreeMap<String, toml::Value>) {
        match build_shaders(&self.state.editor) {
            Err(errors) => self.state.errors = diagnostics_from(errors),
            Ok((specs, rule, render)) => {
                let result = self.sim.lock().unwrap().set_pipelines(&rule, &render);
                match result {
                    Err(errors) => self.state.errors = diagnostics_from(errors),
                    Ok(()) => {
                        self.state.values = resolve_values(&specs, toml_params, &self.state.values);
                        self.state.specs = specs;
                        self.state.editor.clear_dirty();
                        self.state.errors.clear();
                        self.push_params();
                    }
                }
            }
        }
    }

    /// Uploads the current param values with any active modulations applied.
    pub(crate) fn push_params(&self) {
        let values = self.effective_values();
        let packed = pack_params(&self.state.specs, &values);
        self.sim.lock().unwrap().set_params(packed);
    }

    /// Slider values with the LFOs applied for this instant.
    pub(crate) fn effective_values(&self) -> BTreeMap<String, ParamValue> {
        if self.state.modulations.is_empty() {
            return self.state.values.clone();
        }
        let wall = self.launched.elapsed().as_secs_f32();
        let sim = self.state.started.elapsed().as_secs_f32();
        modulated_values(&self.state.specs, &self.state.values, &self.state.modulations, wall, sim)
    }

    /// True when some modulated param is one the shaders actually declare.
    fn modulation_active(&self) -> bool {
        self.state.modulations.keys().any(|k| self.state.specs.iter().any(|s| &s.name == k))
    }

    pub(crate) fn apply_settings_and_reset(&mut self) {
        let mut sim = self.sim.lock().unwrap();
        sim.reconfigure(self.state.pending.clone());
        self.state.pending = sim.config().clone(); // reflect clamping
        drop(sim);
        self.state.started = web_time::Instant::now();
    }

    /// Save in place when the preset already has a home; otherwise behave like Save As.
    pub(crate) fn save(&mut self) {
        match self.state.source.clone() {
            PresetSource::Saved(location) => self.save_to(&location),
            _ => self.save_as(),
        }
    }

    /// Desktop: folder picker, slug subfolder, native confirm. Browser: in-app name prompt.
    pub(crate) fn save_as(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some((location, exists)) = platform::choose_save_location(&self.state.preset_name) else {
                return;
            };
            if exists
                && !platform::confirm(
                    "Overwrite preset?",
                    &format!("{} already contains a preset. Overwrite it?", location.describe()),
                )
            {
                return;
            }
            self.save_to(&location);
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.save_dialog = Some(SaveDialog { name: self.state.preset_name.clone(), confirm_overwrite: false });
        }
    }

    /// Browser Save dialog confirmed: store under the chosen name (after an overwrite notice if needed).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn save_dialog_confirmed(&mut self) {
        let Some(dialog) = self.save_dialog.clone() else { return };
        let name = dialog.name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let (location, exists) = platform::location_for_name(&name);
        if exists && !dialog.confirm_overwrite {
            if let Some(d) = &mut self.save_dialog {
                d.confirm_overwrite = true;
            }
            return;
        }
        self.state.preset_name = name;
        self.save_to(&location);
        self.save_dialog = None;
    }

    fn save_to(&mut self, location: &SavedLocation) {
        let preset = state_to_preset(&self.state);
        match platform::save_to(&preset, location) {
            Ok(()) => {
                self.state.source = PresetSource::Saved(location.clone());
                self.state.modified = false;
                self.state.saved_presets = platform::list_saved();
            }
            Err(e) => self.report(format!("save failed: {e:#}")),
        }
    }

    /// Starts a PNG export of the current state at `scale` pixels per cell.
    pub(crate) fn export_image(&mut self, scale: u32) {
        let mut sim = self.sim.lock().unwrap();
        let filename = crate::sim::export::image_filename(&self.state.preset_name, sim.frame());
        let result = sim.start_export(scale, filename);
        drop(sim);
        if let Err(e) = result {
            self.report(e);
        }
    }

    fn poll_export_image(&mut self) {
        let result = self.sim.lock().unwrap().poll_export();
        match result {
            None => {}
            Some(Err(e)) => self.report(e),
            Some(Ok(img)) => {
                if let Err(e) = platform::save_png(&img.filename, &img.png) {
                    self.report(format!("could not save image: {e:#}"));
                }
            }
        }
    }

    pub(crate) fn export_pending(&self) -> bool {
        self.sim.lock().unwrap().export_pending()
    }

    pub(crate) fn export(&mut self) {
        let preset = state_to_preset(&self.state);
        if let Err(e) = platform::export_bundle(&preset) {
            self.report(format!("export failed: {e:#}"));
        }
    }

    pub(crate) fn import(&mut self) {
        platform::request_import();
    }

    fn poll_import(&mut self) {
        if let Some(result) = platform::poll_import() {
            match result {
                Ok(p) => {
                    self.load_preset(p, PresetSource::Imported);
                    self.state.modified = true;
                }
                Err(e) => self.report(format!("import failed: {e:#}")),
            }
        }
    }

    pub(crate) fn rescan_presets(&mut self) {
        self.state.saved_presets = platform::list_saved();
    }

    /// Turns a drag or click on the viewport into brush strokes for the next frame.
    fn collect_strokes(&mut self, response: &egui::Response, rect: egui::Rect) {
        let down = response.dragged() || response.is_pointer_button_down_on();
        if !down {
            return;
        }
        let Some(pos) = response.interact_pointer_pos() else { return };
        let (primary, secondary) = response
            .ctx
            .input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        let value = if secondary && !primary {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            self.state.brush_value
        };
        let (w, h) = {
            let sim = self.sim.lock().unwrap();
            let c = sim.config();
            (c.width, c.height)
        };
        let r = (rect.min.x, rect.min.y, rect.width(), rect.height());
        if let Some((x, y)) = pointer_to_cell(pos.x, pos.y, r, w, h) {
            self.strokes.push(Stroke { x, y, radius: self.state.brush_radius, value });
            response.ctx.request_repaint();
        }
    }

    /// Shows a non-shader message in the error panel.
    pub(crate) fn report(&mut self, message: String) {
        self.state.errors = vec![Diagnostic::General(message)];
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let lost = self.sim.lock().unwrap().take_device_lost();
        if let Some(message) = lost {
            self.state.playing = false;
            self.report(format!("{message}. Restart the application to continue."));
        }
        self.poll_import();
        self.poll_export_image();
        if self.modulation_active() {
            self.push_params();
        }
        ui_topbar::save_dialog(self, &ctx);
        // Global shortcut: Ctrl/Cmd+Enter applies shaders. Consume it before the editors see it.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            self.apply_shaders();
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        if !typing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            self.state.playing = !self.state.playing;
        }

        egui::Panel::top("topbar").show(ui, |ui| ui_topbar::show(self, ui));
        egui::Panel::bottom("errors")
            .resizable(true)
            .default_size(80.0)
            .show(ui, |ui| ui_errors::show(self, ui));
        egui::Panel::left("side").resizable(true).default_size(520.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui_editor::show(self, ui);
                ui.separator();
                ui_params::show(self, ui);
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let steps = if self.state.playing {
                self.state.steps_per_frame
            } else if self.state.step_once {
                1
            } else {
                0
            };
            self.state.step_once = false;
            let time = self.state.started.elapsed().as_secs_f32();
            let strokes = std::mem::take(&mut self.strokes);
            let (rect, response) = show_viewport(ui, &self.sim, steps, time, strokes);
            self.last_viewport = Some(rect);
            self.collect_strokes(&response, rect);
        });

        if self.state.playing || self.export_pending() || self.modulation_active() {
            ctx.request_repaint();
        }
    }
}

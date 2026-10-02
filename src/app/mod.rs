//! The egui application: wires the editors, params, transport and presets to the simulation.

pub mod state;
mod ui_editor;
mod ui_errors;
mod ui_params;
mod ui_topbar;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::preset::builtin::{load_builtin, BUILTINS, TEMPLATES};
use crate::preset::{preset_exists, scan_presets_dir, slug, Preset};
use crate::shader::params::pack_params;
use crate::shader::validate::ShaderFile;
use crate::sim::Simulation;
use crate::viewport::show_viewport;
use state::{
    build_shaders, diagnostics_from, prepare_preset_load, preset_to_state, resolve_values,
    state_to_preset, AppState, Diagnostic, PresetSource, RateMeter,
};

const PRESETS_DIR: &str = "presets";
const DEFAULT_BUILTIN: usize = 2; // Game of Life

pub struct App {
    sim: Arc<Mutex<Simulation>>,
    state: AppState,
    /// Set when an error entry is clicked; the matching editor moves its cursor there.
    pending_cursor: Option<(ShaderFile, usize)>,
    /// Steps-per-second meter for the status line.
    pub(crate) rate: RateMeter,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_preset(cc, None)
    }

    /// `start` is a built-in id (e.g. `rule30`) or a preset folder; `None` loads Game of Life.
    pub fn with_preset(cc: &eframe::CreationContext<'_>, start: Option<&str>) -> Self {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("wgpu render state (the app requires the wgpu backend)");
        let mut start_error = None;
        let (preset, source) = match start {
            None => (load_builtin(&BUILTINS[DEFAULT_BUILTIN]), PresetSource::Builtin(DEFAULT_BUILTIN)),
            Some(id) => match BUILTINS.iter().position(|b| b.id == id) {
                Some(i) => (load_builtin(&BUILTINS[i]), PresetSource::Builtin(i)),
                None => match Preset::load_dir(Path::new(id)) {
                    Ok(p) => (p, PresetSource::Disk(Path::new(id).to_path_buf())),
                    Err(e) => {
                        start_error = Some(format!("could not load preset `{id}`: {e:#}"));
                        (load_builtin(&BUILTINS[DEFAULT_BUILTIN]), PresetSource::Builtin(DEFAULT_BUILTIN))
                    }
                },
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
            rate: RateMeter::new(std::time::Instant::now(), 0),
        };
        app.state.disk_presets = scan_presets_dir(Path::new(PRESETS_DIR));
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
        self.state.errors.clear();
        self.state.modified = false;
        self.state.started = std::time::Instant::now();
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

    pub(crate) fn push_params(&self) {
        let packed = pack_params(&self.state.specs, &self.state.values);
        self.sim.lock().unwrap().set_params(packed);
    }

    pub(crate) fn apply_settings_and_reset(&mut self) {
        let mut sim = self.sim.lock().unwrap();
        sim.reconfigure(self.state.pending.clone());
        self.state.pending = sim.config().clone(); // reflect clamping
        drop(sim);
        self.state.started = std::time::Instant::now();
    }

    pub(crate) fn save(&mut self) {
        match self.state.source.clone() {
            PresetSource::Disk(path) => self.save_to(&path),
            _ => self.save_as(),
        }
    }

    /// Asks for a parent folder and saves into `<parent>/<slug(name)>/`, confirming first when
    /// that folder already holds a preset.
    pub(crate) fn save_as(&mut self) {
        let Some(parent) =
            rfd::FileDialog::new().set_title("Choose where to create the preset folder").pick_folder()
        else {
            return;
        };
        let target = parent.join(slug(&self.state.preset_name));
        if preset_exists(&target) {
            let answer = rfd::MessageDialog::new()
                .set_title("Overwrite preset?")
                .set_description(format!(
                    "{} already contains a preset. Overwrite it?",
                    target.display()
                ))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if answer != rfd::MessageDialogResult::Yes {
                return;
            }
        }
        self.save_to(&target);
    }

    fn save_to(&mut self, dir: &Path) {
        let preset = state_to_preset(&self.state);
        match preset.save_dir(dir) {
            Ok(()) => {
                self.state.source = PresetSource::Disk(dir.to_path_buf());
                self.state.modified = false;
                self.state.disk_presets = scan_presets_dir(Path::new(PRESETS_DIR));
            }
            Err(e) => self.report(format!("save failed: {e:#}")),
        }
    }

    pub(crate) fn rescan_presets(&mut self) {
        self.state.disk_presets = scan_presets_dir(Path::new(PRESETS_DIR));
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
            show_viewport(ui, &self.sim, steps, time);
        });

        if self.state.playing {
            ctx.request_repaint();
        }
    }
}

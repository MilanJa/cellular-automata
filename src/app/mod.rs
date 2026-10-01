//! The egui application: wires the editors, params, transport and presets to the simulation.

pub mod state;
mod ui_editor;
mod ui_errors;
mod ui_params;
mod ui_topbar;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::preset::builtin::{load_builtin, BUILTINS};
use crate::preset::{scan_presets_dir, Preset};
use crate::shader::params::pack_params;
use crate::shader::validate::{ShaderError, ShaderFile};
use crate::sim::Simulation;
use crate::viewport::show_viewport;
use state::{build_shaders, preset_to_state, resolve_values, state_to_preset, AppState, PresetSource};

const PRESETS_DIR: &str = "presets";
const DEFAULT_BUILTIN: usize = 2; // Game of Life

pub struct App {
    sim: Arc<Mutex<Simulation>>,
    state: AppState,
    /// Set when an error entry is clicked; the matching editor moves its cursor there.
    pending_cursor: Option<(ShaderFile, usize)>,
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
        let mut app = App { sim: Arc::new(Mutex::new(sim)), state, pending_cursor: None };
        app.state.disk_presets = scan_presets_dir(Path::new(PRESETS_DIR));
        app.apply_shaders_with_toml(&toml_params);
        if let Some(msg) = start_error {
            app.report(msg);
        }
        app
    }

    pub(crate) fn load_preset(&mut self, preset: Preset, source: PresetSource) {
        let (editor, config, spf, toml_params) = preset_to_state(&preset);
        self.state.editor = editor;
        self.state.pending = config.clone();
        self.state.steps_per_frame = spf;
        self.state.preset_name = preset.meta.name.clone();
        self.state.source = source;
        self.state.values.clear();
        self.sim.lock().unwrap().reconfigure(config);
        self.state.started = std::time::Instant::now();
        self.apply_shaders_with_toml(&toml_params);
    }

    pub(crate) fn apply_shaders(&mut self) {
        self.apply_shaders_with_toml(&BTreeMap::new());
    }

    fn apply_shaders_with_toml(&mut self, toml_params: &BTreeMap<String, toml::Value>) {
        match build_shaders(&self.state.editor) {
            Err(errors) => self.state.errors = errors,
            Ok((specs, rule, render)) => {
                let mut sim = self.sim.lock().unwrap();
                let mut errors: Vec<ShaderError> = Vec::new();
                if let Err(e) = sim.set_rule(ShaderFile::Rule, &rule) {
                    errors.extend(e);
                }
                if let Err(e) = sim.set_render(ShaderFile::Render, &render) {
                    errors.extend(e);
                }
                drop(sim);
                if errors.is_empty() {
                    self.state.values = resolve_values(&specs, toml_params, &self.state.values);
                    self.state.specs = specs;
                    self.state.editor.dirty = false;
                    self.push_params();
                }
                self.state.errors = errors;
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

    pub(crate) fn save_as(&mut self) {
        if let Some(dir) =
            rfd::FileDialog::new().set_title("Choose a folder for this preset").pick_folder()
        {
            self.save_to(&dir);
        }
    }

    fn save_to(&mut self, dir: &Path) {
        let preset = state_to_preset(&self.state);
        match preset.save_dir(dir) {
            Ok(()) => {
                self.state.source = PresetSource::Disk(dir.to_path_buf());
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
        self.state.errors = vec![ShaderError { file: ShaderFile::Rule, line: 1, column: 1, message }];
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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

//! The egui application: wires the editors, params, transport and presets to the simulation.

pub mod explorer;
mod explorer_ui;
pub mod modulation;
pub mod mutate;
pub mod state;
pub mod theme;
mod ui_editor;
mod ui_errors;
mod ui_inputs;
mod ui_overlays;
mod ui_params;
mod ui_topbar;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use web_time::Instant;

use crate::app::modulation::modulated_values_with_audio;
use crate::app::mutate::mutate_values;
use crate::audio::AudioInput;
use crate::audio::analysis::AudioLevels;
use crate::midi::MidiReceiver;
use crate::midi::mapping::{CcKey, MidiMap, apply_cc};
use crate::platform::{self, SavedLocation};
use crate::preset::Preset;
use crate::preset::builtin::{BUILTINS, TEMPLATES, load_builtin};
use crate::preset::bundle::{from_bundle, to_bundle};
use crate::shader::params::{ParamValue, pack_params};
use crate::shader::validate::ShaderFile;
use crate::sim::paint::{Stroke, pointer_to_cell};
use crate::sim::record::{FRAME_BUDGET_BYTES, RecordingSettings, encode_apng, max_frames, recording_filename};
use crate::sim::seed_image::{RgbaImage, decode_png, image_to_cells};
use crate::sim::stats::detect_stuck;
use crate::sim::{GpuContext, SimConfig, Simulation};
use crate::util::lock;
use crate::viewport::{LayerB, show_viewport};
use state::{
    AppState, PresetSource, RateMeter, build_shaders, prepare_preset_load, preset_to_state, resolve_values,
    state_to_preset,
};

const DEFAULT_BUILTIN: usize = 2; // Game of Life

/// How long an informational notice stays in the bottom panel.
const NOTICE_TTL: Duration = Duration::from_secs(8);
/// How often the browser draft is refreshed while there are unsaved changes.
const DRAFT_INTERVAL: Duration = Duration::from_secs(2);

/// What to load when the app opens.
pub enum Start {
    Default,
    /// A built-in id or, on the desktop, a preset folder.
    Named(String),
    /// A preset decoded from a share link.
    Shared(Box<Preset>),
}

/// An animation being captured: frames arrive through the image export path.
pub(crate) struct Recording {
    pub settings: RecordingSettings,
    pub frames: Vec<Vec<u8>>,
    pub size: Option<(u32, u32)>,
}

/// The in-app "save to browser" prompt (the web has no folder picker).
#[derive(Debug, Clone, Default)]
pub(crate) struct SaveDialog {
    pub name: String,
    /// Set once the user has seen the "already exists" notice; the next Save overwrites.
    pub confirm_overwrite: bool,
}

/// A non-shader message for the bottom panel. Separate from shader errors, so a save
/// confirmation never hides a compile error and vice versa.
#[derive(Debug, Clone)]
pub(crate) struct Notice {
    pub text: String,
    pub level: NoticeLevel,
    pub at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeLevel {
    /// Fades out by itself.
    Info,
    /// Stays until dismissed or replaced.
    Error,
}

/// Browser draft persistence: what was last written and when it was last checked.
struct Drafts {
    last_text: Option<String>,
    last_check: Instant,
}

pub struct App {
    ctx: Arc<GpuContext>,
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
    launched: Instant,
    /// The last image used as a seed, kept so mode changes can re-apply it.
    pub(crate) seed_image: Option<RgbaImage>,
    /// Snapshot to restore on the next frame (set by the timeline slider).
    pending_restore: Option<usize>,
    /// Timeline slider position while scrubbing; `None` follows the newest snapshot.
    pub(crate) scrub: Option<usize>,
    pub(crate) recording: Option<Recording>,
    /// Open Record dialog with its pending settings.
    pub(crate) record_dialog: Option<RecordingSettings>,
    /// The rule explorer window, while open.
    pub(crate) explorer: Option<explorer_ui::Explorer>,
    render_state: eframe::egui_wgpu::RenderState,
    /// Optional second automaton readable from the shaders via `other()`.
    pub(crate) layer_b: Option<Arc<Mutex<LayerB>>>,
    /// Microphone input while enabled, and the latest analysed levels.
    pub(crate) audio: Option<AudioInput>,
    pub(crate) audio_levels: AudioLevels,
    /// MIDI input while enabled, and the last knob that moved (for the learn UI).
    pub(crate) midi: Option<MidiReceiver>,
    pub(crate) last_cc: Option<CcKey>,
    /// Latest non-shader message (saves, imports, failures).
    pub(crate) notice: Option<Notice>,
    /// The param values uploaded last, with modulations applied; `None` while nothing is
    /// modulated. Computed once per frame and shown next to the sliders.
    pub(crate) live_values: Option<BTreeMap<String, ParamValue>>,
    drafts: Drafts,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_preset(cc, None)
    }

    /// `start` is a built-in id (e.g. `rule30`) or, on the desktop, a preset folder;
    /// `None` loads Game of Life.
    pub fn with_preset(cc: &eframe::CreationContext<'_>, start: Option<&str>) -> Self {
        Self::start(cc, start.map_or(Start::Default, |s| Start::Named(s.to_string())))
    }

    pub fn start(cc: &eframe::CreationContext<'_>, start: Start) -> Self {
        let rs = cc.wgpu_render_state.as_ref().expect("wgpu render state (the app requires the wgpu backend)");
        theme::apply(&cc.egui_ctx);
        let ctx = GpuContext::new(rs.device.clone(), rs.queue.clone(), rs.target_format);
        let mut start_error = None;
        let mut restored_draft = false;
        let default = || (load_builtin(&BUILTINS[DEFAULT_BUILTIN]), PresetSource::Builtin(DEFAULT_BUILTIN));
        let (mut preset, source) = match start {
            // A plain open restores unsaved work from the last visit (browser only).
            Start::Default => match platform::load_draft().and_then(|t| from_bundle(&t).ok()) {
                Some(p) => {
                    restored_draft = true;
                    (p, PresetSource::Imported)
                }
                None => default(),
            },
            Start::Shared(p) => (*p, PresetSource::Imported),
            Start::Named(id) => match BUILTINS.iter().position(|b| b.id == id) {
                Some(i) => (load_builtin(&BUILTINS[i]), PresetSource::Builtin(i)),
                None => {
                    let location = SavedLocation::Folder(std::path::PathBuf::from(&id));
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
        let layer_b = preset.layer_b.take();
        let (editor, config, spf, toml_params) = preset_to_state(&preset);
        let sim = Simulation::new(ctx.clone(), config.clone());
        let state =
            AppState::from_parts(editor, config, spf, Vec::new(), BTreeMap::new(), preset.meta.name.clone(), source);
        let now = Instant::now();
        let mut app = App {
            ctx,
            sim: Arc::new(Mutex::new(sim)),
            state,
            pending_cursor: None,
            rate: RateMeter::new(now, 0),
            save_dialog: None,
            strokes: Vec::new(),
            last_viewport: None,
            launched: now,
            seed_image: None,
            pending_restore: None,
            scrub: None,
            recording: None,
            record_dialog: None,
            explorer: None,
            render_state: rs.clone(),
            layer_b: None,
            audio: None,
            audio_levels: AudioLevels::default(),
            midi: None,
            last_cc: None,
            notice: None,
            live_values: None,
            drafts: Drafts { last_text: None, last_check: now },
        };
        app.state.saved_presets = platform::list_saved();
        app.state.modulations = preset.meta.modulation.clone();
        app.state.midi_map = MidiMap::from_bindings(preset.meta.midi.clone());
        app.state.blend = preset.meta.blend.clamp(0.0, 1.0);
        lock(&app.sim).set_blend(app.state.blend);
        app.apply_shaders_with_toml(&toml_params);
        if let Some(b) = layer_b {
            app.set_layer_b(Some(*b));
        }
        if restored_draft {
            app.state.modified = true;
            app.notify("Restored your unsaved work from the last visit. Save it, or load a preset to discard it.");
        }
        if let Some(msg) = start_error {
            app.report(msg);
        }
        app
    }

    /// Switches to `preset` only if its shaders compile; otherwise reports the errors and leaves
    /// the current editors, grid and params exactly as they were. Layer B follows the preset:
    /// the one it carries, or none.
    pub(crate) fn load_preset(&mut self, mut preset: Preset, source: PresetSource) {
        let layer_b = preset.layer_b.take();
        let loaded = match prepare_preset_load(&preset) {
            Ok(l) => l,
            Err(mut errors) => {
                for e in &mut errors {
                    e.message = format!("[{}] {}", preset.meta.name, e.message);
                }
                self.state.errors = errors;
                return;
            }
        };
        let mut sim = lock(&self.sim);
        if let Err(errors) = sim.set_pipelines(&loaded.rule, &loaded.render, &loaded.post, loaded.seed.as_ref()) {
            drop(sim);
            self.state.errors = errors;
            return;
        }
        sim.reconfigure(loaded.config.clone());
        let config = sim.config().clone();
        drop(sim);
        self.state.editor = loaded.editor;
        self.state.pending = config.clone();
        self.state.applied = config;
        self.state.steps_per_frame = loaded.steps_per_frame;
        self.state.preset_name = loaded.name;
        self.state.source = source;
        self.state.values = resolve_values(&loaded.specs, &loaded.toml_params, &BTreeMap::new());
        self.state.specs = loaded.specs;
        self.state.modulations = loaded.modulations;
        self.state.midi_map = MidiMap::from_bindings(loaded.midi);
        self.state.blend = loaded.blend;
        lock(&self.sim).set_blend(loaded.blend);
        self.state.errors.clear();
        self.state.modified = false;
        self.state.started = Instant::now();
        self.push_params();
        self.set_layer_b(layer_b.map(|b| *b));
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
        let _ = self.apply_shaders_with_toml(&BTreeMap::new());
    }

    /// Compiles every editor and installs the pipelines. Returns true on success.
    fn apply_shaders_with_toml(&mut self, toml_params: &BTreeMap<String, toml::Value>) -> bool {
        match build_shaders(&self.state.editor) {
            Err(errors) => {
                self.state.errors = errors;
                false
            }
            Ok(built) => {
                let result =
                    lock(&self.sim).set_pipelines(&built.rule, &built.render, &built.post, built.seed.as_ref());
                match result {
                    Err(errors) => {
                        self.state.errors = errors;
                        false
                    }
                    Ok(()) => {
                        self.state.values = resolve_values(&built.specs, toml_params, &self.state.values);
                        self.state.specs = built.specs;
                        self.state.editor.clear_dirty();
                        self.state.errors.clear();
                        self.push_params();
                        true
                    }
                }
            }
        }
    }

    /// The seed editor's shortcut: compile the editors and, when that worked, Reset so the new
    /// seed shows. A failed compile leaves the grid alone and shows the errors.
    pub(crate) fn apply_shaders_and_reset(&mut self) {
        if self.apply_shaders_with_toml(&BTreeMap::new()) {
            self.apply_settings_and_reset();
        }
    }

    /// Grid init switched to `code`: make sure there is a seed shader and that it is compiled,
    /// so the next Reset has something to run.
    pub(crate) fn init_set_to_code(&mut self) {
        self.state.modified = true;
        if self.state.editor.ensure_seed() {
            self.apply_shaders();
        }
    }

    /// Browser only: the backend checks shaders asynchronously; a rejection arrives here.
    fn poll_pipeline_checks(&mut self) {
        if let Some(errors) = lock(&self.sim).poll_pipeline_check() {
            self.state.errors = errors;
        }
        let Some(b) = &self.layer_b else { return };
        let errors = lock(b).sim.poll_pipeline_check();
        if let Some(mut errors) = errors {
            for e in &mut errors {
                e.message = format!("[layer B] {}", e.message);
            }
            self.state.errors = errors;
        }
    }

    pub(crate) fn set_blend(&mut self, blend: f32) {
        self.state.blend = blend.clamp(0.0, 1.0);
        self.state.modified = true;
        lock(&self.sim).set_blend(self.state.blend);
    }

    /// Uploads the current param values with any active modulations applied, and remembers the
    /// modulated values for the live readout.
    pub(crate) fn push_params(&mut self) {
        self.live_values = if self.state.modulations.is_empty() { None } else { Some(self.effective_values()) };
        let values = self.live_values.as_ref().unwrap_or(&self.state.values);
        let packed = pack_params(&self.state.specs, values);
        lock(&self.sim).set_params(packed);
    }

    /// Slider values with the LFOs and audio sources applied for this instant.
    fn effective_values(&self) -> BTreeMap<String, ParamValue> {
        let wall = self.launched.elapsed().as_secs_f32();
        let sim = self.state.started.elapsed().as_secs_f32();
        modulated_values_with_audio(
            &self.state.specs,
            &self.state.values,
            &self.state.modulations,
            wall,
            sim,
            &self.audio_levels,
        )
    }

    pub(crate) fn toggle_audio(&mut self) {
        if self.audio.is_some() {
            self.audio = None;
            self.audio_levels = AudioLevels::default();
            return;
        }
        match AudioInput::start() {
            Ok(input) => self.audio = Some(input),
            Err(e) => self.report(format!("could not open the microphone: {e:#}")),
        }
    }

    pub(crate) fn toggle_midi(&mut self) {
        if self.midi.is_some() {
            self.midi = None;
            self.state.midi_map.cancel_learn();
            return;
        }
        match MidiReceiver::start() {
            Ok(rx) => self.midi = Some(rx),
            Err(e) => self.report(format!("could not open MIDI input: {e:#}")),
        }
    }

    /// Applies received control changes: a pending learn binds the knob, bound knobs move params.
    fn poll_midi(&mut self) {
        let Some(rx) = &mut self.midi else { return };
        let messages = rx.poll();
        if let Some(err) = rx.error() {
            self.midi = None;
            self.state.midi_map.cancel_learn();
            self.report(err);
            return;
        }
        let mut moved = false;
        for msg in messages {
            self.last_cc = Some(msg.key());
            if self.state.midi_map.on_message(msg).is_some() {
                self.state.modified = true;
            }
            if !apply_cc(&self.state.specs, &mut self.state.values, &self.state.midi_map, msg).is_empty() {
                moved = true;
            }
        }
        if moved {
            self.state.modified = true;
            self.push_params();
        }
    }

    fn poll_audio(&mut self) {
        let Some(input) = &mut self.audio else { return };
        self.audio_levels = input.levels();
        #[cfg(target_arch = "wasm32")]
        if let Some(err) = input.error() {
            self.audio = None;
            self.report(err);
        }
    }

    /// True when some modulated param is one the shaders actually declare.
    fn modulation_active(&self) -> bool {
        self.state.modulations.keys().any(|k| self.state.specs.iter().any(|s| &s.name == k))
    }

    /// Reset: applies the pending grid settings to layer A, resizes layer B to match and
    /// restarts both from their init patterns.
    pub(crate) fn apply_settings_and_reset(&mut self) {
        let mut sim = lock(&self.sim);
        sim.reconfigure(self.state.pending.clone());
        self.state.pending = sim.config().clone(); // reflect clamping
        self.state.applied = self.state.pending.clone();
        drop(sim);
        self.state.started = Instant::now();
        self.clear_stats();
        self.sync_layer_b();
        if let Some(b) = &self.layer_b {
            lock(b).sim.reset();
        }
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
                self.discard_draft();
                self.notify(format!("Saved to {}.", location.describe()));
            }
            Err(e) => self.report(format!("save failed: {e:#}")),
        }
    }

    /// Removes the current preset from where it is saved (after confirmation). The scene stays
    /// open as an unsaved preset.
    pub(crate) fn delete_saved_preset(&mut self) {
        let PresetSource::Saved(location) = self.state.source.clone() else { return };
        let question = format!("Delete {}? This cannot be undone.", location.describe());
        if !platform::confirm("Delete preset?", &question) {
            return;
        }
        match platform::delete_saved(&location) {
            Ok(()) => {
                self.state.source = PresetSource::Imported;
                self.state.modified = true;
                self.state.saved_presets = platform::list_saved();
                self.notify(format!("Deleted {}. The scene is still open, unsaved.", location.describe()));
            }
            Err(e) => self.report(format!("delete failed: {e:#}")),
        }
    }

    /// Browser only: every couple of seconds, while there are unsaved changes, writes the scene
    /// to local storage so a closed tab does not lose it.
    fn maybe_save_draft(&mut self) {
        if !platform::is_web() || self.drafts.last_check.elapsed() < DRAFT_INTERVAL {
            return;
        }
        self.drafts.last_check = Instant::now();
        if !self.state.modified {
            return;
        }
        let Ok(text) = to_bundle(&state_to_preset(&self.state)) else { return };
        if self.drafts.last_text.as_deref() != Some(text.as_str()) {
            platform::save_draft(&text);
            self.drafts.last_text = Some(text);
        }
    }

    fn discard_draft(&mut self) {
        platform::clear_draft();
        self.drafts.last_text = None;
    }

    /// Nudges every param; the previous values go on the undo stack.
    pub(crate) fn mutate(&mut self) {
        self.state.undo.push(self.state.values.clone());
        if self.state.undo.len() > 20 {
            self.state.undo.remove(0);
        }
        self.state.mutate_count += 1;
        let seed = self.state.mutate_count ^ (self.launched.elapsed().as_millis() as u64);
        self.state.values = mutate_values(&self.state.specs, &self.state.values, seed);
        self.state.modified = true;
        self.push_params();
    }

    pub(crate) fn undo_mutate(&mut self) {
        if let Some(previous) = self.state.undo.pop() {
            self.state.values = previous;
            self.state.modified = true;
            self.push_params();
        }
    }

    /// Drains finished statistics, updates the history and the stuck detector, and reseeds when
    /// asked to.
    fn poll_stats(&mut self) {
        let samples = lock(&self.sim).poll_stats();
        if samples.is_empty() {
            return;
        }
        for s in samples {
            self.state.stats.push_back(s);
        }
        while self.state.stats.len() > 300 {
            self.state.stats.pop_front();
        }
        let stuck = detect_stuck(self.state.stats.make_contiguous(), 48);
        self.state.stuck = stuck;
        match (self.state.stuck, self.state.stuck_since) {
            (Some(_), None) => self.state.stuck_since = Some(Instant::now()),
            (None, _) => self.state.stuck_since = None,
            _ => {}
        }
        if self.state.auto_reseed
            && let Some(since) = self.state.stuck_since
            && since.elapsed().as_secs_f32() > 2.0
        {
            self.state.pending.seed = self.state.pending.seed.wrapping_mul(1664525).wrapping_add(1013904223);
            self.apply_settings_and_reset();
        }
    }

    pub(crate) fn request_seed_image(&mut self) {
        platform::request_image_import();
    }

    /// Decodes PNG bytes and writes them into the grid using the current seed mode.
    pub(crate) fn seed_from_png(&mut self, bytes: &[u8]) {
        match decode_png(bytes) {
            Ok(img) => {
                self.seed_image = Some(img);
                self.apply_seed_image();
            }
            Err(e) => self.report(format!("could not read image: {e:#}")),
        }
    }

    /// Writes the remembered seed image into the grid (again).
    pub(crate) fn apply_seed_image(&mut self) {
        let Some(img) = &self.seed_image else { return };
        let mut sim = lock(&self.sim);
        let (w, h) = {
            let c = sim.config();
            (c.width, c.height)
        };
        let cells = image_to_cells(img, w, h, self.state.seed_mode);
        let result = sim.load_state(&cells);
        drop(sim);
        if let Err(e) = result {
            self.report(e);
        }
        self.clear_stats();
    }

    fn poll_dropped_and_imported_files(&mut self) {
        if let Some(bytes) = platform::poll_image_import() {
            self.seed_from_png(&bytes);
        }
    }

    /// Dropped files: PNGs seed the grid, `.toml` bundles are imported as presets.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in &dropped {
            platform::queue_dropped_file(file.as_ref());
        }
        for entry in platform::poll_dropped_files() {
            let (name, bytes) = match entry {
                Ok(f) => f,
                Err(message) => {
                    self.report(format!("could not read dropped file {message}"));
                    continue;
                }
            };
            let lower = name.to_lowercase();
            if lower.ends_with(".png") {
                self.seed_from_png(&bytes);
            } else if lower.ends_with(".toml") {
                match std::str::from_utf8(&bytes).map_err(anyhow::Error::from).and_then(from_bundle) {
                    Ok(p) => {
                        self.load_preset(p, PresetSource::Imported);
                        self.state.modified = true;
                    }
                    Err(e) => {
                        let looks_like_folder_meta = std::str::from_utf8(&bytes)
                            .is_ok_and(|t| t.contains("steps_per_frame") && !t.contains("[meta]"));
                        let hint = if looks_like_folder_meta {
                            " It looks like a preset folder's preset.toml; drop a bundle exported with File > Export bundle instead."
                        } else {
                            ""
                        };
                        self.report(format!("dropped file is not a preset bundle: {e:#}.{hint}"));
                    }
                }
            } else {
                self.report(format!("unsupported file type: {name} (PNG images and .toml bundles)"));
            }
        }
    }

    pub(crate) fn clear_stats(&mut self) {
        self.state.stats.clear();
        self.state.stuck = None;
        self.state.stuck_since = None;
    }

    /// Timeline slider moved: pause and restore that snapshot on the next frame.
    pub(crate) fn scrub_to(&mut self, index: usize) {
        self.state.playing = false;
        self.scrub = Some(index);
        self.pending_restore = Some(index);
        self.clear_stats();
    }

    /// Starts a PNG export of the current state at `scale` pixels per cell.
    pub(crate) fn export_image(&mut self, scale: u32) {
        let mut sim = lock(&self.sim);
        let filename = crate::sim::export::image_filename(&self.state.preset_name, sim.frame());
        let result = sim.start_export(scale, filename);
        drop(sim);
        if let Err(e) = result {
            self.report(e);
        }
    }

    fn poll_export_image(&mut self) {
        let result = lock(&self.sim).poll_export();
        match result {
            None => {}
            Some(Err(e)) => {
                self.recording = None;
                self.report(e);
            }
            Some(Ok(img)) => {
                if self.recording.is_some() {
                    self.record_frame(img);
                } else {
                    match img.to_png() {
                        Ok(png) => match platform::save_png(&img.filename, &png) {
                            Ok(Some(where_)) => self.notify(format!("Image saved: {where_}")),
                            Ok(None) => {}
                            Err(e) => self.report(format!("could not save image: {e:#}")),
                        },
                        Err(e) => self.report(format!("could not encode image: {e:#}")),
                    }
                }
            }
        }
        self.capture_next_frame();
    }

    /// Loads `preset` as layer B, sized like layer A, and wires both layers' `other()`. The
    /// preset is remembered so Save and share links carry it.
    pub(crate) fn set_layer_b(&mut self, preset: Option<Preset>) {
        self.layer_b = None;
        self.state.layer_b_preset = None;
        let mut a = lock(&self.sim);
        let Some(mut preset) = preset else {
            a.set_other(None);
            return;
        };
        preset.layer_b = None; // one level of nesting only
        let loaded = match prepare_preset_load(&preset) {
            Ok(l) => l,
            Err(mut errors) => {
                drop(a);
                for e in &mut errors {
                    e.message = format!("[layer B: {}] {}", preset.meta.name, e.message);
                }
                self.state.errors = errors;
                return;
            }
        };
        let a_cfg = a.config();
        let config = SimConfig {
            mode: a_cfg.mode,
            width: a_cfg.width,
            height: a_cfg.height,
            init: loaded.config.init.clone(),
            seed: loaded.config.seed,
        };
        let mut b = Simulation::new(self.ctx.clone(), config);
        if let Err(errors) = b.set_pipelines(&loaded.rule, &loaded.render, &loaded.post, loaded.seed.as_ref()) {
            drop(a);
            self.state.errors = errors;
            return;
        }
        let values = resolve_values(&loaded.specs, &loaded.toml_params, &BTreeMap::new());
        b.set_params(pack_params(&loaded.specs, &values));
        b.disable_history();
        let mirror_a = a.create_mirror_texture();
        let mirror_b = b.create_mirror_texture();
        a.set_other(Some(&mirror_b));
        b.set_other(Some(&mirror_a));
        drop(a);
        self.layer_b = Some(Arc::new(Mutex::new(LayerB { sim: b, name: loaded.name, mirror_a, mirror_b })));
        self.state.layer_b_preset = Some(preset);
    }

    /// After layer A changed size or mode, rebuild layer B to match.
    fn sync_layer_b(&mut self) {
        let Some(b) = &self.layer_b else { return };
        let a_cfg = lock(&self.sim).config().clone();
        let mut b_guard = lock(b);
        let b_cfg = b_guard.sim.config().clone();
        if b_cfg.width == a_cfg.width && b_cfg.height == a_cfg.height && b_cfg.mode == a_cfg.mode {
            return;
        }
        let new_cfg = SimConfig {
            mode: a_cfg.mode,
            width: a_cfg.width,
            height: a_cfg.height,
            init: b_cfg.init,
            seed: b_cfg.seed,
        };
        b_guard.sim.reconfigure(new_cfg);
        b_guard.mirror_a = lock(&self.sim).create_mirror_texture();
        b_guard.mirror_b = b_guard.sim.create_mirror_texture();
        lock(&self.sim).set_other(Some(&b_guard.mirror_b));
        let mirror_a = b_guard.mirror_a.clone();
        b_guard.sim.set_other(Some(&mirror_a));
    }

    pub(crate) fn layer_b_name(&self) -> Option<String> {
        self.layer_b.as_ref().map(|b| lock(b).name.clone())
    }

    pub(crate) fn toggle_explorer(&mut self) {
        if self.explorer.is_some() {
            self.explorer = None;
        } else {
            let seed = self.launched.elapsed().as_millis() as u64 ^ 0xC0FFEE;
            self.explorer =
                Some(explorer_ui::Explorer::new(self.ctx.clone(), self.render_state.renderer.clone(), seed));
        }
    }

    /// Opens the Record dialog with the last used settings, clamped to the memory budget.
    pub(crate) fn open_record_dialog(&mut self) {
        let mut s = RecordingSettings::default();
        let (w, h) = {
            let sim = lock(&self.sim);
            let c = sim.config();
            (c.width, c.height)
        };
        s.frames = s.frames.min(max_frames(w * s.scale, h * s.scale, FRAME_BUDGET_BYTES));
        self.record_dialog = Some(s);
    }

    pub(crate) fn start_recording(&mut self, settings: RecordingSettings) {
        if self.recording.is_some() {
            return;
        }
        self.record_dialog = None;
        self.recording = Some(Recording { settings, frames: Vec::with_capacity(settings.frames as usize), size: None });
        self.state.playing = true;
        self.capture_next_frame();
    }

    pub(crate) fn stop_recording(&mut self) {
        if let Some(rec) = self.recording.take()
            && !rec.frames.is_empty()
            && let Some((w, h)) = rec.size
        {
            self.finish_recording(w, h, &rec.frames, rec.settings.fps);
        }
    }

    pub(crate) fn recording_progress(&self) -> Option<(usize, u32)> {
        self.recording.as_ref().map(|r| (r.frames.len(), r.settings.frames))
    }

    /// Kicks off the capture of the next frame while a recording is active and no export is
    /// already in flight.
    fn capture_next_frame(&mut self) {
        let Some(rec) = &self.recording else { return };
        if rec.frames.len() as u32 >= rec.settings.frames {
            return;
        }
        let scale = rec.settings.scale;
        let mut sim = lock(&self.sim);
        if sim.export_pending() {
            return;
        }
        let result = sim.start_export(scale, "frame".into());
        drop(sim);
        if let Err(e) = result {
            self.recording = None;
            self.report(format!("recording stopped: {e}"));
        }
    }

    fn record_frame(&mut self, img: crate::sim::export::ExportedImage) {
        let Some(rec) = &mut self.recording else { return };
        if rec.size.is_none() {
            rec.size = Some((img.width, img.height));
        }
        if rec.size != Some((img.width, img.height)) {
            // Grid or scale changed mid-recording: keep what we have.
            let (w, h) = rec.size.unwrap_or((img.width, img.height));
            let frames = std::mem::take(&mut rec.frames);
            let fps = rec.settings.fps;
            self.recording = None;
            self.finish_recording(w, h, &frames, fps);
            return;
        }
        rec.frames.push(img.rgba);
        if rec.frames.len() as u32 >= rec.settings.frames {
            let rec = self.recording.take().unwrap();
            self.finish_recording(img.width, img.height, &rec.frames, rec.settings.fps);
        }
    }

    fn finish_recording(&mut self, width: u32, height: u32, frames: &[Vec<u8>], fps: u16) {
        match encode_apng(width, height, frames, fps) {
            Ok(bytes) => {
                let name = recording_filename(&self.state.preset_name);
                match platform::save_png(&name, &bytes) {
                    Ok(Some(where_)) => self.notify(format!("Animation saved ({} frames): {where_}", frames.len())),
                    Ok(None) => {}
                    Err(e) => self.report(format!("could not save animation: {e:#}")),
                }
            }
            Err(e) => self.report(format!("could not encode animation: {e:#}")),
        }
    }

    pub(crate) fn export_pending(&self) -> bool {
        lock(&self.sim).export_pending()
    }

    /// Copies a link that reproduces the current scene to the clipboard.
    pub(crate) fn share(&mut self, ctx: &egui::Context) {
        let preset = state_to_preset(&self.state);
        match crate::preset::share::encode_share_code(&preset) {
            Ok(code) => {
                let url = crate::preset::share::share_url(&platform::share_base_url(), &code);
                let len = url.len();
                ctx.copy_text(url);
                self.notify(format!("Link copied to the clipboard ({len} characters)."));
            }
            Err(e) => self.report(format!("could not build a share link: {e:#}")),
        }
    }

    pub(crate) fn export(&mut self) {
        let preset = state_to_preset(&self.state);
        match platform::export_bundle(&preset) {
            Ok(Some(where_)) => self.notify(format!("Bundle exported: {where_}")),
            Ok(None) => {}
            Err(e) => self.report(format!("export failed: {e:#}")),
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
        let (primary, secondary) = response.ctx.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
        let value = if secondary && !primary { [0.0, 0.0, 0.0, 1.0] } else { self.state.brush_value };
        let (w, h) = {
            let sim = lock(&self.sim);
            let c = sim.config();
            (c.width, c.height)
        };
        let r = (rect.min.x, rect.min.y, rect.width(), rect.height());
        if let Some((x, y)) = pointer_to_cell(pos.x, pos.y, r, w, h) {
            self.strokes.push(Stroke { x, y, radius: self.state.brush_radius, value });
            response.ctx.request_repaint();
        }
    }

    /// Shows a failure in the bottom panel (stays until dismissed or replaced).
    pub(crate) fn report(&mut self, message: impl Into<String>) {
        self.notice = Some(Notice { text: message.into(), level: NoticeLevel::Error, at: Instant::now() });
    }

    /// Shows a confirmation in the bottom panel (fades out by itself).
    pub(crate) fn notify(&mut self, message: impl Into<String>) {
        self.notice = Some(Notice { text: message.into(), level: NoticeLevel::Info, at: Instant::now() });
    }

    fn expire_notice(&mut self) {
        if self.notice.as_ref().is_some_and(|n| n.level == NoticeLevel::Info && n.at.elapsed() > NOTICE_TTL) {
            self.notice = None;
        }
    }

    /// True while a text editor has keyboard focus (so Space types rather than toggles play).
    fn typing(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused()).is_some_and(|id| egui::widgets::text_edit::TextEditState::load(ctx, id).is_some())
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(message) = self.ctx.take_device_lost() {
            self.state.playing = false;
            self.report(format!("{message}. Restart the application to continue."));
        }
        self.expire_notice();
        self.poll_pipeline_checks();
        self.poll_import();
        self.poll_export_image();
        self.poll_stats();
        self.poll_audio();
        self.poll_midi();
        self.poll_dropped_and_imported_files();
        self.handle_dropped_files(&ctx);
        if self.modulation_active() {
            self.push_params();
        }
        self.maybe_save_draft();
        ui_topbar::save_dialog(self, &ctx);
        // Global shortcut: Ctrl/Cmd+Enter applies shaders. Consume it before the editors see it.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            self.apply_shaders();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&ui_topbar::SAVE_SHORTCUT)) {
            self.save();
        }
        if !Self::typing(&ctx) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            self.state.playing = !self.state.playing;
        }

        let chrome = theme::chrome_frame(ui.style());
        egui::Panel::top("topbar").frame(chrome).show(ui, |ui| ui_topbar::show(self, ui));
        egui::Panel::bottom("errors")
            .resizable(true)
            .default_size(80.0)
            .frame(chrome)
            .show(ui, |ui| ui_errors::show(self, ui));
        egui::Panel::left("side").resizable(true).default_size(520.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui_editor::show(self, ui);
                ui_params::show(self, ui);
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            // While a recording waits for a frame to come back, the simulation holds still, so
            // consecutive frames are always exactly `steps_per_frame` steps apart.
            let waiting_for_capture = self.recording.is_some() && self.export_pending();
            let steps = if waiting_for_capture {
                0
            } else if self.state.playing {
                self.state.steps_per_frame
            } else if self.state.step_once {
                1
            } else {
                0
            };
            self.state.step_once = false;
            let time = self.state.started.elapsed().as_secs_f32();
            let strokes = std::mem::take(&mut self.strokes);
            let restore = self.pending_restore.take();
            let layer_b = self.layer_b.clone();
            let timeline_height = 26.0;
            let viewport_size = egui::vec2(ui.available_width(), (ui.available_height() - timeline_height).max(1.0));
            let (rect, response) = ui
                .allocate_ui_with_layout(viewport_size, egui::Layout::top_down(egui::Align::Min), |ui| {
                    show_viewport(ui, &self.sim, steps, time, strokes, restore, layer_b)
                })
                .inner;
            self.last_viewport = Some(rect);
            self.collect_strokes(&response, rect);
            ui_overlays::brush(self, ui, rect);
            ui_overlays::seed_image(self, ui, rect);
            ui_topbar::timeline(self, ui);
        });

        if self.state.playing
            || self.export_pending()
            || self.modulation_active()
            || self.recording.is_some()
            || self.audio.is_some()
            || self.midi.is_some()
            || self.notice.as_ref().is_some_and(|n| n.level == NoticeLevel::Info)
        {
            ctx.request_repaint();
        }
        ui_topbar::record_dialog(self, &ctx);
        explorer_ui::window(self, &ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::validate::ShaderError;

    #[test]
    fn info_notices_expire_and_errors_stay() {
        let old = Instant::now() - NOTICE_TTL - Duration::from_secs(1);
        let info = Notice { text: "saved".into(), level: NoticeLevel::Info, at: old };
        let error = Notice { text: "failed".into(), level: NoticeLevel::Error, at: old };
        let expired = |n: &Notice| n.level == NoticeLevel::Info && n.at.elapsed() > NOTICE_TTL;
        assert!(expired(&info));
        assert!(!expired(&error));
    }

    #[test]
    fn shader_errors_keep_their_prefix_when_layer_b_fails() {
        let mut errors =
            vec![ShaderError { file: ShaderFile::Rule, line: 2, column: 1, message: "boom".into(), hint: None }];
        for e in &mut errors {
            e.message = format!("[layer B] {}", e.message);
        }
        assert_eq!(errors[0].message, "[layer B] boom");
    }
}

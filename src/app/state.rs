//! Pure application state and the shader build flow (no GPU, no UI).

use std::collections::BTreeMap;
use web_time::Instant;

use crate::app::modulation::Modulation;
use crate::platform::SavedLocation;

use crate::preset::{param_value_from_toml, param_value_to_toml, InitPattern, Preset, PresetMeta};
use crate::shader::assemble::{
    assemble_post, assemble_render, assemble_rule, assemble_rule_pair, Assembled, DEFAULT_POST,
};
use crate::shader::params::{
    merge_params, params_wgsl, parse_params, ParamError, ParamSpec, ParamValue,
};
use crate::shader::validate::{validate, validate_pair, ShaderError, ShaderFile};
use crate::sim::SimConfig;

/// Upper bounds applied to values coming from the UI *and* from hand-edited preset files.
pub const MAX_STEPS_PER_FRAME: u32 = 256;
pub const MAX_GRID_SIZE: u32 = 4096;

#[derive(Debug, Clone, PartialEq)]
pub enum PresetSource {
    Builtin(usize),
    Template(usize),
    /// Imported from a bundle file; not stored anywhere yet.
    Imported,
    Saved(SavedLocation),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EditorState {
    pub rule: String,
    /// Optional second rule; empty means a single rule.
    pub rule_b: String,
    pub render: String,
    /// Post-processing shader text; `DEFAULT_POST` when the preset has none.
    pub post: String,
    /// True when the rule editor changed since the last successful apply.
    pub rule_dirty: bool,
    pub rule_b_dirty: bool,
    /// True when the render editor changed since the last successful apply.
    pub render_dirty: bool,
    pub post_dirty: bool,
}

impl EditorState {
    pub fn mark_dirty(&mut self, file: ShaderFile) {
        match file {
            ShaderFile::Rule => self.rule_dirty = true,
            ShaderFile::RuleB => self.rule_b_dirty = true,
            ShaderFile::Render => self.render_dirty = true,
            ShaderFile::Post => self.post_dirty = true,
        }
    }

    pub fn is_dirty(&self, file: ShaderFile) -> bool {
        match file {
            ShaderFile::Rule => self.rule_dirty,
            ShaderFile::RuleB => self.rule_b_dirty,
            ShaderFile::Render => self.render_dirty,
            ShaderFile::Post => self.post_dirty,
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.rule_dirty || self.rule_b_dirty || self.render_dirty || self.post_dirty
    }

    pub fn clear_dirty(&mut self) {
        self.rule_dirty = false;
        self.rule_b_dirty = false;
        self.render_dirty = false;
        self.post_dirty = false;
    }

    pub fn has_rule_b(&self) -> bool {
        !self.rule_b.trim().is_empty()
    }

    /// Rule B text for a preset: `None` when the editor is empty.
    pub fn rule_b_for_preset(&self) -> Option<String> {
        if self.has_rule_b() { Some(self.rule_b.clone()) } else { None }
    }

    /// The post text to store in a preset: `None` when it is the untouched pass-through.
    pub fn post_for_preset(&self) -> Option<String> {
        if self.post.trim() == DEFAULT_POST.trim() { None } else { Some(self.post.clone()) }
    }
}

/// One line in the error panel: either a located shader error or a general message.
#[derive(Debug, Clone, PartialEq)]
pub enum Diagnostic {
    Shader(ShaderError),
    General(String),
}

impl Diagnostic {
    pub fn text(&self) -> String {
        match self {
            Diagnostic::Shader(e) => {
                format!("{}:{}:{}  {}", e.file.label(), e.line, e.column, e.message)
            }
            Diagnostic::General(m) => m.clone(),
        }
    }

    /// Where a click should take the cursor, if anywhere.
    pub fn location(&self) -> Option<(ShaderFile, usize)> {
        match self {
            Diagnostic::Shader(e) => Some((e.file, e.line)),
            Diagnostic::General(_) => None,
        }
    }

    pub fn shader_file(&self) -> Option<ShaderFile> {
        self.location().map(|(f, _)| f)
    }
}

pub fn diagnostics_from(errors: Vec<ShaderError>) -> Vec<Diagnostic> {
    errors.into_iter().map(Diagnostic::Shader).collect()
}

pub struct AppState {
    pub editor: EditorState,
    pub specs: Vec<ParamSpec>,
    pub values: BTreeMap<String, ParamValue>,
    /// Active LFOs by param name.
    pub modulations: BTreeMap<String, Modulation>,
    pub errors: Vec<Diagnostic>,
    /// True when anything savable changed since the preset was loaded or saved.
    pub modified: bool,
    pub playing: bool,
    pub step_once: bool,
    pub steps_per_frame: u32,
    /// Grid settings as edited in the UI; applied to the simulation on Reset.
    pub pending: SimConfig,
    pub preset_name: String,
    pub source: PresetSource,
    pub saved_presets: Vec<(String, SavedLocation)>,
    pub started: Instant,
    /// Mouse brush: radius in cells and the value painted with the left button.
    pub brush_radius: f32,
    pub brush_value: [f32; 4],
    /// Previous param values, most recent last, for Undo after Mutate.
    pub undo: Vec<BTreeMap<String, ParamValue>>,
    pub mutate_count: u64,
    /// Recent statistics samples, oldest first.
    pub stats: std::collections::VecDeque<crate::sim::stats::StatsSample>,
    pub stuck: Option<crate::sim::stats::Stuck>,
    pub stuck_since: Option<Instant>,
    pub auto_reseed: bool,
    /// How an imported image is turned into cells.
    pub seed_mode: crate::sim::seed_image::SeedMode,
    /// Crossfade between rule A and rule B.
    pub blend: f32,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        editor: EditorState,
        pending: SimConfig,
        steps_per_frame: u32,
        specs: Vec<ParamSpec>,
        values: BTreeMap<String, ParamValue>,
        preset_name: String,
        source: PresetSource,
    ) -> Self {
        AppState {
            editor,
            specs,
            values,
            modulations: BTreeMap::new(),
            errors: Vec::new(),
            modified: false,
            playing: true,
            step_once: false,
            steps_per_frame: steps_per_frame.max(1),
            pending,
            preset_name,
            source,
            saved_presets: Vec::new(),
            started: Instant::now(),
            brush_radius: 2.0,
            brush_value: [1.0, 0.0, 0.0, 1.0],
            undo: Vec::new(),
            mutate_count: 0,
            stats: std::collections::VecDeque::new(),
            stuck: None,
            stuck_since: None,
            auto_reseed: false,
            seed_mode: Default::default(),
            blend: 0.0,
        }
    }
}

/// Measures simulation steps per second from the step counter, sampling at most twice a second.
#[derive(Debug, Clone)]
pub struct RateMeter {
    last_time: Instant,
    last_frame: u32,
    rate: f32,
}

impl RateMeter {
    pub const WINDOW_SECS: f32 = 0.5;

    pub fn new(now: Instant, frame: u32) -> Self {
        RateMeter { last_time: now, last_frame: frame, rate: 0.0 }
    }

    /// Feeds the current step counter and returns the latest estimate.
    pub fn update(&mut self, now: Instant, frame: u32) -> f32 {
        if frame < self.last_frame {
            // Counter went backwards (reset): start a new window.
            self.last_time = now;
            self.last_frame = frame;
            self.rate = 0.0;
            return self.rate;
        }
        let dt = now.saturating_duration_since(self.last_time).as_secs_f32();
        if dt >= Self::WINDOW_SECS {
            self.rate = (frame - self.last_frame) as f32 / dt;
            self.last_time = now;
            self.last_frame = frame;
        }
        self.rate
    }
}

fn param_err(file: ShaderFile, e: ParamError) -> ShaderError {
    ShaderError { file, line: e.line.max(1), column: 1, message: e.message, hint: None }
}

/// Parses params from all three editors, merges them, assembles and validates the shaders.
/// Returns every error found (rule, render, post order) or the specs plus the assembled sources.
pub fn build_shaders(
    editor: &EditorState,
) -> Result<(Vec<ParamSpec>, Assembled, Assembled, Assembled), Vec<ShaderError>> {
    let rule_params =
        parse_params(&editor.rule).map_err(|e| vec![param_err(ShaderFile::Rule, e)])?;
    let rule_b_params =
        parse_params(&editor.rule_b).map_err(|e| vec![param_err(ShaderFile::RuleB, e)])?;
    let render_params =
        parse_params(&editor.render).map_err(|e| vec![param_err(ShaderFile::Render, e)])?;
    let post_params =
        parse_params(&editor.post).map_err(|e| vec![param_err(ShaderFile::Post, e)])?;
    let specs = merge_params(rule_params, rule_b_params).map_err(|e| vec![param_err(ShaderFile::RuleB, e)])?;
    let specs = merge_params(specs, render_params)
        .map_err(|e| vec![param_err(ShaderFile::Render, e)])?;
    let specs = merge_params(specs, post_params).map_err(|e| vec![param_err(ShaderFile::Post, e)])?;
    let pw = params_wgsl(&specs);
    let pair = editor.has_rule_b();
    let rule = if pair {
        assemble_rule_pair(&editor.rule, &editor.rule_b, &pw)
    } else {
        assemble_rule(&editor.rule, &pw)
    };
    let render = assemble_render(&editor.render, &pw);
    let post = assemble_post(&editor.post, &pw);
    let mut errors = Vec::new();
    let rule_result = if pair { validate_pair(&rule) } else { validate(ShaderFile::Rule, &rule) };
    if let Err(e) = rule_result {
        errors.extend(e);
    }
    if let Err(e) = validate(ShaderFile::Render, &render) {
        errors.extend(e);
    }
    if let Err(e) = validate(ShaderFile::Post, &post) {
        errors.extend(e);
    }
    if errors.is_empty() { Ok((specs, rule, render, post)) } else { Err(errors) }
}

/// Splits a preset into editor text, grid config, steps-per-frame and its raw TOML param values.
pub fn preset_to_state(
    preset: &Preset,
) -> (EditorState, SimConfig, u32, BTreeMap<String, toml::Value>) {
    // Modulations travel separately through `LoadedPreset::modulations`.
    let m = &preset.meta;
    let editor = EditorState {
        rule: preset.rule.clone(),
        rule_b: preset.rule_b.clone().unwrap_or_default(),
        render: preset.render.clone(),
        post: preset.post.clone().unwrap_or_else(|| DEFAULT_POST.to_string()),
        ..Default::default()
    };
    let init = match &m.init {
        InitPattern::Random { density } => InitPattern::Random {
            density: if density.is_finite() { density.clamp(0.0, 1.0) } else { 0.5 },
        },
        other => other.clone(),
    };
    let config = SimConfig {
        mode: m.mode,
        width: m.width.clamp(1, MAX_GRID_SIZE),
        height: m.height.clamp(1, MAX_GRID_SIZE),
        init,
        seed: m.seed,
    };
    (editor, config, m.steps_per_frame.clamp(1, MAX_STEPS_PER_FRAME), m.params.clone())
}

/// Everything needed to switch the app to a preset, computed *before* any state is touched.
#[derive(Debug)]
pub struct LoadedPreset {
    pub editor: EditorState,
    pub config: SimConfig,
    pub steps_per_frame: u32,
    pub specs: Vec<ParamSpec>,
    pub rule: Assembled,
    pub render: Assembled,
    pub post: Assembled,
    pub toml_params: BTreeMap<String, toml::Value>,
    pub modulations: BTreeMap<String, Modulation>,
    pub blend: f32,
    pub name: String,
}

/// Validates a preset's shaders and prepares the new state. Fails without side effects, so a
/// preset with a broken shader leaves the current session untouched.
pub fn prepare_preset_load(preset: &Preset) -> Result<LoadedPreset, Vec<ShaderError>> {
    let (editor, config, steps_per_frame, toml_params) = preset_to_state(preset);
    let (specs, rule, render, post) = build_shaders(&editor)?;
    Ok(LoadedPreset {
        editor,
        config,
        steps_per_frame,
        specs,
        rule,
        render,
        post,
        toml_params,
        modulations: preset.meta.modulation.clone(),
        blend: preset.meta.blend.clamp(0.0, 1.0),
        name: preset.meta.name.clone(),
    })
}

pub fn state_to_preset(state: &AppState) -> Preset {
    let params = state
        .specs
        .iter()
        .filter_map(|s| state.values.get(&s.name).map(|v| (s.name.clone(), param_value_to_toml(v))))
        .collect();
    let c = &state.pending;
    Preset {
        meta: PresetMeta {
            name: state.preset_name.clone(),
            mode: c.mode,
            width: c.width,
            height: c.height,
            steps_per_frame: state.steps_per_frame,
            seed: c.seed,
            init: c.init.clone(),
            blend: state.blend,
            params,
            modulation: state
                .modulations
                .iter()
                .filter(|(name, _)| state.specs.iter().any(|s| &s.name == *name))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        },
        rule: state.editor.rule.clone(),
        render: state.editor.render.clone(),
        post: state.editor.post_for_preset(),
        rule_b: state.editor.rule_b_for_preset(),
    }
}

/// Value precedence: TOML value (if its type matches) > previous value (if type matches) > default.
pub fn resolve_values(
    specs: &[ParamSpec],
    toml_params: &BTreeMap<String, toml::Value>,
    previous: &BTreeMap<String, ParamValue>,
) -> BTreeMap<String, ParamValue> {
    specs
        .iter()
        .map(|s| {
            let v = toml_params
                .get(&s.name)
                .and_then(|t| param_value_from_toml(s.ty, t))
                .or_else(|| previous.get(&s.name).filter(|p| p.ty() == s.ty).cloned())
                .unwrap_or_else(|| s.default.clone());
            (s.name.clone(), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::builtin::{load_builtin, BUILTINS};
    use crate::shader::params::{parse_params, ParamValue};

    #[test]
    fn build_shaders_for_every_builtin_succeeds() {
        for b in BUILTINS {
            let p = load_builtin(b);
            let (editor, _, _, _) = preset_to_state(&p);
            build_shaders(&editor).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
        }
    }

    #[test]
    fn build_shaders_collects_param_errors_with_file() {
        let editor = EditorState {
            rule: "// @param x: f64 = 1\nfn rule(pos: vec2<u32>) -> vec4<f32> { return vec4<f32>(0.0); }".into(),
            render: "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return cell; }".into(),
            ..Default::default()
        };
        let errs = build_shaders(&editor).unwrap_err();
        assert_eq!(errs[0].file, ShaderFile::Rule);
        assert_eq!(errs[0].line, 1);
    }

    #[test]
    fn build_shaders_reports_both_files() {
        let editor = EditorState {
            rule: "fn rule(pos: vec2<u32>) -> vec4<f32> { return 1.0; }".into(),
            render: "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return 1.0; }".into(),
            ..Default::default()
        };
        let errs = build_shaders(&editor).unwrap_err();
        assert!(errs.iter().any(|e| e.file == ShaderFile::Rule));
        assert!(errs.iter().any(|e| e.file == ShaderFile::Render));
    }

    #[test]
    fn resolve_values_prefers_toml_then_previous_then_default() {
        let specs = parse_params(
            "// @param a: f32 = 1\n// @param b: f32 = 2\n// @param c: f32 = 3\n// @param d: i32 = 4\n",
        )
        .unwrap();
        let mut toml_params = BTreeMap::new();
        toml_params.insert("a".to_string(), toml::Value::Float(10.0));
        toml_params.insert("d".to_string(), toml::Value::Float(1.5)); // wrong type for i32
        toml_params.insert("ghost".to_string(), toml::Value::Float(0.0)); // undeclared
        let mut previous = BTreeMap::new();
        previous.insert("b".to_string(), ParamValue::F32(20.0));
        previous.insert("c".to_string(), ParamValue::I32(7)); // type mismatch
        let v = resolve_values(&specs, &toml_params, &previous);
        assert_eq!(v["a"], ParamValue::F32(10.0));
        assert_eq!(v["b"], ParamValue::F32(20.0));
        assert_eq!(v["c"], ParamValue::F32(3.0));
        assert_eq!(v["d"], ParamValue::I32(4));
        assert!(!v.contains_key("ghost"));
    }

    #[test]
    fn preset_round_trips_through_state() {
        let p = load_builtin(&BUILTINS[2]);
        let (editor, config, spf, toml_params) = preset_to_state(&p);
        let (specs, _, _, _) = build_shaders(&editor).unwrap();
        let values = resolve_values(&specs, &toml_params, &BTreeMap::new());
        let state = AppState::from_parts(
            editor,
            config,
            spf,
            specs,
            values,
            p.meta.name.clone(),
            PresetSource::Builtin(2),
        );
        let back = state_to_preset(&state);
        assert_eq!(back.rule, p.rule);
        assert_eq!(back.render, p.render);
        assert_eq!(back.meta.width, p.meta.width);
        assert_eq!(back.meta.mode, p.meta.mode);
        assert_eq!(back.meta.name, p.meta.name);
        assert!(back.meta.params.contains_key("fade"));
    }

    #[test]
    fn prepare_preset_load_rejects_bad_shader() {
        let mut p = load_builtin(&BUILTINS[2]);
        p.rule = "fn rule(pos: vec2<u32>) -> vec4<f32> { return oops(; }".into();
        let errs = prepare_preset_load(&p).unwrap_err();
        assert_eq!(errs[0].file, ShaderFile::Rule);
        assert_eq!(errs[0].line, 1);
        let good = load_builtin(&BUILTINS[2]);
        let loaded = prepare_preset_load(&good).unwrap();
        assert_eq!(loaded.editor.rule, good.rule);
        assert!(loaded.specs.iter().any(|s| s.name == "fade"));
    }

    #[test]
    fn preset_to_state_clamps_hand_edited_values() {
        let mut p = load_builtin(&BUILTINS[2]);
        p.meta.steps_per_frame = 100_000;
        p.meta.width = 100_000;
        p.meta.height = 0;
        p.meta.init = crate::preset::InitPattern::Random { density: f32::NAN };
        let (_, config, spf, _) = preset_to_state(&p);
        assert_eq!(spf, MAX_STEPS_PER_FRAME);
        assert_eq!(config.width, MAX_GRID_SIZE);
        assert_eq!(config.height, 1);
        match config.init {
            crate::preset::InitPattern::Random { density } => assert!((0.0..=1.0).contains(&density)),
            other => panic!("unexpected init {other:?}"),
        }
    }

    #[test]
    fn diagnostic_text_and_location() {
        let d = Diagnostic::Shader(ShaderError { file: ShaderFile::Render, line: 3, column: 7, message: "boom".into(), hint: None });
        assert_eq!(d.text(), "render.wgsl:3:7  boom");
        assert_eq!(d.location(), Some((ShaderFile::Render, 3)));
        let g = Diagnostic::General("save failed".into());
        assert_eq!(g.text(), "save failed");
        assert_eq!(g.location(), None);
        let list = diagnostics_from(vec![ShaderError { file: ShaderFile::Rule, line: 1, column: 1, message: "x".into(), hint: None }]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].location(), Some((ShaderFile::Rule, 1)));
    }

    #[test]
    fn editor_dirty_flags_are_per_file() {
        let mut e = EditorState::default();
        assert!(!e.any_dirty());
        e.mark_dirty(ShaderFile::Render);
        assert!(!e.is_dirty(ShaderFile::Rule));
        assert!(e.is_dirty(ShaderFile::Render));
        assert!(e.any_dirty());
        e.clear_dirty();
        assert!(!e.any_dirty());
    }

    #[test]
    fn rate_meter_measures_steps_per_second_and_survives_resets() {
        use std::time::Duration;
        let t0 = Instant::now();
        let mut m = RateMeter::new(t0, 0);
        // Too soon: keeps the previous (zero) estimate.
        assert_eq!(m.update(t0 + Duration::from_millis(100), 10), 0.0);
        // After the sample window: 120 steps in 0.6 s = 200/s.
        let r = m.update(t0 + Duration::from_millis(600), 120);
        assert!((r - 200.0).abs() < 1e-3, "rate {r}");
        // A reset drops the frame counter: no negative rates, estimate is reset.
        let r = m.update(t0 + Duration::from_millis(1300), 5);
        assert_eq!(r, 0.0);
        let r = m.update(t0 + Duration::from_millis(1800), 55);
        assert!((r - 100.0).abs() < 1e-3, "rate {r}");
    }

    #[test]
    fn build_shaders_merges_params_from_all_three_editors_and_validates_post() {
        let mut editor = preset_to_state(&load_builtin(&BUILTINS[2])).0;
        editor.post = "// @param strength: f32 = 0.5 range 0.0 .. 1.0\nfn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color * params.strength; }\n".into();
        let (specs, _, _, post) = build_shaders(&editor).unwrap();
        assert!(specs.iter().any(|s| s.name == "strength"));
        assert!(specs.iter().any(|s| s.name == "fade"));
        assert!(post.source.contains("params.strength"));
        editor.post = "fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return 1.0; }\n".into();
        let errs = build_shaders(&editor).unwrap_err();
        assert!(errs.iter().any(|e| e.file == ShaderFile::Post));
    }

    #[test]
    fn editor_dirty_flags_cover_the_post_editor() {
        let mut e = EditorState::default();
        e.mark_dirty(ShaderFile::Post);
        assert!(e.is_dirty(ShaderFile::Post));
        assert!(!e.is_dirty(ShaderFile::Rule));
        e.clear_dirty();
        assert!(!e.any_dirty());
    }

    #[test]
    fn build_shaders_uses_the_pair_when_rule_b_is_present() {
        let mut editor = preset_to_state(&load_builtin(&BUILTINS[2])).0;
        assert!(editor.rule_b.is_empty());
        let (_, rule, _, _) = build_shaders(&editor).unwrap();
        assert!(!rule.source.contains("fn rule_b("));
        editor.rule_b = "fn rule(pos: vec2<u32>) -> vec4<f32> { return on_if(!alive(i32(pos.x), i32(pos.y)) && neighbours(i32(pos.x), i32(pos.y)) == 2u); }\n".into();
        let (_, rule, _, _) = build_shaders(&editor).unwrap();
        assert!(rule.source.contains("fn rule_b("));
        editor.rule_b = "fn rule(pos: vec2<u32>) -> vec4<f32> { return 1.0; }\n".into();
        let errs = build_shaders(&editor).unwrap_err();
        assert!(errs.iter().any(|e| e.file == ShaderFile::RuleB));
        // Rule B's text only reaches the preset when it is non-empty.
        editor.rule_b = "   \n".into();
        assert_eq!(editor.rule_b_for_preset(), None);
    }
}

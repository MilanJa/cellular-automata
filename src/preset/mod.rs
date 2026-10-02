//! A preset is a folder with `preset.toml`, `rule.wgsl` and `render.wgsl`.

pub mod builtin;
pub mod bundle;
pub mod share;

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::shader::params::{ParamType, ParamValue};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    #[serde(rename = "2d")]
    TwoD,
    #[serde(rename = "1d")]
    OneD,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InitPattern {
    Random { density: f32 },
    Single,
    Blank,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PresetMeta {
    pub name: String,
    pub mode: Mode,
    pub width: u32,
    pub height: u32,
    pub steps_per_frame: u32,
    pub seed: u32,
    pub init: InitPattern,
    /// Crossfade position between rule A and rule B (0..1); only meaningful with a `rule_b.wgsl`.
    #[serde(default)]
    pub blend: f32,
    #[serde(default)]
    pub params: BTreeMap<String, toml::Value>,
    /// Time-driven modulation per param name (see `app::modulation`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, crate::app::modulation::Modulation>,
    /// MIDI controller knob per param name (see `midi::mapping`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub midi: BTreeMap<String, crate::midi::mapping::CcKey>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Preset {
    pub meta: PresetMeta,
    pub rule: String,
    pub render: String,
    /// Optional post-processing shader (`post.wgsl`); `None` means pass-through.
    pub post: Option<String>,
    /// Optional second rule (`rule_b.wgsl`) crossfaded with the first by `meta.blend`.
    pub rule_b: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Preset {
    pub fn load_dir(dir: &Path) -> anyhow::Result<Preset> {
        let meta_path = dir.join("preset.toml");
        let meta_text = std::fs::read_to_string(&meta_path)
            .with_context(|| format!("reading {}", meta_path.display()))?;
        let meta: PresetMeta = toml::from_str(&meta_text).context("parsing preset.toml")?;
        let rule = std::fs::read_to_string(dir.join("rule.wgsl")).context("reading rule.wgsl")?;
        let render =
            std::fs::read_to_string(dir.join("render.wgsl")).context("reading render.wgsl")?;
        let post = std::fs::read_to_string(dir.join("post.wgsl")).ok();
        let rule_b = std::fs::read_to_string(dir.join("rule_b.wgsl")).ok();
        Ok(Preset { meta, rule, render, post, rule_b })
    }

    pub fn save_dir(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let meta_text = toml::to_string(&self.meta).context("serialising preset.toml")?;
        std::fs::write(dir.join("preset.toml"), meta_text)?;
        std::fs::write(dir.join("rule.wgsl"), &self.rule)?;
        std::fs::write(dir.join("render.wgsl"), &self.render)?;
        match &self.post {
            Some(post) => std::fs::write(dir.join("post.wgsl"), post)?,
            None => {
                let _ = std::fs::remove_file(dir.join("post.wgsl"));
            }
        }
        match &self.rule_b {
            Some(b) => std::fs::write(dir.join("rule_b.wgsl"), b)?,
            None => {
                let _ = std::fs::remove_file(dir.join("rule_b.wgsl"));
            }
        }
        Ok(())
    }
}

fn toml_f32(v: &toml::Value) -> Option<f32> {
    match v {
        toml::Value::Float(f) => Some(*f as f32),
        toml::Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

fn toml_vec(v: &toml::Value, n: usize) -> Option<Vec<f32>> {
    let arr = v.as_array()?;
    if arr.len() != n {
        return None;
    }
    arr.iter().map(toml_f32).collect()
}

pub fn param_value_from_toml(ty: ParamType, v: &toml::Value) -> Option<ParamValue> {
    Some(match ty {
        ParamType::F32 => ParamValue::F32(toml_f32(v)?),
        ParamType::I32 => ParamValue::I32(v.as_integer()? as i32),
        ParamType::Bool => ParamValue::Bool(v.as_bool()?),
        ParamType::Vec2 => ParamValue::Vec2(toml_vec(v, 2)?.try_into().ok()?),
        ParamType::Vec3 => ParamValue::Vec3(toml_vec(v, 3)?.try_into().ok()?),
        ParamType::Vec4 => ParamValue::Vec4(toml_vec(v, 4)?.try_into().ok()?),
    })
}

/// Widens an f32 through its shortest round-trip decimal, so `0.1f32` becomes `0.1` in the
/// file rather than `0.10000000149011612`.
fn f32_to_toml_float(x: f32) -> f64 {
    x.to_string().parse::<f64>().unwrap_or(x as f64)
}

pub fn param_value_to_toml(v: &ParamValue) -> toml::Value {
    let arr = |a: &[f32]| {
        toml::Value::Array(a.iter().map(|x| toml::Value::Float(f32_to_toml_float(*x))).collect())
    };
    match v {
        ParamValue::F32(x) => toml::Value::Float(f32_to_toml_float(*x)),
        ParamValue::I32(x) => toml::Value::Integer(*x as i64),
        ParamValue::Bool(b) => toml::Value::Boolean(*b),
        ParamValue::Vec2(a) => arr(a),
        ParamValue::Vec3(a) => arr(a),
        ParamValue::Vec4(a) => arr(a),
    }
}

/// Folder-safe name for a preset: lowercase ASCII letters and digits, runs of anything else
/// collapsed to one `_`. Falls back to `preset` when nothing usable is left.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_sep = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            pending_sep = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_sep = true;
        }
    }
    if out.is_empty() { "preset".to_string() } else { out }
}

/// True when `dir` already holds a preset.
#[cfg(not(target_arch = "wasm32"))]
pub fn preset_exists(dir: &Path) -> bool {
    dir.join("preset.toml").is_file()
}

/// Lists `(name, folder)` for every loadable preset folder directly under `dir`, sorted by name.
/// Folders named after a built-in id are skipped: those are the embedded sources and already
/// appear in the built-in list.
#[cfg(not(target_arch = "wasm32"))]
pub fn scan_presets_dir(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let is_builtin_folder = |e: &std::fs::DirEntry| {
        let name = e.file_name();
        builtin::BUILTINS.iter().any(|b| name == b.id)
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_dir() && !is_builtin_folder(e))
        .filter_map(|e| Preset::load_dir(&e.path()).ok().map(|p| (p.meta.name, e.path())))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::params::{ParamType, ParamValue};

    fn sample() -> Preset {
        let mut params = BTreeMap::new();
        params.insert("threshold".into(), toml::Value::Float(0.5));
        params.insert(
            "col".into(),
            toml::Value::Array(vec![
                toml::Value::Float(1.0),
                toml::Value::Float(0.5),
                toml::Value::Float(0.2),
            ]),
        );
        Preset {
            meta: PresetMeta {
                name: "Test".into(),
                mode: Mode::TwoD,
                width: 64,
                height: 32,
                steps_per_frame: 2,
                seed: 7,
                init: InitPattern::Random { density: 0.3 },
                blend: 0.0,
                params,
                modulation: BTreeMap::new(),
                midi: BTreeMap::new(),
            },
            rule: "fn rule() {}\n".into(),
            render: "fn shade() {}\n".into(),
            post: None,
            rule_b: None,
        }
    }

    #[test]
    fn toml_round_trip() {
        let p = sample();
        let text = toml::to_string(&p.meta).unwrap();
        assert!(text.contains("mode = \"2d\""));
        assert!(text.contains("kind = \"random\""));
        let back: PresetMeta = toml::from_str(&text).unwrap();
        assert_eq!(back, p.meta);
    }

    #[test]
    fn one_d_and_single_init_serialize() {
        let meta = PresetMeta { mode: Mode::OneD, init: InitPattern::Single, ..sample().meta };
        let text = toml::to_string(&meta).unwrap();
        assert!(text.contains("mode = \"1d\""));
        assert!(text.contains("kind = \"single\""));
        let back: PresetMeta = toml::from_str(&text).unwrap();
        assert_eq!(back.init, InitPattern::Single);
    }

    #[test]
    fn save_and_load_dir() {
        let dir = tempfile::tempdir().unwrap();
        let p = sample();
        p.save_dir(dir.path()).unwrap();
        assert!(dir.path().join("preset.toml").exists());
        assert!(dir.path().join("rule.wgsl").exists());
        assert!(dir.path().join("render.wgsl").exists());
        let back = Preset::load_dir(dir.path()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn load_missing_dir_errors() {
        assert!(Preset::load_dir(Path::new("does/not/exist")).is_err());
    }

    #[test]
    fn params_missing_in_toml_default_to_empty() {
        let text = "name = \"x\"\nmode = \"2d\"\nwidth = 8\nheight = 8\nsteps_per_frame = 1\nseed = 1\n[init]\nkind = \"blank\"\n";
        let meta: PresetMeta = toml::from_str(text).unwrap();
        assert!(meta.params.is_empty());
    }

    #[test]
    fn param_value_conversions() {
        assert_eq!(
            param_value_from_toml(ParamType::F32, &toml::Value::Float(0.5)),
            Some(ParamValue::F32(0.5))
        );
        assert_eq!(
            param_value_from_toml(ParamType::F32, &toml::Value::Integer(2)),
            Some(ParamValue::F32(2.0))
        );
        assert_eq!(
            param_value_from_toml(ParamType::I32, &toml::Value::Integer(3)),
            Some(ParamValue::I32(3))
        );
        assert_eq!(
            param_value_from_toml(ParamType::Bool, &toml::Value::Boolean(true)),
            Some(ParamValue::Bool(true))
        );
        assert_eq!(
            param_value_from_toml(
                ParamType::Vec3,
                &toml::Value::Array(vec![
                    toml::Value::Float(1.0),
                    toml::Value::Integer(0),
                    toml::Value::Float(0.5)
                ])
            ),
            Some(ParamValue::Vec3([1.0, 0.0, 0.5]))
        );
        assert_eq!(
            param_value_from_toml(ParamType::Vec3, &toml::Value::Array(vec![toml::Value::Float(1.0)])),
            None
        );
        assert_eq!(param_value_from_toml(ParamType::I32, &toml::Value::String("x".into())), None);
        let back = param_value_to_toml(&ParamValue::Vec2([1.0, 2.0]));
        assert_eq!(param_value_from_toml(ParamType::Vec2, &back), Some(ParamValue::Vec2([1.0, 2.0])));
    }

    #[test]
    fn scan_lists_valid_folders_sorted_and_skips_bad_ones() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = sample();
        b.meta.name = "Bravo".into();
        let mut a = sample();
        a.meta.name = "Alpha".into();
        b.save_dir(&dir.path().join("b")).unwrap();
        a.save_dir(&dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("junk")).unwrap();
        let list = scan_presets_dir(dir.path());
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Bravo"]);
        assert!(scan_presets_dir(Path::new("nope")).is_empty());
    }

    #[test]
    fn slug_makes_safe_folder_names() {
        assert_eq!(slug("Game of Life"), "game_of_life");
        assert_eq!(slug("Gray-Scott Reaction-Diffusion"), "gray_scott_reaction_diffusion");
        assert_eq!(slug("  Rule 30!! "), "rule_30");
        assert_eq!(slug("___"), "preset");
        assert_eq!(slug(""), "preset");
        assert_eq!(slug("caf\u{e9} ☕"), "caf");
    }

    #[test]
    fn preset_exists_checks_for_preset_toml() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!preset_exists(dir.path()));
        sample().save_dir(dir.path()).unwrap();
        assert!(preset_exists(dir.path()));
    }

    #[test]
    fn scan_skips_folders_named_after_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = sample();
        a.meta.name = "Shadow".into();
        a.save_dir(&dir.path().join("life")).unwrap();
        let mut b = sample();
        b.meta.name = "Mine".into();
        b.save_dir(&dir.path().join("mine")).unwrap();
        let names: Vec<String> = scan_presets_dir(dir.path()).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["Mine".to_string()]);
    }

    #[test]
    fn f32_params_serialise_as_short_decimals() {
        let v = param_value_to_toml(&ParamValue::F32(0.1));
        assert_eq!(v, toml::Value::Float(0.1));
        let v = param_value_to_toml(&ParamValue::Vec3([0.3, 1.0, 0.037]));
        assert_eq!(v, toml::Value::Array(vec![toml::Value::Float(0.3), toml::Value::Float(1.0), toml::Value::Float(0.037)]));
        let text = toml::to_string(&toml::Table::from_iter([("fade".to_string(), param_value_to_toml(&ParamValue::F32(40.0)))])).unwrap();
        assert_eq!(text.trim(), "fade = 40.0");
    }

    #[test]
    fn modulation_table_round_trips_and_defaults_to_empty() {
        use crate::app::modulation::{Modulation, Wave};
        let mut p = sample();
        p.meta.modulation.insert(
            "threshold".into(),
            Modulation { wave: Wave::Sine, freq: 0.25, amount: 0.5, phase: 0.0, follow_sim: false },
        );
        let text = toml::to_string(&p.meta).unwrap();
        assert!(text.contains("[modulation.threshold]"), "{text}");
        let back: PresetMeta = toml::from_str(&text).unwrap();
        assert_eq!(back, p.meta);
        let plain: PresetMeta = toml::from_str(&toml::to_string(&sample().meta).unwrap()).unwrap();
        assert!(plain.modulation.is_empty());
    }

    #[test]
    fn midi_table_round_trips_and_defaults_to_empty() {
        use crate::midi::mapping::CcKey;
        let mut p = sample();
        p.meta.midi.insert("threshold".into(), CcKey { channel: 1, cc: 74 });
        let text = toml::to_string(&p.meta).unwrap();
        assert!(text.contains("[midi.threshold]"), "{text}");
        let back: PresetMeta = toml::from_str(&text).unwrap();
        assert_eq!(back, p.meta);
        let plain: PresetMeta = toml::from_str(&toml::to_string(&sample().meta).unwrap()).unwrap();
        assert!(plain.midi.is_empty());
    }

    #[test]
    fn optional_post_shader_is_saved_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample();
        p.save_dir(dir.path()).unwrap();
        assert!(!dir.path().join("post.wgsl").exists(), "no post file when there is no post shader");
        assert_eq!(Preset::load_dir(dir.path()).unwrap().post, None);
        p.post = Some("fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color * 2.0; }\n".into());
        p.save_dir(dir.path()).unwrap();
        assert!(dir.path().join("post.wgsl").exists());
        assert_eq!(Preset::load_dir(dir.path()).unwrap(), p);
    }

    #[test]
    fn optional_rule_b_and_blend_are_saved_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample();
        p.save_dir(dir.path()).unwrap();
        assert!(!dir.path().join("rule_b.wgsl").exists());
        p.rule_b = Some("fn rule(pos: vec2<u32>) -> vec4<f32> { return off(); }\n".into());
        p.meta.blend = 0.4;
        p.save_dir(dir.path()).unwrap();
        assert!(dir.path().join("rule_b.wgsl").exists());
        let back = Preset::load_dir(dir.path()).unwrap();
        assert_eq!(back, p);
        let text = toml::to_string(&p.meta).unwrap();
        assert!(text.contains("blend = 0.4"), "{text}");
        let old: PresetMeta = toml::from_str("name = \"x\"\nmode = \"2d\"\nwidth = 8\nheight = 8\nsteps_per_frame = 1\nseed = 1\n[init]\nkind = \"blank\"\n").unwrap();
        assert_eq!(old.blend, 0.0);
    }
}

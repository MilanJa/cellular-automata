//! A preset is a folder with `preset.toml`, `rule.wgsl` and `render.wgsl`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    pub params: BTreeMap<String, toml::Value>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Preset {
    pub meta: PresetMeta,
    pub rule: String,
    pub render: String,
}

impl Preset {
    pub fn load_dir(dir: &Path) -> anyhow::Result<Preset> {
        let meta_path = dir.join("preset.toml");
        let meta_text = std::fs::read_to_string(&meta_path)
            .with_context(|| format!("reading {}", meta_path.display()))?;
        let meta: PresetMeta = toml::from_str(&meta_text).context("parsing preset.toml")?;
        let rule = std::fs::read_to_string(dir.join("rule.wgsl")).context("reading rule.wgsl")?;
        let render =
            std::fs::read_to_string(dir.join("render.wgsl")).context("reading render.wgsl")?;
        Ok(Preset { meta, rule, render })
    }

    pub fn save_dir(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let meta_text = toml::to_string(&self.meta).context("serialising preset.toml")?;
        std::fs::write(dir.join("preset.toml"), meta_text)?;
        std::fs::write(dir.join("rule.wgsl"), &self.rule)?;
        std::fs::write(dir.join("render.wgsl"), &self.render)?;
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

pub fn param_value_to_toml(v: &ParamValue) -> toml::Value {
    let arr =
        |a: &[f32]| toml::Value::Array(a.iter().map(|x| toml::Value::Float(*x as f64)).collect());
    match v {
        ParamValue::F32(x) => toml::Value::Float(*x as f64),
        ParamValue::I32(x) => toml::Value::Integer(*x as i64),
        ParamValue::Bool(b) => toml::Value::Boolean(*b),
        ParamValue::Vec2(a) => arr(a),
        ParamValue::Vec3(a) => arr(a),
        ParamValue::Vec4(a) => arr(a),
    }
}

/// Lists `(name, folder)` for every loadable preset folder directly under `dir`, sorted by name.
pub fn scan_presets_dir(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
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
                params,
            },
            rule: "fn rule() {}\n".into(),
            render: "fn shade() {}\n".into(),
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
}

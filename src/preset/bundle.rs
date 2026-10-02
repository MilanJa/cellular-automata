//! A single-file preset format (`*.capreset.toml`) used for browser storage and export/import.
//! It is the folder layout flattened into one TOML document: `[meta]` plus the two shaders.

use serde::{Deserialize, Serialize};

use super::{slug, Preset, PresetMeta};

#[derive(Serialize, Deserialize)]
struct Bundle {
    meta: PresetMeta,
    rule: String,
    render: String,
}

pub fn to_bundle(preset: &Preset) -> anyhow::Result<String> {
    let b = Bundle { meta: preset.meta.clone(), rule: preset.rule.clone(), render: preset.render.clone() };
    Ok(toml::to_string(&b)?)
}

pub fn from_bundle(text: &str) -> anyhow::Result<Preset> {
    let b: Bundle = toml::from_str(text)?;
    Ok(Preset { meta: b.meta, rule: b.rule, render: b.render })
}

pub const BUNDLE_SUFFIX: &str = ".capreset.toml";

pub fn bundle_filename(name: &str) -> String {
    format!("{}{BUNDLE_SUFFIX}", slug(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::builtin::{load_builtin, BUILTINS};

    #[test]
    fn bundle_round_trips_a_preset_with_shaders_and_params() {
        let mut p = load_builtin(&BUILTINS[2]);
        p.meta.params.insert("fade".into(), toml::Value::Float(12.5));
        let text = to_bundle(&p).unwrap();
        assert!(text.contains("[meta]"), "{text}");
        assert!(text.contains("rule = "), "{text}");
        let back = from_bundle(&text).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn bundle_rejects_garbage_with_an_error() {
        assert!(from_bundle("not = [toml").is_err());
        assert!(from_bundle("[meta]\nname = \"x\"\n").is_err(), "missing fields must fail");
    }

    #[test]
    fn bundle_filename_is_slug_based() {
        assert_eq!(bundle_filename("Game of Life"), "game_of_life.capreset.toml");
    }
}

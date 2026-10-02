//! A single-file preset format (`*.capreset.toml`) used for browser storage, export/import and
//! share links. It is the folder layout flattened into one TOML document: `[meta]` plus the
//! shaders, and an optional nested `[layer_b]` bundle.

use serde::{Deserialize, Serialize};

use super::{Preset, PresetMeta, slug};

#[derive(Serialize, Deserialize)]
struct Bundle {
    meta: PresetMeta,
    rule: String,
    render: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    post: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rule_b: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layer_b: Option<Box<Bundle>>,
}

impl Bundle {
    fn from_preset(preset: &Preset) -> Bundle {
        Bundle {
            meta: preset.meta.clone(),
            rule: preset.rule.clone(),
            render: preset.render.clone(),
            post: preset.post.clone(),
            rule_b: preset.rule_b.clone(),
            layer_b: preset.layer_b.as_deref().map(|b| Box::new(Bundle::from_preset(b))),
        }
    }

    fn into_preset(self) -> Preset {
        Preset {
            meta: self.meta,
            rule: self.rule,
            render: self.render,
            post: self.post,
            rule_b: self.rule_b,
            layer_b: self.layer_b.map(|b| Box::new(b.into_preset())),
        }
    }
}

pub fn to_bundle(preset: &Preset) -> anyhow::Result<String> {
    Ok(toml::to_string(&Bundle::from_preset(preset))?)
}

pub fn from_bundle(text: &str) -> anyhow::Result<Preset> {
    let b: Bundle = toml::from_str(text)?;
    Ok(b.into_preset())
}

pub const BUNDLE_SUFFIX: &str = ".capreset.toml";

pub fn bundle_filename(name: &str) -> String {
    format!("{}{BUNDLE_SUFFIX}", slug(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::builtin::{BUILTINS, load_builtin};

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

    #[test]
    fn bundle_carries_the_optional_post_shader() {
        let mut p = load_builtin(&BUILTINS[2]);
        assert_eq!(from_bundle(&to_bundle(&p).unwrap()).unwrap().post, None);
        p.post = Some("fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color; }\n".into());
        let back = from_bundle(&to_bundle(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn bundle_carries_rule_b() {
        let mut p = load_builtin(&BUILTINS[2]);
        p.rule_b = Some("fn rule(pos: vec2<u32>) -> vec4<f32> { return off(); }\n".into());
        p.meta.blend = 0.7;
        assert_eq!(from_bundle(&to_bundle(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn bundle_carries_layer_b() {
        let mut p = load_builtin(&BUILTINS[2]);
        let text = to_bundle(&p).unwrap();
        assert!(!text.contains("[layer_b"), "no layer B table without a layer B: {text}");
        p.layer_b = Some(Box::new(load_builtin(&BUILTINS[4])));
        let text = to_bundle(&p).unwrap();
        assert!(text.contains("[layer_b.meta]"), "{text}");
        assert_eq!(from_bundle(&text).unwrap(), p);
    }
}

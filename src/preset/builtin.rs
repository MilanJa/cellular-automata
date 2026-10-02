//! Presets and templates embedded in the binary from the `presets/` folder.

use super::{Preset, PresetMeta};

pub struct Builtin {
    pub id: &'static str,
    pub meta_toml: &'static str,
    pub rule: &'static str,
    pub render: &'static str,
}

macro_rules! embedded {
    ($id:literal, $dir:literal) => {
        Builtin {
            id: $id,
            meta_toml: include_str!(concat!("../../presets/", $dir, "/preset.toml")),
            rule: include_str!(concat!("../../presets/", $dir, "/rule.wgsl")),
            render: include_str!(concat!("../../presets/", $dir, "/render.wgsl")),
        }
    };
}

/// Ready-to-run examples, listed first in the preset dropdown.
pub const BUILTINS: &[Builtin] = &[
    embedded!("rule30", "rule30"),
    embedded!("rule110", "rule110"),
    embedded!("life", "life"),
    embedded!("lifelike", "lifelike"),
    embedded!("gray_scott", "gray_scott"),
];

/// Commented starting points for new work, offered by the "New" menu.
pub const TEMPLATES: &[Builtin] = &[
    embedded!("binary2d", "templates/binary2d"),
    embedded!("lifelike", "templates/lifelike"),
    embedded!("elementary1d", "templates/elementary1d"),
    embedded!("continuous2d", "templates/continuous2d"),
    embedded!("render_only", "templates/render_only"),
];

pub fn load_builtin(b: &Builtin) -> Preset {
    let meta: PresetMeta = toml::from_str(b.meta_toml)
        .unwrap_or_else(|e| panic!("embedded preset {} has invalid preset.toml: {e}", b.id));
    Preset { meta, rule: b.rule.to_string(), render: b.render.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::assemble::{assemble_render, assemble_rule};
    use crate::shader::params::{merge_params, params_wgsl, parse_params};
    use crate::shader::validate::{validate, ShaderFile};

    fn check_all(list: &[Builtin], what: &str) {
        for b in list {
            let preset = load_builtin(b);
            let rule_params = parse_params(&preset.rule)
                .unwrap_or_else(|e| panic!("{what} {}: rule params: {:?}", b.id, e));
            let render_params = parse_params(&preset.render)
                .unwrap_or_else(|e| panic!("{what} {}: render params: {:?}", b.id, e));
            let specs = merge_params(rule_params, render_params)
                .unwrap_or_else(|e| panic!("{what} {}: merge: {:?}", b.id, e));
            let pw = params_wgsl(&specs);
            let rule = assemble_rule(&preset.rule, &pw);
            let render = assemble_render(&preset.render, &pw);
            if let Err(e) = validate(ShaderFile::Rule, &rule) {
                panic!("{what} {}: rule: {:?}\n{}", b.id, e, rule.source);
            }
            if let Err(e) = validate(ShaderFile::Render, &render) {
                panic!("{what} {}: render: {:?}\n{}", b.id, e, render.source);
            }
            assert!(preset.meta.width > 0 && preset.meta.height > 0);
        }
    }

    #[test]
    fn there_are_five_builtins_with_unique_ids() {
        assert_eq!(BUILTINS.len(), 5);
        let mut ids: Vec<_> = BUILTINS.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn there_are_five_templates_with_unique_ids() {
        assert_eq!(TEMPLATES.len(), 5);
        let mut ids: Vec<_> = TEMPLATES.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn every_builtin_parses_and_validates() {
        check_all(BUILTINS, "builtin");
    }

    #[test]
    fn every_template_parses_and_validates() {
        check_all(TEMPLATES, "template");
    }

    #[test]
    fn toml_override_from_a_builtin_is_applied_and_unknown_keys_are_ignored() {
        use crate::shader::params::ParamValue;
        // Rule 110 overrides `rule_number` in its preset.toml; add an undeclared key too.
        let mut preset = load_builtin(&BUILTINS[1]);
        preset.meta.params.insert("ghost".into(), toml::Value::Float(1.0));
        let specs = parse_params(&preset.rule).unwrap();
        let values = crate::app::state::resolve_values(&specs, &preset.meta.params, &Default::default());
        assert_eq!(values["rule_number"], ParamValue::I32(110));
        assert!(!values.contains_key("ghost"));
    }
}

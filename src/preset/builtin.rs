//! Presets embedded in the binary from the `presets/` folder.

use super::{Preset, PresetMeta};

pub struct Builtin {
    pub id: &'static str,
    pub meta_toml: &'static str,
    pub rule: &'static str,
    pub render: &'static str,
}

macro_rules! builtin {
    ($id:literal) => {
        Builtin {
            id: $id,
            meta_toml: include_str!(concat!("../../presets/", $id, "/preset.toml")),
            rule: include_str!(concat!("../../presets/", $id, "/rule.wgsl")),
            render: include_str!(concat!("../../presets/", $id, "/render.wgsl")),
        }
    };
}

pub const BUILTINS: &[Builtin] = &[
    builtin!("rule30"),
    builtin!("rule110"),
    builtin!("life"),
    builtin!("gray_scott"),
];

pub fn load_builtin(b: &Builtin) -> Preset {
    let meta: PresetMeta = toml::from_str(b.meta_toml)
        .unwrap_or_else(|e| panic!("built-in preset {} has invalid preset.toml: {e}", b.id));
    Preset { meta, rule: b.rule.to_string(), render: b.render.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::assemble::{assemble_render, assemble_rule};
    use crate::shader::params::{merge_params, params_wgsl, parse_params};
    use crate::shader::validate::{validate, ShaderFile};

    #[test]
    fn there_are_four_builtins_with_unique_ids() {
        assert_eq!(BUILTINS.len(), 4);
        let mut ids: Vec<_> = BUILTINS.iter().map(|b| b.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn every_builtin_parses_and_validates() {
        for b in BUILTINS {
            let preset = load_builtin(b);
            let rule_params = parse_params(&preset.rule)
                .unwrap_or_else(|e| panic!("{}: rule params: {:?}", b.id, e));
            let render_params = parse_params(&preset.render)
                .unwrap_or_else(|e| panic!("{}: render params: {:?}", b.id, e));
            let specs = merge_params(rule_params, render_params).unwrap();
            let pw = params_wgsl(&specs);
            let rule = assemble_rule(&preset.rule, &pw);
            let render = assemble_render(&preset.render, &pw);
            if let Err(e) = validate(ShaderFile::Rule, &rule) {
                panic!("{}: rule: {:?}\n{}", b.id, e, rule.source);
            }
            if let Err(e) = validate(ShaderFile::Render, &render) {
                panic!("{}: render: {:?}\n{}", b.id, e, render.source);
            }
            assert!(preset.meta.width > 0 && preset.meta.height > 0);
        }
    }

    #[test]
    fn toml_params_that_shader_does_not_declare_are_ignored() {
        let b = &BUILTINS[0];
        let mut preset = load_builtin(b);
        preset.meta.params.insert("ghost".into(), toml::Value::Float(1.0));
        let specs = parse_params(&preset.rule).unwrap();
        let known: Vec<_> = preset
            .meta
            .params
            .keys()
            .filter(|k| specs.iter().any(|s| &s.name == *k))
            .collect();
        assert!(!known.iter().any(|k| *k == "ghost"));
    }
}

//! Presets and templates embedded in the binary from the `presets/` folder.

use std::sync::OnceLock;

use super::{Preset, PresetMeta};

pub struct Builtin {
    pub id: &'static str,
    pub meta_toml: &'static str,
    pub rule: &'static str,
    pub render: &'static str,
    pub post: Option<&'static str>,
    pub seed: Option<&'static str>,
    /// A second automaton that runs alongside as layer B (the preset folder's `layer_b/`).
    pub layer_b: Option<&'static Builtin>,
    /// Display name, parsed from `meta_toml` on first use so menus do not re-parse TOML per frame.
    name: OnceLock<String>,
}

impl Builtin {
    pub fn name(&self) -> &str {
        self.name.get_or_init(|| load_builtin(self).meta.name)
    }
}

/// `embedded!(id, dir)` embeds the three required files; add `post` and/or `seed` for the
/// optional `post.wgsl` and `seed.wgsl`, and `; layer_b = Some(&OTHER)` for a nested layer B.
macro_rules! embedded {
    ($id:literal, $dir:literal $(, $extra:ident)*) => {
        embedded!($id, $dir $(, $extra)*; layer_b = None)
    };
    ($id:literal, $dir:literal $(, $extra:ident)*; layer_b = $layer_b:expr) => {
        Builtin {
            id: $id,
            meta_toml: include_str!(concat!("../../presets/", $dir, "/preset.toml")),
            rule: include_str!(concat!("../../presets/", $dir, "/rule.wgsl")),
            render: include_str!(concat!("../../presets/", $dir, "/render.wgsl")),
            post: embedded!(@post $dir $(, $extra)*),
            seed: embedded!(@seed $dir $(, $extra)*),
            layer_b: $layer_b,
            name: OnceLock::new(),
        }
    };
    (@post $dir:literal) => { None };
    (@post $dir:literal, post $(, $rest:ident)*) => {
        Some(include_str!(concat!("../../presets/", $dir, "/post.wgsl")))
    };
    (@post $dir:literal, $other:ident $(, $rest:ident)*) => { embedded!(@post $dir $(, $rest)*) };
    (@seed $dir:literal) => { None };
    (@seed $dir:literal, seed $(, $rest:ident)*) => {
        Some(include_str!(concat!("../../presets/", $dir, "/seed.wgsl")))
    };
    (@seed $dir:literal, $other:ident $(, $rest:ident)*) => { embedded!(@seed $dir $(, $rest)*) };
}

/// Ready-to-run examples, listed first in the preset dropdown. A `static` (not a `const`) so
/// the cached names live in one place. New entries go at the end: tests and the default preset
/// refer to the earlier ones by index.
pub static BUILTINS: [Builtin; 13] = [
    embedded!("rule30", "rule30"),
    embedded!("rule110", "rule110"),
    embedded!("life", "life"),
    embedded!("lifelike", "lifelike"),
    embedded!("gray_scott", "gray_scott"),
    embedded!("neon_life", "neon_life", post),
    embedded!("hex_life", "hex_life"),
    embedded!("glider_gun", "glider_gun", seed),
    embedded!("acorn", "acorn", seed),
    embedded!("gray_scott_discs", "gray_scott_discs", seed),
    embedded!("restless_life", "restless_life"),
    embedded!("slither", "slither", seed; layer_b = Some(&SLITHER_SCENT)),
    embedded!("slither_garden", "slither_garden", seed; layer_b = Some(&SLITHER_GARDEN_B)),
];

/// Slither's layer B: the scent field its snakes hunt by.
static SLITHER_SCENT: Builtin = embedded!("slither_scent", "slither/layer_b");

/// Slither Garden's layer B: the scent plus the Life-like garden sown in the snakes' wake.
static SLITHER_GARDEN_B: Builtin = embedded!("slither_garden_b", "slither_garden/layer_b");

/// Commented starting points for new work, offered by the "New" menu.
pub static TEMPLATES: [Builtin; 9] = [
    embedded!("binary2d", "templates/binary2d"),
    embedded!("lifelike", "templates/lifelike"),
    embedded!("elementary1d", "templates/elementary1d"),
    embedded!("continuous2d", "templates/continuous2d"),
    embedded!("render_only", "templates/render_only"),
    embedded!("neon_life", "neon_life", post),
    embedded!("post_effects", "templates/post_effects", post),
    embedded!("tri_life", "templates/tri_life"),
    embedded!("layer_driven", "templates/layer_driven"),
];

/// The worked examples of `docs/tutorial/`, one per chapter that ends in a runnable preset,
/// offered by the "New" menu under "Tutorial". Ids are `tutNN_<topic>`; `docs/tutorial/images`
/// is rendered from them by `cargo run --example render_docs`.
pub static TUTORIAL: [Builtin; 6] = [
    embedded!("tut03_majority", "tutorial/03_majority"),
    embedded!("tut04_life", "tutorial/04_life"),
    embedded!("tut05_colour", "tutorial/05_colour"),
    embedded!("tut06_seeds", "tutorial/06_seeds", seed),
    embedded!("tut07_gray_scott", "tutorial/07_gray_scott", seed),
    embedded!("tut08_elementary", "tutorial/08_elementary"),
];

pub fn load_builtin(b: &Builtin) -> Preset {
    let meta: PresetMeta =
        toml::from_str(b.meta_toml).unwrap_or_else(|e| panic!("embedded preset {} has invalid preset.toml: {e}", b.id));
    Preset {
        meta,
        rule: b.rule.to_string(),
        render: b.render.to_string(),
        post: b.post.map(str::to_string),
        rule_b: None,
        seed: b.seed.map(str::to_string),
        layer_b: b.layer_b.map(|lb| Box::new(load_builtin(lb))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::InitPattern;
    use crate::shader::assemble::{DEFAULT_POST, assemble_post, assemble_render, assemble_rule, assemble_seed};
    use crate::shader::params::{merge_params, params_wgsl, parse_params};
    use crate::shader::validate::{ShaderFile, validate};

    fn check_all(list: &[Builtin], what: &str) {
        for b in list {
            let preset = load_builtin(b);
            let rule_params =
                parse_params(&preset.rule).unwrap_or_else(|e| panic!("{what} {}: rule params: {:?}", b.id, e));
            let render_params =
                parse_params(&preset.render).unwrap_or_else(|e| panic!("{what} {}: render params: {:?}", b.id, e));
            let post_src = preset.post.clone().unwrap_or_else(|| DEFAULT_POST.to_string());
            let post_params =
                parse_params(&post_src).unwrap_or_else(|e| panic!("{what} {}: post params: {:?}", b.id, e));
            let seed_src = preset.seed.clone().unwrap_or_default();
            let seed_params =
                parse_params(&seed_src).unwrap_or_else(|e| panic!("{what} {}: seed params: {:?}", b.id, e));
            let specs = merge_params(rule_params, render_params)
                .and_then(|s| merge_params(s, post_params))
                .and_then(|s| merge_params(s, seed_params))
                .unwrap_or_else(|e| panic!("{what} {}: merge: {:?}", b.id, e));
            let pw = params_wgsl(&specs);
            let rule = assemble_rule(&preset.rule, &pw);
            let render = assemble_render(&preset.render, &pw);
            let post = assemble_post(&post_src, &pw);
            if let Err(e) = validate(ShaderFile::Rule, &rule) {
                panic!("{what} {}: rule: {:?}\n{}", b.id, e, rule.source);
            }
            if let Err(e) = validate(ShaderFile::Render, &render) {
                panic!("{what} {}: render: {:?}\n{}", b.id, e, render.source);
            }
            if let Err(e) = validate(ShaderFile::Post, &post) {
                panic!("{what} {}: post: {:?}\n{}", b.id, e, post.source);
            }
            if preset.seed.is_some() {
                let seed = assemble_seed(&seed_src, &pw);
                if let Err(e) = validate(ShaderFile::Seed, &seed) {
                    panic!("{what} {}: seed: {:?}\n{}", b.id, e, seed.source);
                }
            }
            assert_eq!(
                preset.meta.init == InitPattern::Code,
                preset.seed.is_some(),
                "{what} {}: a `code` init and a seed.wgsl go together",
                b.id
            );
            assert!(preset.meta.width > 0 && preset.meta.height > 0);
            if let Some(layer_b) = b.layer_b {
                check_all(std::slice::from_ref(layer_b), &format!("{what} {} layer B", b.id));
                assert!(layer_b.layer_b.is_none(), "{what} {}: a layer B nests only one level", b.id);
            }
        }
    }

    #[test]
    fn builtins_have_unique_ids() {
        assert_eq!(BUILTINS.len(), 13);
        let mut ids: Vec<_> = BUILTINS.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 13);
    }

    #[test]
    fn seeded_builtins_ship_a_seed_shader_and_a_code_init() {
        for id in ["glider_gun", "acorn", "gray_scott_discs"] {
            let b = BUILTINS.iter().find(|b| b.id == id).unwrap_or_else(|| panic!("{id} missing"));
            let p = load_builtin(b);
            assert_eq!(p.meta.init, InitPattern::Code, "{id}");
            assert!(p.seed.as_deref().is_some_and(|s| s.contains("fn seed(pos: vec2<u32>) -> vec4<f32>")), "{id}");
        }
        // The seed-side slider of the Gray-Scott discs preset is declared in seed.wgsl only.
        let discs = load_builtin(BUILTINS.iter().find(|b| b.id == "gray_scott_discs").unwrap());
        let specs = parse_params(discs.seed.as_deref().unwrap()).unwrap();
        assert!(specs.iter().any(|s| s.name == "discs"));
    }

    #[test]
    fn templates_have_unique_ids() {
        assert_eq!(TEMPLATES.len(), 9);
        let mut ids: Vec<_> = TEMPLATES.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 9);
    }

    #[test]
    fn every_builtin_parses_and_validates() {
        check_all(&BUILTINS, "builtin");
    }

    #[test]
    fn builtin_names_come_from_their_toml_and_are_cached() {
        let life = BUILTINS.iter().find(|b| b.id == "life").unwrap();
        assert_eq!(life.name(), load_builtin(life).meta.name);
        assert!(std::ptr::eq(life.name(), life.name()), "the same cached string is returned");
    }

    #[test]
    fn every_template_parses_and_validates() {
        check_all(&TEMPLATES, "template");
    }

    #[test]
    fn every_tutorial_preset_parses_and_validates_and_is_numbered_like_its_chapter() {
        check_all(&TUTORIAL, "tutorial");
        let mut ids: Vec<_> = TUTORIAL.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TUTORIAL.len());
        for b in TUTORIAL.iter() {
            let chapter = &b.id[3..5];
            assert!(chapter.chars().all(|c| c.is_ascii_digit()), "{}: id should start with tutNN", b.id);
            assert!(b.name().starts_with(&format!("Tutorial {}: ", chapter.trim_start_matches('0'))), "{}", b.name());
            assert!(!BUILTINS.iter().any(|x| x.id == b.id), "{}: clashes with a built-in id", b.id);
        }
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

    #[test]
    fn neon_life_ships_with_a_glow_modulation_on_a_declared_param() {
        let b = BUILTINS.iter().find(|b| b.id == "neon_life").unwrap();
        let p = load_builtin(b);
        let m = p.meta.modulation.get("glow").expect("glow modulation");
        assert!(m.freq > 0.0 && m.amount > 0.0);
        let specs = parse_params(&p.render).unwrap();
        assert!(specs.iter().any(|s| s.name == "glow"));
    }
}

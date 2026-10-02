//! Validates assembled WGSL with naga and maps error locations back to the user's line numbers.

use super::assemble::Assembled;
use super::hints::hint_for;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderFile {
    Rule,
    RuleB,
    Render,
    Post,
}

impl ShaderFile {
    pub fn label(self) -> &'static str {
        match self {
            ShaderFile::Rule => "rule.wgsl",
            ShaderFile::RuleB => "rule_b.wgsl",
            ShaderFile::Render => "render.wgsl",
            ShaderFile::Post => "post.wgsl",
        }
    }
}

/// WGSL that naga has parsed and validated. `Simulation::set_pipelines` only accepts these,
/// so validation runs exactly once per apply and the GPU never sees unchecked source.
#[derive(Debug, Clone)]
pub struct Validated {
    file: ShaderFile,
    source: String,
}

impl Validated {
    pub fn file(&self) -> ShaderFile {
        self.file
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Validates a rule pair (see `assemble_rule_pair`) and attributes each error to rule A or B
/// with a line number relative to that rule's own text.
pub fn validate_pair(assembled: &Assembled) -> Result<Validated, Vec<ShaderError>> {
    let a_lines = assembled
        .rule_b_line_offset
        .map(|b| b.saturating_sub(assembled.user_line_offset))
        .unwrap_or(assembled.user_line_count);
    validate(ShaderFile::Rule, assembled).map_err(|errs| {
        errs.into_iter()
            .map(|mut e| {
                if e.line > a_lines {
                    e.file = ShaderFile::RuleB;
                    e.line -= a_lines;
                }
                e
            })
            .collect()
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderError {
    pub file: ShaderFile,
    /// 1-based line in the user's source, clamped to `1..=user_line_count`.
    pub line: usize,
    pub column: usize,
    pub message: String,
    /// Plain-language help for common mistakes, when the message is recognised.
    pub hint: Option<String>,
}

fn map_location(
    file: ShaderFile,
    assembled: &Assembled,
    loc: Option<naga::SourceLocation>,
    message: String,
) -> ShaderError {
    let (line, column, in_prelude) = match loc {
        Some(l) => {
            let abs = l.line_number as usize;
            let user_first = assembled.user_line_offset + 1;
            let user_last = assembled.user_line_offset + assembled.user_line_count;
            if abs < user_first {
                (1, 1, true)
            } else if abs > user_last {
                (assembled.user_line_count, 1, false)
            } else {
                (abs - assembled.user_line_offset, l.line_position as usize, false)
            }
        }
        None => (1, 1, false),
    };
    let line_text =
        if in_prelude { "" } else { assembled.source.lines().nth(assembled.user_line_offset + line - 1).unwrap_or("") };
    let hint = hint_for(&message, line_text);
    let message = if in_prelude { format!("{message} (in generated prelude)") } else { message };
    ShaderError { file, line, column, message, hint }
}

fn in_user_range(assembled: &Assembled, loc: &naga::SourceLocation) -> bool {
    let abs = loc.line_number as usize;
    abs > assembled.user_line_offset && abs <= assembled.user_line_offset + assembled.user_line_count
}

/// Picks the most useful location: the first (parse) or last (validation) span that falls in
/// the user's lines, else the last span overall.
fn pick_location(
    assembled: &Assembled,
    locs: impl Iterator<Item = naga::SourceLocation>,
    prefer_last: bool,
) -> Option<naga::SourceLocation> {
    let locs: Vec<_> = locs.collect();
    let mut in_range = locs.iter().filter(|l| in_user_range(assembled, l));
    let chosen = if prefer_last { in_range.next_back() } else { in_range.next() };
    chosen.or(locs.last()).copied()
}

/// Joins an error and its `source()` chain into one line, innermost cause last.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut cur = e.source();
    while let Some(src) = cur {
        parts.push(src.to_string());
        cur = src.source();
    }
    parts.join(": ")
}

pub fn validate(file: ShaderFile, assembled: &Assembled) -> Result<Validated, Vec<ShaderError>> {
    let module = match naga::front::wgsl::parse_str(&assembled.source) {
        Ok(m) => m,
        Err(e) => {
            let loc = pick_location(assembled, e.labels().map(|(span, _)| span.location(&assembled.source)), false)
                .or_else(|| e.location(&assembled.source));
            return Err(vec![map_location(file, assembled, loc, e.message().to_string())]);
        }
    };
    // The device is created with default features, so only the baseline capability set is
    // allowed; anything beyond it (f64, f16, subgroups, ...) is reported here instead of
    // reaching the GPU backend.
    let mut validator =
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty());
    match validator.validate(&module) {
        Ok(_) => Ok(Validated { file, source: assembled.source.clone() }),
        Err(e) => {
            // The innermost span is the most specific (the offending expression), while the
            // outermost usually just points at the enclosing function.
            let loc = pick_location(assembled, e.spans().map(|(span, _)| span.location(&assembled.source)), true)
                .or_else(|| e.location(&assembled.source));
            let message = error_chain(e.as_inner());
            Err(vec![map_location(file, assembled, loc, message)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::assemble::{assemble_render, assemble_rule};
    use crate::shader::params::params_wgsl;

    const OK_RULE: &str = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return cell(i32(pos.x), i32(pos.y));\n}\n";
    const OK_RENDER: &str =
        "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {\n    return vec4<f32>(cell.rgb, 1.0);\n}\n";

    #[test]
    fn valid_rule_and_render_pass() {
        assert!(validate(ShaderFile::Rule, &assemble_rule(OK_RULE, &params_wgsl(&[]))).is_ok());
        assert!(validate(ShaderFile::Render, &assemble_render(OK_RENDER, &params_wgsl(&[]))).is_ok());
    }

    #[test]
    fn syntax_error_maps_to_user_line() {
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let x = ;\n    return vec4<f32>(0.0);\n}\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert_eq!(errs[0].line, 2);
        assert_eq!(errs[0].file, ShaderFile::Rule);
    }

    #[test]
    fn validation_error_maps_to_user_line() {
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return vec4<f32>(0.0);\n}\nfn other() -> f32 { return vec2<f32>(0.0); }\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert!(errs[0].line >= 1 && errs[0].line <= 4, "line was {}", errs[0].line);
    }

    #[test]
    fn missing_rule_function_is_reported_in_user_range() {
        let user = "fn not_rule() -> f32 { return 1.0; }\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert_eq!(errs[0].line, 1);
        assert!(errs[0].message.contains("rule"));
    }

    #[test]
    fn eof_error_clamps_to_last_user_line() {
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return vec4<f32>(0.0);\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert_eq!(errs[0].line, 2, "{:?}", errs[0]);
    }

    #[test]
    fn features_the_device_lacks_are_rejected_by_validation() {
        // f64 needs the FLOAT64 capability, which the default device does not have.
        let user =
            "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let d: f64 = 1.0lf;\n    return vec4<f32>(f32(d));\n}\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert_eq!(errs[0].line, 2, "{:?}", errs[0]);
    }

    #[test]
    fn return_type_mismatch_points_at_the_user_return_line() {
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return 1.0;\n}\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert_eq!(errs[0].line, 2, "{:?}", errs[0]);
        assert!(!errs[0].message.contains("generated prelude"), "{:?}", errs[0]);
    }

    #[test]
    fn hints_are_attached_to_errors() {
        let user =
            "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let x: f32 = pos.x * 2.0;\n    return vec4<f32>(x);\n}\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        let hint = errs[0].hint.as_deref().expect("hint for numeric type mix");
        assert!(hint.contains("f32("), "{hint}");
    }

    #[test]
    fn post_shader_validates_and_reports_its_own_file() {
        use crate::shader::assemble::{DEFAULT_POST, assemble_post};
        assert!(validate(ShaderFile::Post, &assemble_post(DEFAULT_POST, &params_wgsl(&[]))).is_ok());
        let bloom = "fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    let px = 1.0 / vec2<f32>(globals.size);\n    var acc = vec4<f32>(0.0);\n    for (var i = -2; i <= 2; i++) { acc += scene(uv + vec2<f32>(f32(i), 0.0) * px * 2.0); }\n    return color + acc * 0.1 + prev(uv) * 0.3;\n}\n";
        assert!(validate(ShaderFile::Post, &assemble_post(bloom, &params_wgsl(&[]))).is_ok());
        let bad = "fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    return oops(;\n}\n";
        let errs = validate(ShaderFile::Post, &assemble_post(bad, &params_wgsl(&[]))).unwrap_err();
        assert_eq!((errs[0].file, errs[0].line), (ShaderFile::Post, 2));
        assert_eq!(ShaderFile::Post.label(), "post.wgsl");
    }

    #[test]
    fn rule_pair_validates_and_errors_in_b_are_attributed_to_rule_b() {
        use crate::shader::assemble::assemble_rule_pair;
        let life = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let n = neighbours(i32(pos.x), i32(pos.y));\n    let me = alive(i32(pos.x), i32(pos.y));\n    return on_if((me && (n == 2u || n == 3u)) || (!me && n == 3u));\n}\n";
        let seeds = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return on_if(!alive(i32(pos.x), i32(pos.y)) && neighbours(i32(pos.x), i32(pos.y)) == 2u);\n}\n";
        assert!(validate_pair(&assemble_rule_pair(life, seeds, &params_wgsl(&[]))).is_ok());
        let bad_b = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return oops(;\n}\n";
        let errs = validate_pair(&assemble_rule_pair(life, bad_b, &params_wgsl(&[]))).unwrap_err();
        assert_eq!((errs[0].file, errs[0].line), (ShaderFile::RuleB, 2), "{:?}", errs[0]);
        assert_eq!(ShaderFile::RuleB.label(), "rule_b.wgsl");
        let bad_a = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return oops(;\n}\n";
        let errs = validate_pair(&assemble_rule_pair(bad_a, seeds, &params_wgsl(&[]))).unwrap_err();
        assert_eq!((errs[0].file, errs[0].line), (ShaderFile::Rule, 2), "{:?}", errs[0]);
    }

    #[test]
    fn hex_and_triangle_helpers_are_available_to_rules_and_renders() {
        let rule = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let x = i32(pos.x);\n    let y = i32(pos.y);\n    let h = neighbours_hex(x, y);\n    let t3 = neighbours_tri(x, y);\n    let t12 = neighbours_tri12(x, y);\n    let up = tri_is_up(x, y);\n    return on_if(h == 2u || t3 == 1u || (up && t12 == 4u));\n}\n";
        let a = assemble_rule(rule, &params_wgsl(&[]));
        validate(ShaderFile::Rule, &a).unwrap_or_else(|e| panic!("{e:?}\n{}", a.source));
        let render = "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {\n    let hc = hex_cell(uv);\n    let hl = hex_local(uv);\n    let hd = hex_dist(hl);\n    let tc = tri_cell(uv);\n    let c = cell_at(hc.x, hc.y).r * (1.0 - hd) + cell_at(tc.x, tc.y).g;\n    return gray(c);\n}\n";
        let a = assemble_render(render, &params_wgsl(&[]));
        validate(ShaderFile::Render, &a).unwrap_or_else(|e| panic!("{e:?}\n{}", a.source));
    }

    #[test]
    fn rules_and_renders_can_read_the_other_layer() {
        let rule = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let x = i32(pos.x);\n    let y = i32(pos.y);\n    return on_if(other(x, y).r > 0.5 || other_alive(x + 1, y));\n}\n";
        let a = assemble_rule(rule, &params_wgsl(&[]));
        validate(ShaderFile::Rule, &a).unwrap_or_else(|e| panic!("{e:?}\n{}", a.source));
        let render = "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {\n    let p = vec2<i32>(uv * vec2<f32>(globals.size));\n    return gray(cell.r + other(p.x, p.y).g);\n}\n";
        let a = assemble_render(render, &params_wgsl(&[]));
        validate(ShaderFile::Render, &a).unwrap_or_else(|e| panic!("{e:?}\n{}", a.source));
    }
}

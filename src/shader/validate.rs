//! Validates assembled WGSL with naga and maps error locations back to the user's line numbers.

use super::assemble::Assembled;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderFile {
    Rule,
    Render,
}

impl ShaderFile {
    pub fn label(self) -> &'static str {
        match self {
            ShaderFile::Rule => "rule.wgsl",
            ShaderFile::Render => "render.wgsl",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderError {
    pub file: ShaderFile,
    /// 1-based line in the user's source, clamped to `1..=user_line_count`.
    pub line: usize,
    pub column: usize,
    pub message: String,
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
    let message = if in_prelude { format!("{message} (in generated prelude)") } else { message };
    ShaderError { file, line, column, message }
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

pub fn validate(file: ShaderFile, assembled: &Assembled) -> Result<naga::Module, Vec<ShaderError>> {
    let module = match naga::front::wgsl::parse_str(&assembled.source) {
        Ok(m) => m,
        Err(e) => {
            let loc = e.location(&assembled.source);
            return Err(vec![map_location(file, assembled, loc, e.message().to_string())]);
        }
    };
    // The device is created with default features, so only the baseline capability set is
    // allowed; anything beyond it (f64, f16, subgroups, ...) is reported here instead of
    // reaching the GPU backend.
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    );
    match validator.validate(&module) {
        Ok(_) => Ok(module),
        Err(e) => {
            // The innermost span is the most specific (the offending expression), while the
            // outermost usually just points at the enclosing function.
            let loc = e
                .spans()
                .last()
                .map(|(span, _)| span.location(&assembled.source))
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

    const OK_RULE: &str =
        "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return cell(i32(pos.x), i32(pos.y));\n}\n";
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
        assert!(errs[0].line >= 1 && errs[0].line <= 2, "line was {}", errs[0].line);
    }

    #[test]
    fn features_the_device_lacks_are_rejected_by_validation() {
        // f64 needs the FLOAT64 capability, which the default device does not have.
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    let d: f64 = 1.0lf;\n    return vec4<f32>(f32(d));\n}\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert_eq!(errs[0].line, 2, "{:?}", errs[0]);
    }
}

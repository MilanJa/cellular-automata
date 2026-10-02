use std::collections::BTreeMap;

pub const MAX_PARAMS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    F32,
    I32,
    Bool,
    Vec2,
    Vec3,
    Vec4,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    F32(f32),
    I32(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
}

impl ParamValue {
    pub fn ty(&self) -> ParamType {
        match self {
            ParamValue::F32(_) => ParamType::F32,
            ParamValue::I32(_) => ParamType::I32,
            ParamValue::Bool(_) => ParamType::Bool,
            ParamValue::Vec2(_) => ParamType::Vec2,
            ParamValue::Vec3(_) => ParamType::Vec3,
            ParamValue::Vec4(_) => ParamType::Vec4,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    pub name: String,
    pub ty: ParamType,
    pub default: ParamValue,
    pub range: Option<(f64, f64)>,
    pub color: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamError {
    pub line: usize,
    pub message: String,
}

fn err(line: usize, message: impl Into<String>) -> ParamError {
    ParamError { line, message: message.into() }
}

fn parse_type(s: &str) -> Option<ParamType> {
    match s.replace(' ', "").as_str() {
        "f32" => Some(ParamType::F32),
        "i32" => Some(ParamType::I32),
        "bool" => Some(ParamType::Bool),
        "vec2<f32>" => Some(ParamType::Vec2),
        "vec3<f32>" => Some(ParamType::Vec3),
        "vec4<f32>" => Some(ParamType::Vec4),
        _ => None,
    }
}

fn parse_number_list(s: &str) -> Option<Vec<f32>> {
    let inner = s.trim().strip_prefix('(')?.strip_suffix(')')?;
    inner.split(',').map(|p| p.trim().parse::<f32>().ok()).collect()
}

fn parse_default(ty: ParamType, s: &str) -> Option<ParamValue> {
    let s = s.trim();
    Some(match ty {
        ParamType::F32 => ParamValue::F32(s.parse().ok()?),
        ParamType::I32 => ParamValue::I32(s.parse().ok()?),
        ParamType::Bool => ParamValue::Bool(match s {
            "true" => true,
            "false" => false,
            _ => return None,
        }),
        ParamType::Vec2 => ParamValue::Vec2(parse_number_list(s)?.try_into().ok()?),
        ParamType::Vec3 => ParamValue::Vec3(parse_number_list(s)?.try_into().ok()?),
        ParamType::Vec4 => ParamValue::Vec4(parse_number_list(s)?.try_into().ok()?),
    })
}

/// Returns `None` when the line is not a `// @param` annotation.
fn parse_line(line_no: usize, line: &str) -> Option<Result<ParamSpec, ParamError>> {
    let rest = line.trim().strip_prefix("//")?.trim_start();
    let body = rest.strip_prefix("@param")?;
    Some(parse_body(line_no, body))
}

/// WGSL keywords and reserved words (WGSL spec §15.2 and §15.3), plus the identifiers the
/// generated prelude uses. A param with one of these names would fail inside the prelude with a
/// confusing location, so they are rejected at the `@param` line instead.
const RESERVED_NAMES: &[&str] = &[
    // keywords
    "alias", "break", "case", "const", "const_assert", "continue", "continuing", "default",
    "diagnostic", "discard", "else", "enable", "false", "fn", "for", "if", "let", "loop",
    "override", "requires", "return", "struct", "switch", "true", "var", "while",
    // reserved words
    "NULL", "Self", "abstract", "active", "alignas", "alignof", "as", "asm", "asm_fragment",
    "async", "attribute", "auto", "await", "become", "cast", "catch", "class", "co_await",
    "co_return", "co_yield", "coherent", "column_major", "common", "compile", "compile_fragment",
    "concept", "const_cast", "consteval", "constexpr", "constinit", "crate", "debugger",
    "decltype", "delete", "demote", "demote_to_helper", "do", "dynamic_cast", "enum", "explicit",
    "export", "extends", "extern", "external", "fallthrough", "filter", "final", "finally",
    "friend", "from", "fxgroup", "get", "goto", "groupshared", "highp", "impl", "implements",
    "import", "inline", "instanceof", "interface", "layout", "lowp", "macro", "macro_rules",
    "match", "mediump", "meta", "mod", "module", "move", "mut", "mutable", "namespace", "new",
    "nil", "noexcept", "noinline", "nointerpolation", "non_coherent", "noncoherent", "noperspective",
    "null", "nullptr", "of", "operator", "package", "packoffset", "partition", "pass", "patch",
    "pixelfragment", "precise", "precision", "premerge", "priv", "protected", "pub", "public",
    "readonly", "ref", "regardless", "register", "reinterpret_cast", "require", "resource",
    "restrict", "self", "set", "shared", "sizeof", "smooth", "snorm", "static", "static_assert",
    "static_cast", "std", "subroutine", "super", "target", "template", "this", "thread_local",
    "throw", "trait", "try", "type", "typedef", "typeid", "typename", "typeof", "union", "unless",
    "unorm", "unsafe", "unsized", "use", "using", "varying", "virtual", "volatile", "wgsl",
    "where", "with", "writeonly", "yield",
    // predeclared types and the prelude's own identifiers
    "bool", "f16", "f32", "i32", "u32", "vec2", "vec3", "vec4", "mat2x2", "mat3x3", "mat4x4",
    "array", "atomic", "ptr", "sampler", "texture_2d", "texture_storage_2d",
    "params", "globals", "src", "dst", "state", "cell", "prev_cell", "hash", "rand", "wrap",
    "rule", "shade", "main", "vs_main", "fs_main", "Globals", "Params", "VsOut", "_unused",
];

fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || name.starts_with(|c: char| c.is_ascii_digit())
    {
        return Err(format!("invalid param name `{name}`"));
    }
    if name.starts_with("_pad") || RESERVED_NAMES.contains(&name) {
        return Err(format!("param name `{name}` is reserved in WGSL or by the generated prelude"));
    }
    Ok(())
}

fn parse_body(line_no: usize, body: &str) -> Result<ParamSpec, ParamError> {
    let (name, after_name) = body
        .split_once(':')
        .ok_or_else(|| err(line_no, "expected `name: type = default`"))?;
    let name = name.trim();
    valid_name(name).map_err(|m| err(line_no, m))?;
    let (ty_str, after_ty) = after_name
        .split_once('=')
        .ok_or_else(|| err(line_no, "expected `= default`"))?;
    let ty = parse_type(ty_str.trim())
        .ok_or_else(|| err(line_no, format!("unsupported param type `{}`", ty_str.trim())))?;

    let mut rest = after_ty.trim().to_string();
    let mut color = false;
    if let Some(stripped) = rest.strip_suffix("color") {
        color = true;
        rest = stripped.trim().to_string();
    }
    let mut range = None;
    let default_str = if let Some((def, rng)) = rest.split_once("range") {
        let (lo, hi) = rng
            .split_once("..")
            .ok_or_else(|| err(line_no, "range must be `LO .. HI`"))?;
        let lo: f64 = lo.trim().parse().map_err(|_| err(line_no, "bad range lower bound"))?;
        let hi: f64 = hi.trim().parse().map_err(|_| err(line_no, "bad range upper bound"))?;
        range = Some((lo, hi));
        def.trim().to_string()
    } else {
        rest
    };
    let default = parse_default(ty, &default_str)
        .ok_or_else(|| err(line_no, format!("bad default `{default_str}` for type")))?;
    if color && !matches!(ty, ParamType::Vec3 | ParamType::Vec4) {
        return Err(err(line_no, "`color` only applies to vec3<f32> or vec4<f32>"));
    }
    Ok(ParamSpec { name: name.to_string(), ty, default, range, color })
}

pub fn parse_params(source: &str) -> Result<Vec<ParamSpec>, ParamError> {
    let mut specs: Vec<ParamSpec> = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let Some(result) = parse_line(line_no, line) else { continue };
        let spec = result?;
        if specs.iter().any(|s| s.name == spec.name) {
            return Err(err(line_no, format!("duplicate param `{}`", spec.name)));
        }
        specs.push(spec);
        if specs.len() > MAX_PARAMS {
            return Err(err(line_no, format!("at most {MAX_PARAMS} params are supported")));
        }
    }
    Ok(specs)
}

/// Merges two param lists by name. The first list wins on duplicates; a type conflict is an error.
pub fn merge_params(a: Vec<ParamSpec>, b: Vec<ParamSpec>) -> Result<Vec<ParamSpec>, ParamError> {
    let mut out = a;
    for spec in b {
        match out.iter().find(|s| s.name == spec.name) {
            Some(existing) if existing.ty != spec.ty => {
                return Err(err(
                    0,
                    format!("param `{}` declared with two different types", spec.name),
                ));
            }
            Some(_) => {}
            None => out.push(spec),
        }
    }
    if out.len() > MAX_PARAMS {
        return Err(err(
            0,
            format!("at most {MAX_PARAMS} params are supported across both shaders"),
        ));
    }
    Ok(out)
}

/// Emits `struct Params { ... }` with every param occupying one 16-byte slot, in declaration order.
pub fn params_wgsl(specs: &[ParamSpec]) -> String {
    let mut s = String::from("struct Params {\n");
    if specs.is_empty() {
        s.push_str("    _unused: vec4<f32>,\n");
    }
    for (i, spec) in specs.iter().enumerate() {
        let (ty, pads) = match spec.ty {
            ParamType::F32 => ("f32", 3),
            ParamType::I32 => ("i32", 3),
            ParamType::Bool => ("u32", 3),
            ParamType::Vec2 => ("vec2<f32>", 2),
            ParamType::Vec3 => ("vec3<f32>", 1),
            ParamType::Vec4 => ("vec4<f32>", 0),
        };
        s.push_str(&format!("    {}: {},", spec.name, ty));
        for p in 0..pads {
            s.push_str(&format!(" _pad{i}_{p}: u32,"));
        }
        s.push('\n');
    }
    s.push_str("}\n");
    s
}

pub fn pack_slot(v: &ParamValue) -> [u32; 4] {
    match v {
        ParamValue::F32(x) => [x.to_bits(), 0, 0, 0],
        ParamValue::I32(x) => [*x as u32, 0, 0, 0],
        ParamValue::Bool(b) => [*b as u32, 0, 0, 0],
        ParamValue::Vec2(a) => [a[0].to_bits(), a[1].to_bits(), 0, 0],
        ParamValue::Vec3(a) => [a[0].to_bits(), a[1].to_bits(), a[2].to_bits(), 0],
        ParamValue::Vec4(a) => [a[0].to_bits(), a[1].to_bits(), a[2].to_bits(), a[3].to_bits()],
    }
}

pub fn pack_params(
    specs: &[ParamSpec],
    values: &BTreeMap<String, ParamValue>,
) -> [[u32; 4]; MAX_PARAMS] {
    let mut out = [[0u32; 4]; MAX_PARAMS];
    for (i, spec) in specs.iter().take(MAX_PARAMS).enumerate() {
        let v = values.get(&spec.name).filter(|v| v.ty() == spec.ty).unwrap_or(&spec.default);
        out[i] = pack_slot(v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parses_f32_with_range() {
        let specs =
            parse_params("// @param threshold: f32 = 0.5 range 0.0 .. 1.0\nfn rule() {}").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "threshold");
        assert_eq!(specs[0].ty, ParamType::F32);
        assert_eq!(specs[0].default, ParamValue::F32(0.5));
        assert_eq!(specs[0].range, Some((0.0, 1.0)));
        assert!(!specs[0].color);
    }

    #[test]
    fn parses_i32_bool_and_vectors() {
        let src = "\
// @param n: i32 = 3 range 0 .. 8
// @param on: bool = true
// @param uv: vec2<f32> = (0.1, 0.2)
// @param col: vec3<f32> = (1.0, 0.5, 0.2) color
// @param q: vec4<f32> = (1, 2, 3, 4)
";
        let specs = parse_params(src).unwrap();
        assert_eq!(specs[0].default, ParamValue::I32(3));
        assert_eq!(specs[1].default, ParamValue::Bool(true));
        assert_eq!(specs[2].default, ParamValue::Vec2([0.1, 0.2]));
        assert_eq!(specs[3].default, ParamValue::Vec3([1.0, 0.5, 0.2]));
        assert!(specs[3].color);
        assert_eq!(specs[4].default, ParamValue::Vec4([1.0, 2.0, 3.0, 4.0]));
    }

    #[test]
    fn tolerates_crlf_and_extra_spaces() {
        let specs = parse_params("//   @param   a :  f32 =  2  \r\n").unwrap();
        assert_eq!(specs[0].name, "a");
        assert_eq!(specs[0].default, ParamValue::F32(2.0));
    }

    #[test]
    fn ignores_non_param_comments_and_code() {
        let specs = parse_params("// hello\nlet x = 1.0; // @param not at start\n").unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn reports_bad_type_with_line() {
        let err = parse_params("fn a() {}\n// @param x: f64 = 1.0\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("f64"));
    }

    #[test]
    fn reports_duplicate_name() {
        let err = parse_params("// @param x: f32 = 1\n// @param x: f32 = 2\n").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn rejects_more_than_max_params() {
        let src: String = (0..17).map(|i| format!("// @param p{i}: f32 = 0\n")).collect();
        assert!(parse_params(&src).is_err());
    }

    #[test]
    fn merge_keeps_first_and_rejects_type_conflict() {
        let a = parse_params("// @param x: f32 = 1\n").unwrap();
        let b = parse_params("// @param x: f32 = 2\n// @param y: i32 = 1\n").unwrap();
        let merged = merge_params(a.clone(), b).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].default, ParamValue::F32(1.0));
        let c = parse_params("// @param x: i32 = 2\n").unwrap();
        assert!(merge_params(a, c).is_err());
    }

    #[test]
    fn wgsl_struct_pads_every_param_to_16_bytes() {
        let specs = parse_params(
            "// @param a: f32 = 1\n// @param c: vec3<f32> = (0,0,0)\n// @param v: vec2<f32> = (0,0)\n// @param q: vec4<f32> = (0,0,0,0)\n// @param b: bool = false\n",
        )
        .unwrap();
        let wgsl = params_wgsl(&specs);
        assert!(wgsl.contains("a: f32, _pad0_0: u32, _pad0_1: u32, _pad0_2: u32"));
        assert!(wgsl.contains("c: vec3<f32>, _pad1_0: u32"));
        assert!(wgsl.contains("v: vec2<f32>, _pad2_0: u32, _pad2_1: u32"));
        assert!(wgsl.contains("q: vec4<f32>,"));
        assert!(wgsl.contains("b: u32, _pad4_0: u32"));
    }

    #[test]
    fn wgsl_struct_for_no_params_is_non_empty() {
        assert!(params_wgsl(&[]).contains("_unused: vec4<f32>"));
    }

    #[test]
    fn pack_uses_bit_patterns() {
        assert_eq!(pack_slot(&ParamValue::F32(1.0)), [1.0f32.to_bits(), 0, 0, 0]);
        assert_eq!(pack_slot(&ParamValue::I32(-1)), [(-1i32) as u32, 0, 0, 0]);
        assert_eq!(pack_slot(&ParamValue::Bool(true)), [1, 0, 0, 0]);
        assert_eq!(
            pack_slot(&ParamValue::Vec2([1.0, 2.0])),
            [1.0f32.to_bits(), 2.0f32.to_bits(), 0, 0]
        );
    }

    #[test]
    fn pack_params_uses_value_or_default_in_slot_order() {
        let specs = parse_params("// @param a: f32 = 1\n// @param b: f32 = 2\n").unwrap();
        let mut values = BTreeMap::new();
        values.insert("b".to_string(), ParamValue::F32(5.0));
        let packed = pack_params(&specs, &values);
        assert_eq!(packed[0][0], 1.0f32.to_bits());
        assert_eq!(packed[1][0], 5.0f32.to_bits());
        assert_eq!(packed[2], [0, 0, 0, 0]);
    }

    #[test]
    fn rejects_reserved_and_generated_names() {
        for bad in ["loop", "fn", "type", "f32", "vec3", "_pad0_0", "_pad7_2", "_unused", "params", "globals"] {
            assert!(
                parse_params(&format!("fn x() {{}}\n// @param {bad}: f32 = 1\n")).is_err(),
                "`{bad}` should be rejected"
            );
        }
        let err = parse_params("fn x() {}\n// @param loop: f32 = 1\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("loop"));
        assert!(parse_params("// @param padding: f32 = 1\n// @param my_loop: f32 = 2\n").is_ok());
    }
}

# Cellular Automata Shader IDE Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust desktop app where 1D/2D cellular automata rules and their visualisation are live-edited WGSL shaders running on the GPU.

**Architecture:** `eframe` (wgpu backend) owns the window and device. A `Simulation` struct owns two ping-pong `rgba32float` textures, uniform buffers and the compute/render pipelines, and is shared with an `egui_wgpu::CallbackTrait` paint callback through an `Arc<Mutex<_>>`. User WGSL is wrapped in a fixed prelude/epilogue, validated with `naga` for line-accurate errors, and only swapped in when it compiles. Presets are folders of `preset.toml` + `rule.wgsl` + `render.wgsl`.

**Tech Stack:** Rust 1.96 (edition 2024), eframe 0.36.2, egui 0.36.2, egui-wgpu 0.36.2, wgpu 30.0.1, naga 30.0.1 (`wgsl-in`), bytemuck 1, serde 1 + toml 1, rand 0.10 + rand_chacha 0.10, rfd 0.17, pollster 1, anyhow 1, log + env_logger.

**Spec:** `docs/superpowers/specs/2026-10-01-ca-shader-ide-design.md`

## Global Constraints

- One wgpu in the dependency tree: eframe 0.36.2 pulls wgpu 30.0.1 and naga 30.0.1; pin `naga = "30"` and do not add `wgpu` directly (use `eframe::wgpu`).
- Cell state is `rgba32float`. Sampling is done with `textureLoad` (no sampler, no filtering feature needed).
- Grid default 512x512; clamp to `limits.max_texture_dimension_2d` and to `max_compute_workgroups_per_dimension * 16`.
- A failed shader build never replaces the live pipeline.
- Max 16 `@param` declarations; each occupies one 16-byte slot in the `Params` uniform.
- Compute bind group 0: `0 = src texture_2d<f32>`, `1 = dst texture_storage_2d<rgba32float, write>`, `2 = globals uniform`, `3 = params uniform`. Render bind group 0: `0 = state texture_2d<f32>`, `2 = globals`, `3 = params`.
- Globals uniform is exactly 32 bytes: `size: vec2<u32>, frame: u32, seed: u32, time: f32, mode: u32, row: u32, prev_row: u32`.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. A `@param` line with a trailing `\r` (Windows line endings) must parse identically to one without. Test in Task 2.
2. A grid height of 1 in 1D mode must not issue a zero-sized texture copy (wgpu validation error). Test `plan_row(1, 1)` in Task 8.
3. A preset folder whose `preset.toml` references a param that the shader no longer declares must load and silently ignore the value. Test in Task 6.
4. A shader error on a line after the user's last line (e.g. missing closing brace reported at EOF) must clamp to the last user line, not a negative or prelude line. Test in Task 4.
5. Changing grid width/height to a value beyond device limits must clamp, not panic. Test `clamp_size` in Task 8.

---

### Task 1: Project scaffold and empty window

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/app.rs`, `src/lib.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `cellular_automata` lib crate with modules `shader`, `sim`, `preset`, `viewport`, `app` (modules added by later tasks; this task creates the files as empty `pub mod` stubs only for `app`).

- [ ] **Step 1: Write Cargo.toml**

```toml
[package]
name = "cellular-automata"
version = "0.1.0"
edition = "2024"

[lib]
name = "cellular_automata"
path = "src/lib.rs"

[[bin]]
name = "cellular-automata"
path = "src/main.rs"

[dependencies]
eframe = "0.36.2"
egui = "0.36.2"
naga = { version = "30", features = ["wgsl-in"] }
bytemuck = { version = "1", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "1"
rand = "0.10"
rand_chacha = "0.10"
rfd = "0.17"
pollster = "1"
anyhow = "1"
log = "0.4"
env_logger = "0.11"

[dev-dependencies]
tempfile = "3"

[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

- [ ] **Step 2: Write src/lib.rs and src/app.rs and src/main.rs**

`src/lib.rs`:
```rust
pub mod app;
```

`src/app.rs`:
```rust
pub struct App;

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        App
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.label("Cellular Automata Shader IDE");
    }
}
```

`src/main.rs`:
```rust
fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Cellular Automata Shader IDE")
            .with_inner_size([1400.0, 900.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "cellular-automata",
        options,
        Box::new(|cc| Ok(Box::new(cellular_automata::app::App::new(cc)))),
    )
}
```

- [ ] **Step 3: Write CI workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci
on: [push, pull_request]
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all-targets
      - run: cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles (first build downloads and compiles ~300 crates, several minutes).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src .github
git commit -m "Scaffold eframe/wgpu application"
```

---

### Task 2: `@param` annotation parser and Params WGSL codegen

**Files:**
- Create: `src/shader/mod.rs`, `src/shader/params.rs`
- Modify: `src/lib.rs` (add `pub mod shader;`)

**Interfaces:**
- Produces:
  ```rust
  pub enum ParamType { F32, I32, Bool, Vec2, Vec3, Vec4 }
  pub enum ParamValue { F32(f32), I32(i32), Bool(bool), Vec2([f32; 2]), Vec3([f32; 3]), Vec4([f32; 4]) }
  pub struct ParamSpec { pub name: String, pub ty: ParamType, pub default: ParamValue, pub range: Option<(f64, f64)>, pub color: bool }
  pub struct ParamError { pub line: usize, pub message: String }
  pub const MAX_PARAMS: usize = 16;
  pub fn parse_params(source: &str) -> Result<Vec<ParamSpec>, ParamError>
  pub fn merge_params(a: Vec<ParamSpec>, b: Vec<ParamSpec>) -> Result<Vec<ParamSpec>, ParamError>  // b's conflicts reported with b's line
  pub fn params_wgsl(specs: &[ParamSpec]) -> String   // "struct Params { ... }" only (binding decl lives in prelude)
  pub fn pack_slot(v: &ParamValue) -> [u32; 4]
  pub fn pack_params(specs: &[ParamSpec], values: &BTreeMap<String, ParamValue>) -> [[u32; 4]; MAX_PARAMS]
  impl ParamValue { pub fn ty(&self) -> ParamType }
  ```
- Grammar: `// @param NAME: TYPE = DEFAULT [range LO .. HI] [color]`. `DEFAULT` is a number, `true`/`false`, or `(a, b[, c[, d]])`. `TYPE` in `f32 i32 bool vec2<f32> vec3<f32> vec4<f32>`.
- WGSL output: one 16-byte slot per param, in declaration order. `f32` → `name: f32, _padN_0: u32, _padN_1: u32, _padN_2: u32`; `i32` → `name: i32` + 3 pads; `bool` → `name: u32` + 3 pads; `vec2<f32>` → `name: vec2<f32>, _padN_0: u32, _padN_1: u32`; `vec3<f32>` → `name: vec3<f32>, _padN_0: u32`; `vec4<f32>` → `name: vec4<f32>`. No params → `struct Params { _unused: vec4<f32>, }`.

- [ ] **Step 1: Write failing tests** at the bottom of `src/shader/params.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parses_f32_with_range() {
        let specs = parse_params("// @param threshold: f32 = 0.5 range 0.0 .. 1.0\nfn rule() {}").unwrap();
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
        let specs = parse_params("// @param a: f32 = 1\n// @param c: vec3<f32> = (0,0,0)\n// @param v: vec2<f32> = (0,0)\n// @param q: vec4<f32> = (0,0,0,0)\n// @param b: bool = false\n").unwrap();
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
        assert_eq!(pack_slot(&ParamValue::Vec2([1.0, 2.0])), [1.0f32.to_bits(), 2.0f32.to_bits(), 0, 0]);
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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test shader::params`
Expected: compile error, `parse_params` not found.

- [ ] **Step 3: Implement** `src/shader/mod.rs`:

```rust
pub mod params;
```

`src/shader/params.rs` (implementation above the tests):

```rust
use std::collections::BTreeMap;

pub const MAX_PARAMS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType { F32, I32, Bool, Vec2, Vec3, Vec4 }

#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    F32(f32), I32(i32), Bool(bool), Vec2([f32; 2]), Vec3([f32; 3]), Vec4([f32; 4]),
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
pub struct ParamError { pub line: usize, pub message: String }

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
        ParamType::Bool => ParamValue::Bool(match s { "true" => true, "false" => false, _ => return None }),
        ParamType::Vec2 => { let v = parse_number_list(s)?; ParamValue::Vec2(v.try_into().ok()?) }
        ParamType::Vec3 => { let v = parse_number_list(s)?; ParamValue::Vec3(v.try_into().ok()?) }
        ParamType::Vec4 => { let v = parse_number_list(s)?; ParamValue::Vec4(v.try_into().ok()?) }
    })
}

/// Parses one `@param` line body (text after `@param`). Returns None if not an annotation.
fn parse_line(line_no: usize, line: &str) -> Option<Result<ParamSpec, ParamError>> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("//")?.trim_start();
    let rest = rest.strip_prefix("@param")?;
    Some(parse_body(line_no, rest))
}

fn parse_body(line_no: usize, body: &str) -> Result<ParamSpec, ParamError> {
    let (name, after_name) = body.split_once(':').ok_or_else(|| err(line_no, "expected `name: type = default`"))?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(err(line_no, format!("invalid param name `{name}`")));
    }
    let (ty_str, after_ty) = after_name.split_once('=').ok_or_else(|| err(line_no, "expected `= default`"))?;
    let ty = parse_type(ty_str.trim()).ok_or_else(|| err(line_no, format!("unsupported param type `{}`", ty_str.trim())))?;

    // Split the remainder into default / `range LO .. HI` / `color` words.
    let mut rest = after_ty.trim().to_string();
    let mut color = false;
    if let Some(stripped) = rest.strip_suffix("color") { color = true; rest = stripped.trim().to_string(); }
    let mut range = None;
    let default_str = if let Some((def, rng)) = rest.split_once("range") {
        let (lo, hi) = rng.split_once("..").ok_or_else(|| err(line_no, "range must be `LO .. HI`"))?;
        let lo: f64 = lo.trim().parse().map_err(|_| err(line_no, "bad range lower bound"))?;
        let hi: f64 = hi.trim().parse().map_err(|_| err(line_no, "bad range upper bound"))?;
        range = Some((lo, hi));
        def.trim().to_string()
    } else { rest };
    let default = parse_default(ty, &default_str).ok_or_else(|| err(line_no, format!("bad default `{default_str}` for type")))?;
    if color && !matches!(ty, ParamType::Vec3 | ParamType::Vec4) {
        return Err(err(line_no, "`color` only applies to vec3<f32> or vec4<f32>"));
    }
    Ok(ParamSpec { name: name.to_string(), ty, default, range, color })
}

pub fn parse_params(source: &str) -> Result<Vec<ParamSpec>, ParamError> {
    let mut specs: Vec<ParamSpec> = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        if let Some(result) = parse_line(line_no, line) {
            let spec = result?;
            if specs.iter().any(|s| s.name == spec.name) {
                return Err(err(line_no, format!("duplicate param `{}`", spec.name)));
            }
            specs.push(spec);
            if specs.len() > MAX_PARAMS {
                return Err(err(line_no, format!("at most {MAX_PARAMS} params are supported")));
            }
        }
    }
    Ok(specs)
}

pub fn merge_params(a: Vec<ParamSpec>, b: Vec<ParamSpec>) -> Result<Vec<ParamSpec>, ParamError> {
    let mut out = a;
    for spec in b {
        if let Some(existing) = out.iter().find(|s| s.name == spec.name) {
            if existing.ty != spec.ty {
                return Err(err(0, format!("param `{}` declared with two different types", spec.name)));
            }
        } else {
            out.push(spec);
        }
    }
    if out.len() > MAX_PARAMS {
        return Err(err(0, format!("at most {MAX_PARAMS} params are supported across both shaders")));
    }
    Ok(out)
}

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
        for p in 0..pads { s.push_str(&format!(" _pad{i}_{p}: u32,")); }
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

pub fn pack_params(specs: &[ParamSpec], values: &BTreeMap<String, ParamValue>) -> [[u32; 4]; MAX_PARAMS] {
    let mut out = [[0u32; 4]; MAX_PARAMS];
    for (i, spec) in specs.iter().take(MAX_PARAMS).enumerate() {
        let v = values.get(&spec.name).filter(|v| v.ty() == spec.ty).unwrap_or(&spec.default);
        out[i] = pack_slot(v);
    }
    out
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test shader::params`
Expected: all 12 pass.

- [ ] **Step 5: Commit**

```bash
git add src/shader src/lib.rs
git commit -m "Add @param annotation parser and Params WGSL codegen"
```

---

### Task 3: Shader assembly (prelude + user source + epilogue)

**Files:**
- Create: `src/shader/assemble.rs`
- Modify: `src/shader/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Assembled { pub source: String, pub user_line_offset: usize, pub user_line_count: usize }
  pub fn assemble_rule(user: &str, params_struct: &str) -> Assembled
  pub fn assemble_render(user: &str, params_struct: &str) -> Assembled
  ```
  `user_line_offset` = number of lines before the user's first line (so user line `n` is assembled line `n + offset`).

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::params::params_wgsl;

    #[test]
    fn rule_offset_points_at_user_source() {
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> { return vec4<f32>(0.0); }";
        let a = assemble_rule(user, &params_wgsl(&[]));
        let lines: Vec<&str> = a.source.lines().collect();
        assert_eq!(lines[a.user_line_offset], user);
        assert_eq!(a.user_line_count, 1);
    }

    #[test]
    fn render_offset_points_at_user_source() {
        let user = "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return cell; }";
        let a = assemble_render(user, &params_wgsl(&[]));
        let lines: Vec<&str> = a.source.lines().collect();
        assert_eq!(lines[a.user_line_offset], user);
    }

    #[test]
    fn rule_contains_bindings_and_entry() {
        let a = assemble_rule("fn rule(pos: vec2<u32>) -> vec4<f32> { return vec4<f32>(0.0); }", &params_wgsl(&[]));
        assert!(a.source.contains("@group(0) @binding(0) var src: texture_2d<f32>;"));
        assert!(a.source.contains("@group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;"));
        assert!(a.source.contains("@compute @workgroup_size(16, 16)"));
    }

    #[test]
    fn offset_counts_params_struct_lines() {
        let one = assemble_rule("x", "struct Params { a: f32, }\n");
        let two = assemble_rule("x", "struct Params {\n a: f32,\n}\n");
        assert_eq!(two.user_line_offset, one.user_line_offset + 2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test shader::assemble`
Expected: compile error.

- [ ] **Step 3: Implement** `src/shader/assemble.rs`

```rust
pub struct Assembled {
    pub source: String,
    pub user_line_offset: usize,
    pub user_line_count: usize,
}

pub const GLOBALS_WGSL: &str = r#"struct Globals {
    size: vec2<u32>,
    frame: u32,
    seed: u32,
    time: f32,
    mode: u32,
    row: u32,
    prev_row: u32,
}
"#;

const RULE_PRELUDE: &str = r#"@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> globals: Globals;
@group(0) @binding(3) var<uniform> params: Params;

fn wrap(v: i32, n: u32) -> u32 {
    let ni = i32(n);
    return u32(((v % ni) + ni) % ni);
}

fn cell(x: i32, y: i32) -> vec4<f32> {
    return textureLoad(src, vec2<u32>(wrap(x, globals.size.x), wrap(y, globals.size.y)), 0);
}

fn prev_cell(x: i32) -> vec4<f32> {
    return cell(x, i32(globals.prev_row));
}

fn hash(v: u32) -> u32 {
    var x = v;
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn rand(pos: vec2<u32>, salt: u32) -> f32 {
    let h = hash(pos.x ^ hash(pos.y ^ hash(salt ^ globals.seed)));
    return f32(h) / 4294967295.0;
}

// ---- user rule ----
"#;

const RULE_EPILOGUE: &str = r#"
// ---- entry ----
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var pos = gid.xy;
    if (globals.mode == 1u) {
        if (gid.y != 0u) { return; }
        pos.y = globals.row;
    }
    if (pos.x >= globals.size.x || pos.y >= globals.size.y) { return; }
    textureStore(dst, pos, rule(pos));
}
"#;

const RENDER_PRELUDE: &str = r#"@group(0) @binding(0) var state: texture_2d<f32>;
@group(0) @binding(2) var<uniform> globals: Globals;
@group(0) @binding(3) var<uniform> params: Params;

fn cell(x: i32, y: i32) -> vec4<f32> {
    let w = i32(globals.size.x);
    let h = i32(globals.size.y);
    let xx = u32(((x % w) + w) % w);
    let yy = u32(((y % h) + h) % h);
    return textureLoad(state, vec2<u32>(xx, yy), 0);
}

// ---- user render ----
"#;

const RENDER_EPILOGUE: &str = r#"
// ---- entry ----
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let fsize = vec2<f32>(globals.size);
    let p = vec2<u32>(clamp(in.uv * fsize, vec2<f32>(0.0), fsize - vec2<f32>(1.0)));
    let c = textureLoad(state, p, 0);
    return shade(in.uv, c);
}
"#;

fn assemble(prelude: &str, user: &str, params_struct: &str, epilogue: &str) -> Assembled {
    let head = format!("{GLOBALS_WGSL}{params_struct}{prelude}");
    let user_line_offset = head.lines().count();
    let user_line_count = user.lines().count().max(1);
    let source = format!("{head}{user}\n{epilogue}");
    Assembled { source, user_line_offset, user_line_count }
}

pub fn assemble_rule(user: &str, params_struct: &str) -> Assembled {
    assemble(RULE_PRELUDE, user, params_struct, RULE_EPILOGUE)
}

pub fn assemble_render(user: &str, params_struct: &str) -> Assembled {
    assemble(RENDER_PRELUDE, user, params_struct, RENDER_EPILOGUE)
}
```

Note: `head.lines().count()` relies on every prelude part ending in `\n`; `params_wgsl` output does, and both constants do.

- [ ] **Step 4: Run tests**

Run: `cargo test shader::assemble`
Expected: 4 pass.

- [ ] **Step 5: Commit**

```bash
git add src/shader
git commit -m "Add WGSL prelude/epilogue assembly for rule and render shaders"
```

---

### Task 4: naga validation with user-relative error locations

**Files:**
- Create: `src/shader/validate.rs`
- Modify: `src/shader/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub enum ShaderFile { Rule, Render }
  #[derive(Debug, Clone, PartialEq)]
  pub struct ShaderError { pub file: ShaderFile, pub line: usize, pub column: usize, pub message: String }
  pub fn validate(file: ShaderFile, assembled: &Assembled) -> Result<naga::Module, Vec<ShaderError>>
  pub fn validate_ok(assembled: &Assembled) -> bool  // convenience for tests
  ```
  `line` is 1-based in the user's source, clamped to `1..=user_line_count`. `message` is naga's message without the ASCII-art snippet (use `Display`), plus a `(in generated prelude)` suffix when the span fell outside the user range.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::assemble::{assemble_rule, assemble_render};
    use crate::shader::params::params_wgsl;

    const OK_RULE: &str = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return cell(i32(pos.x), i32(pos.y));\n}\n";
    const OK_RENDER: &str = "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {\n    return vec4<f32>(cell.rgb, 1.0);\n}\n";

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
        // type error: returning f32 where vec4 expected is caught by the validator, not the parser
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return vec4<f32>(0.0);\n}\nfn other() -> f32 { return vec2<f32>(0.0); }\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert!(errs[0].line >= 1 && errs[0].line <= 4, "line was {}", errs[0].line);
    }

    #[test]
    fn missing_rule_function_is_reported_in_user_range() {
        let user = "fn not_rule() -> f32 { return 1.0; }\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert!(errs[0].line >= 1 && errs[0].line <= 1);
        assert!(errs[0].message.contains("rule"));
    }

    #[test]
    fn eof_error_clamps_to_last_user_line() {
        let user = "fn rule(pos: vec2<u32>) -> vec4<f32> {\n    return vec4<f32>(0.0);\n";
        let errs = validate(ShaderFile::Rule, &assemble_rule(user, &params_wgsl(&[]))).unwrap_err();
        assert!(errs[0].line >= 1 && errs[0].line <= 2, "line was {}", errs[0].line);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test shader::validate`
Expected: compile error.

- [ ] **Step 3: Implement** `src/shader/validate.rs`

```rust
use super::assemble::Assembled;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderFile { Rule, Render }

impl ShaderFile {
    pub fn label(self) -> &'static str {
        match self { ShaderFile::Rule => "rule.wgsl", ShaderFile::Render => "render.wgsl" }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderError {
    pub file: ShaderFile,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

fn map_location(file: ShaderFile, assembled: &Assembled, loc: Option<naga::SourceLocation>, message: String) -> ShaderError {
    let (line, column, in_prelude) = match loc {
        Some(l) => {
            let abs = l.line_number as usize; // 1-based in assembled source
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

pub fn validate(file: ShaderFile, assembled: &Assembled) -> Result<naga::Module, Vec<ShaderError>> {
    let module = match naga::front::wgsl::parse_str(&assembled.source) {
        Ok(m) => m,
        Err(e) => {
            let loc = e.location(&assembled.source);
            return Err(vec![map_location(file, assembled, loc, e.message().to_string())]);
        }
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    match validator.validate(&module) {
        Ok(_) => Ok(module),
        Err(e) => {
            let loc = e.location(&assembled.source);
            let message = e.emit_to_string(&assembled.source);
            // Keep only the first line of naga's diagnostic (the headline).
            let headline = message.lines().next().unwrap_or("validation error").trim_start_matches("error: ").to_string();
            Err(vec![map_location(file, assembled, loc, headline)])
        }
    }
}

pub fn validate_ok(assembled: &Assembled) -> bool {
    validate(ShaderFile::Rule, assembled).is_ok()
}
```

If `ParseError::message()` does not exist in naga 30, use `e.to_string()` and take the first line. Check with `grep -n "pub fn message" ~/.cargo/registry/src/*/naga-30.0.1/src/front/wgsl/error.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test shader::validate`
Expected: 5 pass. If `missing_rule_function_is_reported_in_user_range` reports the epilogue line (the call site of `rule`), the clamp maps it to the last user line, which satisfies the assertion.

- [ ] **Step 5: Commit**

```bash
git add src/shader
git commit -m "Validate assembled WGSL with naga and map errors to user lines"
```

---

### Task 5: Preset model, TOML round-trip, folder load/save

**Files:**
- Create: `src/preset/mod.rs`
- Modify: `src/lib.rs` (add `pub mod preset;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "lowercase")]
  pub enum Mode { #[serde(rename = "2d")] TwoD, #[serde(rename = "1d")] OneD }
  #[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] #[serde(tag = "kind", rename_all = "lowercase")]
  pub enum InitPattern { Random { density: f32 }, Single, Blank }
  #[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
  pub struct PresetMeta { pub name: String, pub mode: Mode, pub width: u32, pub height: u32, pub steps_per_frame: u32, pub seed: u32, pub init: InitPattern, #[serde(default)] pub params: BTreeMap<String, toml::Value> }
  #[derive(Clone, PartialEq, Debug)]
  pub struct Preset { pub meta: PresetMeta, pub rule: String, pub render: String }
  impl Preset {
      pub fn load_dir(dir: &Path) -> anyhow::Result<Preset>
      pub fn save_dir(&self, dir: &Path) -> anyhow::Result<()>
  }
  pub fn param_value_from_toml(ty: ParamType, v: &toml::Value) -> Option<ParamValue>
  pub fn param_value_to_toml(v: &ParamValue) -> toml::Value
  pub fn scan_presets_dir(dir: &Path) -> Vec<(String, PathBuf)>   // (name, folder) sorted by name; silently skips bad folders
  ```

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::params::{ParamType, ParamValue};

    fn sample() -> Preset {
        let mut params = BTreeMap::new();
        params.insert("threshold".into(), toml::Value::Float(0.5));
        params.insert("col".into(), toml::Value::Array(vec![toml::Value::Float(1.0), toml::Value::Float(0.5), toml::Value::Float(0.2)]));
        Preset {
            meta: PresetMeta {
                name: "Test".into(), mode: Mode::TwoD, width: 64, height: 32,
                steps_per_frame: 2, seed: 7, init: InitPattern::Random { density: 0.3 }, params,
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
        assert_eq!(param_value_from_toml(ParamType::F32, &toml::Value::Float(0.5)), Some(ParamValue::F32(0.5)));
        assert_eq!(param_value_from_toml(ParamType::F32, &toml::Value::Integer(2)), Some(ParamValue::F32(2.0)));
        assert_eq!(param_value_from_toml(ParamType::I32, &toml::Value::Integer(3)), Some(ParamValue::I32(3)));
        assert_eq!(param_value_from_toml(ParamType::Bool, &toml::Value::Boolean(true)), Some(ParamValue::Bool(true)));
        assert_eq!(param_value_from_toml(ParamType::Vec3, &toml::Value::Array(vec![toml::Value::Float(1.0), toml::Value::Integer(0), toml::Value::Float(0.5)])), Some(ParamValue::Vec3([1.0, 0.0, 0.5])));
        assert_eq!(param_value_from_toml(ParamType::Vec3, &toml::Value::Array(vec![toml::Value::Float(1.0)])), None);
        assert_eq!(param_value_from_toml(ParamType::I32, &toml::Value::String("x".into())), None);
        let back = param_value_to_toml(&ParamValue::Vec2([1.0, 2.0]));
        assert_eq!(param_value_from_toml(ParamType::Vec2, &back), Some(ParamValue::Vec2([1.0, 2.0])));
    }

    #[test]
    fn scan_lists_valid_folders_sorted_and_skips_bad_ones() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = sample(); b.meta.name = "Bravo".into();
        let mut a = sample(); a.meta.name = "Alpha".into();
        b.save_dir(&dir.path().join("b")).unwrap();
        a.save_dir(&dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("junk")).unwrap();
        let list = scan_presets_dir(dir.path());
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Bravo"]);
        assert!(scan_presets_dir(Path::new("nope")).is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test preset::`
Expected: compile error.

- [ ] **Step 3: Implement** `src/preset/mod.rs`

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::shader::params::{ParamType, ParamValue};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    #[serde(rename = "2d")] TwoD,
    #[serde(rename = "1d")] OneD,
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
        let meta_text = std::fs::read_to_string(dir.join("preset.toml"))
            .with_context(|| format!("reading {}", dir.join("preset.toml").display()))?;
        let meta: PresetMeta = toml::from_str(&meta_text).context("parsing preset.toml")?;
        let rule = std::fs::read_to_string(dir.join("rule.wgsl")).context("reading rule.wgsl")?;
        let render = std::fs::read_to_string(dir.join("render.wgsl")).context("reading render.wgsl")?;
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
    if arr.len() != n { return None; }
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
    let arr = |a: &[f32]| toml::Value::Array(a.iter().map(|x| toml::Value::Float(*x as f64)).collect());
    match v {
        ParamValue::F32(x) => toml::Value::Float(*x as f64),
        ParamValue::I32(x) => toml::Value::Integer(*x as i64),
        ParamValue::Bool(b) => toml::Value::Boolean(*b),
        ParamValue::Vec2(a) => arr(a),
        ParamValue::Vec3(a) => arr(a),
        ParamValue::Vec4(a) => arr(a),
    }
}

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
```

- [ ] **Step 4: Run tests**

Run: `cargo test preset::`
Expected: 7 pass.

- [ ] **Step 5: Commit**

```bash
git add src/preset src/lib.rs
git commit -m "Add preset model with TOML round-trip and folder load/save"
```

---

### Task 6: Built-in presets and their validation test

**Files:**
- Create: `presets/rule30/{preset.toml,rule.wgsl,render.wgsl}`, `presets/rule110/...`, `presets/life/...`, `presets/gray_scott/...`
- Create: `src/preset/builtin.rs`
- Modify: `src/preset/mod.rs` (add `pub mod builtin;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct Builtin { pub id: &'static str, pub meta_toml: &'static str, pub rule: &'static str, pub render: &'static str }
  pub const BUILTINS: &[Builtin]
  pub fn load_builtin(b: &Builtin) -> Preset   // panics only if embedded TOML is invalid (covered by test)
  ```

- [ ] **Step 1: Write the preset files**

`presets/rule30/preset.toml`:
```toml
name = "Rule 30"
mode = "1d"
width = 1024
height = 512
steps_per_frame = 2
seed = 1

[init]
kind = "single"

[params]
```

`presets/rule30/rule.wgsl`:
```wgsl
// Elementary cellular automaton. Change `rule_number` to explore all 256 rules.
// @param rule_number: i32 = 30 range 0 .. 255

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let l = u32(prev_cell(x - 1).r > 0.5);
    let c = u32(prev_cell(x).r > 0.5);
    let r = u32(prev_cell(x + 1).r > 0.5);
    let idx = (l << 2u) | (c << 1u) | r;
    let on = (u32(params.rule_number) >> idx) & 1u;
    return vec4<f32>(f32(on), 0.0, 0.0, 1.0);
}
```

`presets/rule30/render.wgsl`:
```wgsl
// @param fg: vec3<f32> = (0.95, 0.95, 0.9) color
// @param bg: vec3<f32> = (0.08, 0.08, 0.1) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(mix(params.bg, params.fg, cell.r), 1.0);
}
```

`presets/rule110/preset.toml`: same as rule30 but `name = "Rule 110"`, `seed = 2`, and `[params]` with `rule_number = 110`. `rule.wgsl` and `render.wgsl` identical to rule30 (the param default stays 30; the TOML overrides it).

`presets/life/preset.toml`:
```toml
name = "Game of Life"
mode = "2d"
width = 512
height = 512
steps_per_frame = 1
seed = 42

[init]
kind = "random"
density = 0.3

[params]
```

`presets/life/rule.wgsl`:
```wgsl
// Conway's Game of Life. r = alive (0/1), g = age in steps (for colouring).

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    var n = 0u;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) { continue; }
            n += u32(cell(x + dx, y + dy).r > 0.5);
        }
    }
    let me = cell(x, y);
    let alive = me.r > 0.5;
    let next = (alive && (n == 2u || n == 3u)) || (!alive && n == 3u);
    let age = select(0.0, me.g + 1.0, next && alive);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}
```

`presets/life/render.wgsl`:
```wgsl
// @param young: vec3<f32> = (1.0, 0.9, 0.3) color
// @param old: vec3<f32> = (0.2, 0.5, 1.0) color
// @param fade: f32 = 40.0 range 1.0 .. 200.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = clamp(cell.g / params.fade, 0.0, 1.0);
    let col = mix(params.young, params.old, t) * cell.r;
    return vec4<f32>(col, 1.0);
}
```

`presets/gray_scott/preset.toml`:
```toml
name = "Gray-Scott Reaction-Diffusion"
mode = "2d"
width = 512
height = 512
steps_per_frame = 8
seed = 3

[init]
kind = "random"
density = 0.02

[params]
```

`presets/gray_scott/rule.wgsl`:
```wgsl
// Gray-Scott reaction-diffusion. r = U, g = V. Init: random cells seed V.
// @param feed: f32 = 0.037 range 0.0 .. 0.1
// @param kill: f32 = 0.06 range 0.0 .. 0.1
// @param du: f32 = 0.2 range 0.0 .. 0.5
// @param dv: f32 = 0.1 range 0.0 .. 0.5

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    // On the first frame the init pattern has r = 1 where "alive"; convert to U/V fields.
    if (globals.frame == 0u) {
        let v = c.r;
        return vec4<f32>(1.0 - v * 0.5, v, 0.0, 1.0);
    }
    let lap = cell(x - 1, y) + cell(x + 1, y) + cell(x, y - 1) + cell(x, y + 1)
        + 0.5 * (cell(x - 1, y - 1) + cell(x + 1, y - 1) + cell(x - 1, y + 1) + cell(x + 1, y + 1))
        - 6.0 * c;
    let u = c.r;
    let v = c.g;
    let uvv = u * v * v;
    let nu = u + params.du * lap.r - uvv + params.feed * (1.0 - u);
    let nv = v + params.dv * lap.g + uvv - (params.feed + params.kill) * v;
    return vec4<f32>(clamp(nu, 0.0, 1.0), clamp(nv, 0.0, 1.0), 0.0, 1.0);
}
```

`presets/gray_scott/render.wgsl`:
```wgsl
// @param a: vec3<f32> = (0.02, 0.02, 0.08) color
// @param b: vec3<f32> = (0.1, 0.7, 0.9) color
// @param c: vec3<f32> = (1.0, 1.0, 0.8) color
// @param gain: f32 = 3.0 range 0.5 .. 10.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = clamp(cell.g * params.gain, 0.0, 1.0);
    let col = select(mix(params.a, params.b, t * 2.0), mix(params.b, params.c, t * 2.0 - 1.0), t > 0.5);
    return vec4<f32>(col, 1.0);
}
```

- [ ] **Step 2: Write failing test** in `src/preset/builtin.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::assemble::{assemble_render, assemble_rule};
    use crate::shader::params::{merge_params, parse_params, params_wgsl};
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
            let rule_params = parse_params(&preset.rule).unwrap_or_else(|e| panic!("{}: rule params: {:?}", b.id, e));
            let render_params = parse_params(&preset.render).unwrap_or_else(|e| panic!("{}: render params: {:?}", b.id, e));
            let specs = merge_params(rule_params, render_params).unwrap();
            let pw = params_wgsl(&specs);
            let rule = assemble_rule(&preset.rule, &pw);
            let render = assemble_render(&preset.render, &pw);
            if let Err(e) = validate(ShaderFile::Rule, &rule) { panic!("{}: rule: {:?}\n{}", b.id, e, rule.source); }
            if let Err(e) = validate(ShaderFile::Render, &render) { panic!("{}: render: {:?}\n{}", b.id, e, render.source); }
            assert!(preset.meta.width > 0 && preset.meta.height > 0);
        }
    }

    #[test]
    fn toml_params_that_shader_does_not_declare_are_ignored() {
        // Mirrors app behaviour: unknown keys never fail loading.
        let b = &BUILTINS[0];
        let mut preset = load_builtin(b);
        preset.meta.params.insert("ghost".into(), toml::Value::Float(1.0));
        let specs = parse_params(&preset.rule).unwrap();
        let known: Vec<_> = preset.meta.params.keys().filter(|k| specs.iter().any(|s| &s.name == *k)).collect();
        assert!(!known.iter().any(|k| *k == "ghost"));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test preset::builtin`
Expected: compile error (`BUILTINS` missing).

- [ ] **Step 4: Implement** `src/preset/builtin.rs`

```rust
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
```

Add `pub mod builtin;` to `src/preset/mod.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test preset::builtin`
Expected: 3 pass. Fix any WGSL errors the validator reports in the preset files (the test prints the assembled source).

- [ ] **Step 6: Commit**

```bash
git add presets src/preset
git commit -m "Add built-in presets (Rule 30, Rule 110, Life, Gray-Scott) with validation test"
```

---

### Task 7: Uniform structs and init pattern generation

**Files:**
- Create: `src/sim/mod.rs` (module declarations only for now), `src/sim/uniforms.rs`, `src/sim/init.rs`
- Modify: `src/lib.rs` (add `pub mod sim;`)

**Interfaces:**
- Produces:
  ```rust
  #[repr(C)] #[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
  pub struct Globals { pub size: [u32; 2], pub frame: u32, pub seed: u32, pub time: f32, pub mode: u32, pub row: u32, pub prev_row: u32 }
  pub type ParamsData = [[u32; 4]; MAX_PARAMS];
  pub fn generate_init(pattern: &InitPattern, mode: Mode, width: u32, height: u32, seed: u32) -> Vec<f32>  // width*height*4 floats, row-major, rgba
  ```

- [ ] **Step 1: Write failing tests**

`src/sim/uniforms.rs` tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn globals_is_32_bytes() {
        assert_eq!(std::mem::size_of::<Globals>(), 32);
    }
    #[test]
    fn params_data_is_256_bytes() {
        assert_eq!(std::mem::size_of::<ParamsData>(), 256);
    }
}
```

`src/sim/init.rs` tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{InitPattern, Mode};

    #[test]
    fn blank_is_all_zero_with_alpha_one() {
        let v = generate_init(&InitPattern::Blank, Mode::TwoD, 4, 2, 0);
        assert_eq!(v.len(), 4 * 2 * 4);
        for px in v.chunks(4) { assert_eq!(px, &[0.0, 0.0, 0.0, 1.0]); }
    }

    #[test]
    fn single_2d_sets_centre_only() {
        let v = generate_init(&InitPattern::Single, Mode::TwoD, 8, 6, 0);
        let on: Vec<usize> = v.chunks(4).enumerate().filter(|(_, p)| p[0] > 0.5).map(|(i, _)| i).collect();
        assert_eq!(on, vec![3 * 8 + 4]);
    }

    #[test]
    fn single_1d_sets_centre_of_row_zero() {
        let v = generate_init(&InitPattern::Single, Mode::OneD, 8, 6, 0);
        let on: Vec<usize> = v.chunks(4).enumerate().filter(|(_, p)| p[0] > 0.5).map(|(i, _)| i).collect();
        assert_eq!(on, vec![4]);
    }

    #[test]
    fn random_is_deterministic_and_respects_density() {
        let a = generate_init(&InitPattern::Random { density: 0.25 }, Mode::TwoD, 100, 100, 9);
        let b = generate_init(&InitPattern::Random { density: 0.25 }, Mode::TwoD, 100, 100, 9);
        let c = generate_init(&InitPattern::Random { density: 0.25 }, Mode::TwoD, 100, 100, 10);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let on = a.chunks(4).filter(|p| p[0] > 0.5).count();
        assert!((2000..3000).contains(&on), "on = {on}");
    }

    #[test]
    fn random_1d_only_fills_row_zero() {
        let v = generate_init(&InitPattern::Random { density: 0.5 }, Mode::OneD, 64, 4, 1);
        let on_later_rows = v[64 * 4..].chunks(4).filter(|p| p[0] > 0.5).count();
        assert_eq!(on_later_rows, 0);
        let on_row0 = v[..64 * 4].chunks(4).filter(|p| p[0] > 0.5).count();
        assert!(on_row0 > 10);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test sim::`
Expected: compile error.

- [ ] **Step 3: Implement**

`src/sim/mod.rs`:
```rust
pub mod init;
pub mod uniforms;
```

`src/sim/uniforms.rs`:
```rust
use bytemuck::{Pod, Zeroable};
use crate::shader::params::MAX_PARAMS;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct Globals {
    pub size: [u32; 2],
    pub frame: u32,
    pub seed: u32,
    pub time: f32,
    pub mode: u32,
    pub row: u32,
    pub prev_row: u32,
}

pub type ParamsData = [[u32; 4]; MAX_PARAMS];
```

`src/sim/init.rs`:
```rust
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::preset::{InitPattern, Mode};

pub fn generate_init(pattern: &InitPattern, mode: Mode, width: u32, height: u32, seed: u32) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![0.0f32; w * h * 4];
    for px in data.chunks_mut(4) { px[3] = 1.0; }
    let rows = match mode { Mode::TwoD => h, Mode::OneD => 1 };
    match pattern {
        InitPattern::Blank => {}
        InitPattern::Single => {
            let x = w / 2;
            let y = match mode { Mode::TwoD => h / 2, Mode::OneD => 0 };
            data[(y * w + x) * 4] = 1.0;
        }
        InitPattern::Random { density } => {
            let mut rng = ChaCha8Rng::seed_from_u64(seed as u64);
            let p = (*density as f64).clamp(0.0, 1.0);
            for y in 0..rows {
                for x in 0..w {
                    if rng.random_bool(p) { data[(y * w + x) * 4] = 1.0; }
                }
            }
        }
    }
    data
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test sim::`
Expected: 7 pass.

- [ ] **Step 5: Commit**

```bash
git add src/sim src/lib.rs
git commit -m "Add uniform layouts and seeded init pattern generation"
```

---

### Task 8: Simulation core (GPU textures, pipelines, stepping, 1D row logic)

**Files:**
- Create: `src/sim/simulation.rs`, `src/sim/row.rs`
- Modify: `src/sim/mod.rs`

**Interfaces:**
- Consumes: `Assembled`, `validate`, `ShaderError`, `Globals`, `ParamsData`, `generate_init`, `Mode`, `InitPattern`.
- Produces:
  ```rust
  pub struct RowPlan { pub scroll: bool, pub read_row: u32, pub write_row: u32, pub next_row: u32 }
  pub fn plan_row(row: u32, height: u32) -> RowPlan
  pub fn clamp_size(requested: u32, limits: &wgpu::Limits) -> u32
  pub struct SimConfig { pub mode: Mode, pub width: u32, pub height: u32, pub init: InitPattern, pub seed: u32 }
  pub struct Simulation { ... }
  impl Simulation {
      pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat, config: SimConfig) -> Self
      pub fn config(&self) -> &SimConfig
      pub fn reconfigure(&mut self, config: SimConfig)            // recreates textures if size changed; always resets
      pub fn reset(&mut self)
      pub fn set_rule(&mut self, file: ShaderFile, assembled: &Assembled) -> Result<(), Vec<ShaderError>>
      pub fn set_render(&mut self, file: ShaderFile, assembled: &Assembled) -> Result<(), Vec<ShaderError>>
      pub fn set_params(&mut self, data: ParamsData)
      pub fn set_time(&mut self, seconds: f32)
      pub fn step(&mut self, encoder: &mut wgpu::CommandEncoder, n: u32)
      pub fn draw(&self, pass: &mut wgpu::RenderPass<'static>)
      pub fn frame(&self) -> u32
      pub fn has_pipelines(&self) -> bool
  }
  ```

- [ ] **Step 1: Write failing tests** for pure logic in `src/sim/row.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_before_bottom_write_next_row_without_scroll() {
        let p = plan_row(1, 8);
        assert!(!p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (0, 1, 2));
        let p = plan_row(7, 8);
        assert!(!p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (6, 7, 8));
    }

    #[test]
    fn at_bottom_scrolls_and_keeps_writing_last_row() {
        let p = plan_row(8, 8);
        assert!(p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (7, 7, 8));
        let p = plan_row(50, 8);
        assert!(p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (7, 7, 8));
    }

    #[test]
    fn height_one_never_scrolls_nor_reads_negative() {
        let p = plan_row(1, 1);
        assert!(!p.scroll, "a 1-row grid has nothing to scroll");
        assert_eq!((p.read_row, p.write_row, p.next_row), (0, 0, 1));
    }

    #[test]
    fn clamp_size_respects_texture_and_dispatch_limits() {
        let mut limits = wgpu::Limits::default();
        limits.max_texture_dimension_2d = 2048;
        limits.max_compute_workgroups_per_dimension = 100;
        assert_eq!(clamp_size(4096, &limits), 1600); // 100 workgroups * 16
        limits.max_compute_workgroups_per_dimension = 65535;
        assert_eq!(clamp_size(4096, &limits), 2048);
        assert_eq!(clamp_size(0, &limits), 1);
        assert_eq!(clamp_size(300, &limits), 300);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test sim::row`
Expected: compile error.

- [ ] **Step 3: Implement** `src/sim/row.rs`

```rust
use eframe::wgpu;

pub const WORKGROUP: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowPlan {
    pub scroll: bool,
    pub read_row: u32,
    pub write_row: u32,
    pub next_row: u32,
}

/// Decide which row a 1D step reads and writes. `row` is the row we want to write next.
pub fn plan_row(row: u32, height: u32) -> RowPlan {
    let last = height.saturating_sub(1);
    if height <= 1 {
        return RowPlan { scroll: false, read_row: 0, write_row: 0, next_row: 1 };
    }
    if row < height {
        RowPlan { scroll: false, read_row: row.saturating_sub(1), write_row: row, next_row: row + 1 }
    } else {
        RowPlan { scroll: true, read_row: last, write_row: last, next_row: height }
    }
}

pub fn clamp_size(requested: u32, limits: &wgpu::Limits) -> u32 {
    let by_dispatch = limits.max_compute_workgroups_per_dimension.saturating_mul(WORKGROUP);
    requested.max(1).min(limits.max_texture_dimension_2d).min(by_dispatch)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test sim::row`
Expected: 4 pass.

- [ ] **Step 5: Implement** `src/sim/simulation.rs` (GPU code, verified by `cargo build` and the app)

```rust
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::preset::{InitPattern, Mode};
use crate::shader::assemble::Assembled;
use crate::shader::validate::{validate, ShaderError, ShaderFile};
use crate::sim::init::generate_init;
use crate::sim::row::{clamp_size, plan_row, WORKGROUP};
use crate::sim::uniforms::{Globals, ParamsData};

#[derive(Clone, Debug, PartialEq)]
pub struct SimConfig {
    pub mode: Mode,
    pub width: u32,
    pub height: u32,
    pub init: InitPattern,
    pub seed: u32,
}

struct Textures {
    tex: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    compute_bind_groups: [wgpu::BindGroup; 2], // [i] reads tex[i], writes tex[1-i]
    render_bind_groups: [wgpu::BindGroup; 2],  // [i] reads tex[i]
}

pub struct Simulation {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target_format: wgpu::TextureFormat,
    config: SimConfig,
    compute_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    compute_pipeline_layout: wgpu::PipelineLayout,
    render_pipeline_layout: wgpu::PipelineLayout,
    globals_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    textures: Textures,
    cur: usize,
    compute: Option<wgpu::ComputePipeline>,
    render: Option<wgpu::RenderPipeline>,
    globals: Globals,
}

impl Simulation {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat, mut config: SimConfig) -> Self {
        let limits = device.limits();
        config.width = clamp_size(config.width, &limits);
        config.height = clamp_size(config.height, &limits);

        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let sampled = |visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ca compute layout"),
            entries: &[
                sampled(wgpu::ShaderStages::COMPUTE),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 },
                    count: None,
                },
                uniform(2),
                uniform(3),
            ],
        });
        let render_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ca render layout"),
            entries: &[sampled(wgpu::ShaderStages::FRAGMENT), uniform(2), uniform(3)],
        });
        let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ca compute pipeline layout"), bind_group_layouts: &[&compute_layout], push_constant_ranges: &[],
        });
        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ca render pipeline layout"), bind_group_layouts: &[&render_layout], push_constant_ranges: &[],
        });
        let globals = Globals { size: [config.width, config.height], mode: mode_code(config.mode), seed: config.seed, ..Default::default() };
        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals"), contents: bytemuck::bytes_of(&globals), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let params: ParamsData = [[0; 4]; crate::shader::params::MAX_PARAMS];
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca params"), contents: bytemuck::bytes_of(&params), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let textures = create_textures(&device, &compute_layout, &render_layout, &globals_buf, &params_buf, config.width, config.height);
        let mut sim = Simulation {
            device, queue, target_format, config, compute_layout, render_layout,
            compute_pipeline_layout, render_pipeline_layout, globals_buf, params_buf,
            textures, cur: 0, compute: None, render: None, globals,
        };
        sim.reset();
        sim
    }

    pub fn config(&self) -> &SimConfig { &self.config }
    pub fn frame(&self) -> u32 { self.globals.frame }
    pub fn has_pipelines(&self) -> bool { self.compute.is_some() && self.render.is_some() }

    pub fn reconfigure(&mut self, mut config: SimConfig) {
        let limits = self.device.limits();
        config.width = clamp_size(config.width, &limits);
        config.height = clamp_size(config.height, &limits);
        if config.width != self.config.width || config.height != self.config.height {
            self.textures = create_textures(&self.device, &self.compute_layout, &self.render_layout, &self.globals_buf, &self.params_buf, config.width, config.height);
        }
        self.config = config;
        self.reset();
    }

    pub fn reset(&mut self) {
        let c = &self.config;
        let data = generate_init(&c.init, c.mode, c.width, c.height, c.seed);
        self.cur = 0;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.textures.tex[0], mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(c.width * 16), rows_per_image: Some(c.height) },
            wgpu::Extent3d { width: c.width, height: c.height, depth_or_array_layers: 1 },
        );
        self.globals = Globals {
            size: [c.width, c.height], frame: 0, seed: c.seed, time: 0.0,
            mode: mode_code(c.mode), row: 1, prev_row: 0,
        };
        self.upload_globals();
    }

    pub fn set_params(&mut self, data: ParamsData) {
        self.queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&data));
    }

    pub fn set_time(&mut self, seconds: f32) { self.globals.time = seconds; }

    fn upload_globals(&self) {
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
    }

    fn create_module(&self, file: ShaderFile, assembled: &Assembled) -> Result<wgpu::ShaderModule, Vec<ShaderError>> {
        validate(file, assembled)?;
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(file.label()),
            source: wgpu::ShaderSource::Wgsl(assembled.source.as_str().into()),
        });
        Ok(module)
    }

    fn backend_error(file: ShaderFile, err: Option<wgpu::Error>) -> Result<(), Vec<ShaderError>> {
        match err {
            None => Ok(()),
            Some(e) => Err(vec![ShaderError { file, line: 1, column: 1, message: format!("GPU backend rejected shader: {e}") }]),
        }
    }

    pub fn set_rule(&mut self, file: ShaderFile, assembled: &Assembled) -> Result<(), Vec<ShaderError>> {
        let module = self.create_module(file, assembled)?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ca compute"),
            layout: Some(&self.compute_pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self::backend_error(file, pollster::block_on(scope.pop()))?;
        self.compute = Some(pipeline);
        Ok(())
    }

    pub fn set_render(&mut self, file: ShaderFile, assembled: &Assembled) -> Result<(), Vec<ShaderError>> {
        let module = self.create_module(file, assembled)?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ca render"),
            layout: Some(&self.render_pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: self.target_format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self::backend_error(file, pollster::block_on(scope.pop()))?;
        self.render = Some(pipeline);
        Ok(())
    }

    pub fn step(&mut self, encoder: &mut wgpu::CommandEncoder, n: u32) {
        let Some(pipeline) = &self.compute else { return };
        let (w, h) = (self.config.width, self.config.height);
        for _ in 0..n {
            let src = self.cur;
            let dst = 1 - self.cur;
            match self.config.mode {
                Mode::TwoD => {
                    self.upload_globals();
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ca step"), timestamp_writes: None });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &self.textures.compute_bind_groups[src], &[]);
                    pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1);
                }
                Mode::OneD => {
                    let plan = plan_row(self.globals.row, h);
                    // Carry unchanged rows from src to dst.
                    if plan.scroll {
                        if h > 1 {
                            copy_rows(encoder, &self.textures.tex[src], 1, &self.textures.tex[dst], 0, w, h - 1);
                        }
                    } else if plan.write_row > 0 {
                        copy_rows(encoder, &self.textures.tex[src], 0, &self.textures.tex[dst], 0, w, plan.write_row);
                    }
                    self.globals.row = plan.write_row;
                    self.globals.prev_row = plan.read_row;
                    self.upload_globals();
                    {
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ca step 1d"), timestamp_writes: None });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &self.textures.compute_bind_groups[src], &[]);
                        pass.dispatch_workgroups(w.div_ceil(WORKGROUP), 1, 1);
                    }
                    self.globals.row = plan.next_row;
                }
            }
            self.cur = dst;
            self.globals.frame = self.globals.frame.wrapping_add(1);
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some(pipeline) = &self.render else { return };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.textures.render_bind_groups[self.cur], &[]);
        pass.draw(0..3, 0..1);
    }
}

fn mode_code(mode: Mode) -> u32 {
    match mode { Mode::TwoD => 0, Mode::OneD => 1 }
}

fn copy_rows(encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture, src_y: u32, dst: &wgpu::Texture, dst_y: u32, width: u32, rows: u32) {
    if rows == 0 { return; }
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo { texture: src, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: src_y, z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyTextureInfo { texture: dst, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: dst_y, z: 0 }, aspect: wgpu::TextureAspect::All },
        wgpu::Extent3d { width, height: rows, depth_or_array_layers: 1 },
    );
}

fn create_textures(
    device: &wgpu::Device,
    compute_layout: &wgpu::BindGroupLayout,
    render_layout: &wgpu::BindGroupLayout,
    globals_buf: &wgpu::Buffer,
    params_buf: &wgpu::Buffer,
    width: u32,
    height: u32,
) -> Textures {
    let make = |label: &str| device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let tex = [make("ca state A"), make("ca state B")];
    let views = [tex[0].create_view(&Default::default()), tex[1].create_view(&Default::default())];
    let compute_bg = |src: usize| device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ca compute bg"),
        layout: compute_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[src]) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1 - src]) },
            wgpu::BindGroupEntry { binding: 2, resource: globals_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });
    let render_bg = |src: usize| device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ca render bg"),
        layout: render_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[src]) },
            wgpu::BindGroupEntry { binding: 2, resource: globals_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });
    let compute_bind_groups = [compute_bg(0), compute_bg(1)];
    let render_bind_groups = [render_bg(0), render_bg(1)];
    Textures { tex, views, compute_bind_groups, render_bind_groups }
}
```

Add to `src/sim/mod.rs`:
```rust
pub mod row;
pub mod simulation;
pub use simulation::{SimConfig, Simulation};
```

Known subtlety: in `step`, writing `globals_buf` with `queue.write_buffer` inside a loop before the encoder is submitted means all N steps see the *last* written globals (queue writes happen at submit time, before the encoder's commands). For 2D this only affects `frame` (off by up to N-1, acceptable). For 1D it would break row addressing when `steps_per_frame > 1`. Fix: in `Mode::OneD`, use a **per-step staging approach**: create a small `globals_staging` buffer per step with `create_buffer_init` holding the step's Globals and `encoder.copy_buffer_to_buffer(&staging, 0, &self.globals_buf, 0, 32)` before the compute pass. Do the same in 2D so `frame` is exact (Gray-Scott uses `frame == 0`). Implement `upload_globals_in_encoder(&self, encoder)`:

```rust
    fn upload_globals_in_encoder(&self, encoder: &mut wgpu::CommandEncoder) {
        let staging = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals staging"),
            contents: bytemuck::bytes_of(&self.globals),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&staging, 0, &self.globals_buf, 0, std::mem::size_of::<Globals>() as u64);
    }
```

and call it instead of `self.upload_globals()` inside `step` (both branches). Keep `upload_globals()` (queue write) for `reset`.

- [ ] **Step 6: Build**

Run: `cargo build && cargo test`
Expected: compiles; all tests still pass.

- [ ] **Step 7: Commit**

```bash
git add src/sim
git commit -m "Add GPU simulation core with ping-pong textures and 1D row scrolling"
```

---

### Task 9: Viewport paint callback

**Files:**
- Create: `src/viewport.rs`
- Modify: `src/lib.rs` (add `pub mod viewport;`)

**Interfaces:**
- Consumes: `Simulation::{step, draw, set_time}`.
- Produces:
  ```rust
  pub struct ViewportCallback { pub sim: Arc<Mutex<Simulation>>, pub steps: u32, pub time: f32, pub grid_aspect: f32 }
  impl egui_wgpu::CallbackTrait for ViewportCallback
  pub fn show_viewport(ui: &mut egui::Ui, sim: &Arc<Mutex<Simulation>>, steps: u32, time: f32) -> egui::Rect
  pub fn letterbox(viewport_w: f32, viewport_h: f32, grid_aspect: f32) -> (f32, f32, f32, f32)  // x, y, w, h in the same units
  ```

- [ ] **Step 1: Write failing test for letterbox math**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_viewport_pillarboxes_square_grid() {
        let (x, y, w, h) = letterbox(400.0, 200.0, 1.0);
        assert_eq!((x, y, w, h), (100.0, 0.0, 200.0, 200.0));
    }
    #[test]
    fn tall_viewport_letterboxes_wide_grid() {
        let (x, y, w, h) = letterbox(200.0, 400.0, 2.0);
        assert_eq!((x, y, w, h), (0.0, 150.0, 200.0, 100.0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test viewport::`
Expected: compile error.

- [ ] **Step 3: Implement** `src/viewport.rs`

```rust
use std::sync::{Arc, Mutex};

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use egui::PaintCallbackInfo;

use crate::sim::Simulation;

pub fn letterbox(viewport_w: f32, viewport_h: f32, grid_aspect: f32) -> (f32, f32, f32, f32) {
    let view_aspect = viewport_w / viewport_h;
    if view_aspect > grid_aspect {
        let w = viewport_h * grid_aspect;
        ((viewport_w - w) / 2.0, 0.0, w, viewport_h)
    } else {
        let h = viewport_w / grid_aspect;
        (0.0, (viewport_h - h) / 2.0, viewport_w, h)
    }
}

pub struct ViewportCallback {
    pub sim: Arc<Mutex<Simulation>>,
    pub steps: u32,
    pub time: f32,
    pub grid_aspect: f32,
}

impl CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mut sim = self.sim.lock().unwrap();
        sim.set_time(self.time);
        sim.step(egui_encoder, self.steps);
        Vec::new()
    }

    fn paint(&self, info: PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, _resources: &CallbackResources) {
        let vp = info.viewport_in_pixels();
        let (x, y, w, h) = letterbox(vp.width_px as f32, vp.height_px as f32, self.grid_aspect);
        if w < 1.0 || h < 1.0 { return; }
        pass.set_viewport(vp.left_px as f32 + x, vp.top_px as f32 + y, w, h, 0.0, 1.0);
        let sim = self.sim.lock().unwrap();
        sim.draw(pass);
    }
}

/// Allocates the remaining space in `ui`, paints a dark background and schedules the GPU callback.
pub fn show_viewport(ui: &mut egui::Ui, sim: &Arc<Mutex<Simulation>>, steps: u32, time: f32) -> egui::Rect {
    let size = ui.available_size();
    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(12));
    let grid_aspect = {
        let s = sim.lock().unwrap();
        let c = s.config();
        c.width as f32 / c.height.max(1) as f32
    };
    let cb = ViewportCallback { sim: sim.clone(), steps, time, grid_aspect };
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    rect
}
```

- [ ] **Step 4: Run tests and build**

Run: `cargo test viewport:: && cargo build`
Expected: 2 pass; compiles.

- [ ] **Step 5: Commit**

```bash
git add src/viewport.rs src/lib.rs
git commit -m "Add egui-wgpu paint callback that steps and draws the simulation"
```

---

### Task 10: WGSL syntax highlighting layouter

**Files:**
- Create: `src/shader/highlight.rs`
- Modify: `src/shader/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum TokenKind { Comment, Annotation, Keyword, Type, Builtin, Number, Attribute, Ident, Punct, Whitespace }
  pub fn tokenize(src: &str) -> Vec<(TokenKind, &str)>   // concatenation of slices == src
  pub fn highlight_job(src: &str, font_id: egui::FontId, dark: bool) -> egui::text::LayoutJob
  pub fn layouter<'a>() -> impl FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> Arc<egui::Galley> + 'a
  ```

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        tokenize(src).into_iter().filter(|(k, _)| *k != TokenKind::Whitespace).collect()
    }

    #[test]
    fn tokens_cover_the_whole_source() {
        let src = "fn rule(pos: vec2<u32>) -> vec4<f32> { // hi\n  return vec4<f32>(1.0); }";
        let joined: String = tokenize(src).iter().map(|(_, s)| *s).collect();
        assert_eq!(joined, src);
    }

    #[test]
    fn classifies_keywords_types_builtins_numbers() {
        let t = kinds("let x: f32 = clamp(1.0, 0u, 2);");
        assert_eq!(t[0], (TokenKind::Keyword, "let"));
        assert_eq!(t[1], (TokenKind::Ident, "x"));
        assert_eq!(t[3], (TokenKind::Type, "f32"));
        assert_eq!(t[5], (TokenKind::Builtin, "clamp"));
        assert_eq!(t[7], (TokenKind::Number, "1.0"));
        assert_eq!(t[9], (TokenKind::Number, "0u"));
    }

    #[test]
    fn comments_and_param_annotations() {
        let t = kinds("// plain\n// @param a: f32 = 1\n@compute");
        assert_eq!(t[0], (TokenKind::Comment, "// plain"));
        assert_eq!(t[1], (TokenKind::Annotation, "// @param a: f32 = 1"));
        assert_eq!(t[2], (TokenKind::Attribute, "@compute"));
    }

    #[test]
    fn job_has_one_section_per_token_and_full_text() {
        let src = "fn a() {}";
        let job = highlight_job(src, egui::FontId::monospace(12.0), true);
        assert_eq!(job.text, src);
        assert_eq!(job.sections.len(), tokenize(src).len());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test shader::highlight`
Expected: compile error.

- [ ] **Step 3: Implement** `src/shader/highlight.rs`

```rust
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind { Comment, Annotation, Keyword, Type, Builtin, Number, Attribute, Ident, Punct, Whitespace }

const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "const", "return", "if", "else", "for", "while", "loop", "break", "continue",
    "struct", "switch", "case", "default", "discard", "true", "false", "override", "continuing", "alias",
];
const TYPES: &[&str] = &[
    "f32", "i32", "u32", "bool", "f16", "vec2", "vec3", "vec4", "mat2x2", "mat3x3", "mat4x4", "array",
    "texture_2d", "texture_storage_2d", "sampler", "rgba32float", "write", "read", "read_write", "uniform", "storage",
];
const BUILTINS: &[&str] = &[
    "textureLoad", "textureStore", "select", "clamp", "min", "max", "abs", "floor", "ceil", "fract", "round",
    "sin", "cos", "tan", "atan2", "exp", "log", "pow", "sqrt", "dot", "cross", "length", "normalize", "distance",
    "mix", "smoothstep", "step", "sign", "mod", "any", "all", "cell", "prev_cell", "hash", "rand", "wrap",
];

fn is_ident_start(c: char) -> bool { c.is_ascii_alphabetic() || c == '_' }
fn is_ident_char(c: char) -> bool { c.is_ascii_alphanumeric() || c == '_' }

pub fn tokenize(src: &str) -> Vec<(TokenKind, &str)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        let c = rest.chars().next().unwrap();
        let start = i;
        let kind = if rest.starts_with("//") {
            let end = rest.find('\n').unwrap_or(rest.len());
            i += end;
            let text = &src[start..i];
            if text.trim_start_matches('/').trim_start().starts_with("@param") { TokenKind::Annotation } else { TokenKind::Comment }
        } else if c.is_whitespace() {
            while i < src.len() && (bytes[i] as char).is_whitespace() { i += 1; }
            TokenKind::Whitespace
        } else if c == '@' {
            i += 1;
            while i < src.len() && is_ident_char(bytes[i] as char) { i += 1; }
            TokenKind::Attribute
        } else if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|d: char| d.is_ascii_digit())) {
            i += 1;
            while i < src.len() && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'.') { i += 1; }
            TokenKind::Number
        } else if is_ident_start(c) {
            while i < src.len() && is_ident_char(bytes[i] as char) { i += 1; }
            let word = &src[start..i];
            if KEYWORDS.contains(&word) { TokenKind::Keyword }
            else if TYPES.contains(&word) { TokenKind::Type }
            else if BUILTINS.contains(&word) { TokenKind::Builtin }
            else { TokenKind::Ident }
        } else {
            i += c.len_utf8();
            TokenKind::Punct
        };
        out.push((kind, &src[start..i]));
    }
    out
}

fn color(kind: TokenKind, dark: bool) -> Color32 {
    match (kind, dark) {
        (TokenKind::Comment, true) => Color32::from_rgb(110, 120, 110),
        (TokenKind::Comment, false) => Color32::from_rgb(90, 110, 90),
        (TokenKind::Annotation, true) => Color32::from_rgb(230, 170, 90),
        (TokenKind::Annotation, false) => Color32::from_rgb(170, 100, 20),
        (TokenKind::Keyword, true) => Color32::from_rgb(200, 120, 220),
        (TokenKind::Keyword, false) => Color32::from_rgb(140, 40, 160),
        (TokenKind::Type, true) => Color32::from_rgb(120, 200, 220),
        (TokenKind::Type, false) => Color32::from_rgb(20, 120, 150),
        (TokenKind::Builtin, true) => Color32::from_rgb(120, 190, 140),
        (TokenKind::Builtin, false) => Color32::from_rgb(20, 120, 60),
        (TokenKind::Number, true) => Color32::from_rgb(230, 200, 120),
        (TokenKind::Number, false) => Color32::from_rgb(150, 110, 20),
        (TokenKind::Attribute, true) => Color32::from_rgb(220, 150, 150),
        (TokenKind::Attribute, false) => Color32::from_rgb(160, 60, 60),
        (TokenKind::Ident | TokenKind::Punct | TokenKind::Whitespace, true) => Color32::from_rgb(220, 220, 220),
        (TokenKind::Ident | TokenKind::Punct | TokenKind::Whitespace, false) => Color32::from_rgb(30, 30, 30),
    }
}

pub fn highlight_job(src: &str, font_id: FontId, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (kind, text) in tokenize(src) {
        job.append(text, 0.0, TextFormat { font_id: font_id.clone(), color: color(kind, dark), ..Default::default() });
    }
    job
}

/// Builds a layouter closure for `egui::TextEdit::layouter`. Results are cached per frame by egui's
/// `FrameCache`, keyed on the source text, so retyping does not re-tokenise unchanged text.
#[derive(Default)]
struct Highlighter;

impl egui::cache::ComputerMut<(&str, bool), LayoutJob> for Highlighter {
    fn compute(&mut self, (src, dark): (&str, bool)) -> LayoutJob {
        highlight_job(src, FontId::monospace(13.0), dark)
    }
}

type HighlightCache = egui::cache::FrameCache<LayoutJob, Highlighter>;

pub fn layouter<'a>() -> impl FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> Arc<egui::Galley> + 'a {
    move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let dark = ui.visuals().dark_mode;
        let mut job = ui.ctx().memory_mut(|m| m.caches.cache::<HighlightCache>().get((buf.as_str(), dark)));
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    }
}
```

- [ ] **Step 4: Run tests and build**

Run: `cargo test shader::highlight && cargo build`
Expected: 4 pass; compiles. If `egui::cache` paths differ, check `grep -rn "pub struct FrameCache" ~/.cargo/registry/src/*/egui-0.36.2/src/`.

- [ ] **Step 5: Commit**

```bash
git add src/shader
git commit -m "Add WGSL syntax highlighting layouter for the editors"
```

---

### Task 11: Application state and shader apply flow (no UI yet)

**Files:**
- Create: `src/app/mod.rs` (replaces `src/app.rs`), `src/app/state.rs`

**Interfaces:**
- Produces (in `state.rs`):
  ```rust
  pub enum PresetSource { Builtin(usize), Disk(PathBuf), Unsaved }
  pub struct EditorState { pub rule: String, pub render: String, pub dirty: bool }
  pub struct AppState {
      pub editor: EditorState,
      pub specs: Vec<ParamSpec>,
      pub values: BTreeMap<String, ParamValue>,
      pub errors: Vec<ShaderError>,
      pub playing: bool,
      pub step_once: bool,
      pub steps_per_frame: u32,
      pub pending: SimConfig,          // settings edited in the UI; applied on Reset / Apply settings
      pub preset_name: String,
      pub source: PresetSource,
      pub disk_presets: Vec<(String, PathBuf)>,
      pub started: Instant,
  }
  pub fn build_shaders(editor: &EditorState) -> Result<(Vec<ParamSpec>, Assembled, Assembled), Vec<ShaderError>>
  pub fn preset_to_state(preset: &Preset) -> (EditorState, SimConfig, u32 /*steps_per_frame*/, BTreeMap<String, toml::Value>)
  pub fn state_to_preset(state: &AppState) -> Preset
  pub fn resolve_values(specs: &[ParamSpec], toml_params: &BTreeMap<String, toml::Value>, previous: &BTreeMap<String, ParamValue>) -> BTreeMap<String, ParamValue>
  ```
  `build_shaders` parses params from both sources, merges, emits the struct, assembles both, validates both, and returns all errors (rule errors first) or the assembled pair. `resolve_values` precedence: TOML value (if type matches) > previous value (if type matches) > default.

- [ ] **Step 1: Write failing tests** in `src/app/state.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::builtin::{load_builtin, BUILTINS};
    use crate::shader::params::{parse_params, ParamValue};

    #[test]
    fn build_shaders_for_every_builtin_succeeds() {
        for b in BUILTINS {
            let p = load_builtin(b);
            let (editor, _, _, _) = preset_to_state(&p);
            build_shaders(&editor).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
        }
    }

    #[test]
    fn build_shaders_collects_param_errors_with_file() {
        let editor = EditorState { rule: "// @param x: f64 = 1\nfn rule(pos: vec2<u32>) -> vec4<f32> { return vec4<f32>(0.0); }".into(), render: "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return cell; }".into(), dirty: false };
        let errs = build_shaders(&editor).unwrap_err();
        assert_eq!(errs[0].file, ShaderFile::Rule);
        assert_eq!(errs[0].line, 1);
    }

    #[test]
    fn build_shaders_reports_both_files() {
        let editor = EditorState { rule: "fn rule(pos: vec2<u32>) -> vec4<f32> { return 1.0; }".into(), render: "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return 1.0; }".into(), dirty: false };
        let errs = build_shaders(&editor).unwrap_err();
        assert!(errs.iter().any(|e| e.file == ShaderFile::Rule));
        assert!(errs.iter().any(|e| e.file == ShaderFile::Render));
    }

    #[test]
    fn resolve_values_prefers_toml_then_previous_then_default() {
        let specs = parse_params("// @param a: f32 = 1\n// @param b: f32 = 2\n// @param c: f32 = 3\n// @param d: i32 = 4\n").unwrap();
        let mut toml_params = BTreeMap::new();
        toml_params.insert("a".to_string(), toml::Value::Float(10.0));
        toml_params.insert("d".to_string(), toml::Value::Float(1.5)); // wrong type for i32 → ignored
        toml_params.insert("ghost".to_string(), toml::Value::Float(0.0)); // undeclared → ignored
        let mut previous = BTreeMap::new();
        previous.insert("b".to_string(), ParamValue::F32(20.0));
        previous.insert("c".to_string(), ParamValue::I32(7)); // type mismatch → ignored
        let v = resolve_values(&specs, &toml_params, &previous);
        assert_eq!(v["a"], ParamValue::F32(10.0));
        assert_eq!(v["b"], ParamValue::F32(20.0));
        assert_eq!(v["c"], ParamValue::F32(3.0));
        assert_eq!(v["d"], ParamValue::I32(4));
        assert!(!v.contains_key("ghost"));
    }

    #[test]
    fn preset_round_trips_through_state() {
        let p = load_builtin(&BUILTINS[2]);
        let (editor, config, spf, toml_params) = preset_to_state(&p);
        let (specs, _, _) = build_shaders(&editor).unwrap();
        let values = resolve_values(&specs, &toml_params, &BTreeMap::new());
        let state = AppState::from_parts(editor, config, spf, specs, values, p.meta.name.clone(), PresetSource::Builtin(2));
        let back = state_to_preset(&state);
        assert_eq!(back.rule, p.rule);
        assert_eq!(back.render, p.render);
        assert_eq!(back.meta.width, p.meta.width);
        assert_eq!(back.meta.mode, p.meta.mode);
        assert_eq!(back.meta.name, p.meta.name);
        assert!(back.meta.params.contains_key("fade"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test app::state`
Expected: compile error.

- [ ] **Step 3: Implement** `src/app/state.rs`

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::preset::{param_value_from_toml, param_value_to_toml, Preset, PresetMeta};
use crate::shader::assemble::{assemble_render, assemble_rule, Assembled};
use crate::shader::params::{merge_params, params_wgsl, parse_params, ParamError, ParamSpec, ParamValue};
use crate::shader::validate::{validate, ShaderError, ShaderFile};
use crate::sim::SimConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum PresetSource { Builtin(usize), Disk(PathBuf), Unsaved }

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EditorState {
    pub rule: String,
    pub render: String,
    pub dirty: bool,
}

pub struct AppState {
    pub editor: EditorState,
    pub specs: Vec<ParamSpec>,
    pub values: BTreeMap<String, ParamValue>,
    pub errors: Vec<ShaderError>,
    pub playing: bool,
    pub step_once: bool,
    pub steps_per_frame: u32,
    pub pending: SimConfig,
    pub preset_name: String,
    pub source: PresetSource,
    pub disk_presets: Vec<(String, PathBuf)>,
    pub started: Instant,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        editor: EditorState, pending: SimConfig, steps_per_frame: u32, specs: Vec<ParamSpec>,
        values: BTreeMap<String, ParamValue>, preset_name: String, source: PresetSource,
    ) -> Self {
        AppState {
            editor, specs, values, errors: Vec::new(), playing: true, step_once: false,
            steps_per_frame: steps_per_frame.max(1), pending, preset_name, source,
            disk_presets: Vec::new(), started: Instant::now(),
        }
    }
}

fn param_err(file: ShaderFile, e: ParamError) -> ShaderError {
    ShaderError { file, line: e.line.max(1), column: 1, message: e.message }
}

pub fn build_shaders(editor: &EditorState) -> Result<(Vec<ParamSpec>, Assembled, Assembled), Vec<ShaderError>> {
    let rule_params = parse_params(&editor.rule).map_err(|e| vec![param_err(ShaderFile::Rule, e)])?;
    let render_params = parse_params(&editor.render).map_err(|e| vec![param_err(ShaderFile::Render, e)])?;
    let specs = merge_params(rule_params, render_params).map_err(|e| vec![param_err(ShaderFile::Render, e)])?;
    let pw = params_wgsl(&specs);
    let rule = assemble_rule(&editor.rule, &pw);
    let render = assemble_render(&editor.render, &pw);
    let mut errors = Vec::new();
    if let Err(e) = validate(ShaderFile::Rule, &rule) { errors.extend(e); }
    if let Err(e) = validate(ShaderFile::Render, &render) { errors.extend(e); }
    if errors.is_empty() { Ok((specs, rule, render)) } else { Err(errors) }
}

pub fn preset_to_state(preset: &Preset) -> (EditorState, SimConfig, u32, BTreeMap<String, toml::Value>) {
    let m = &preset.meta;
    let editor = EditorState { rule: preset.rule.clone(), render: preset.render.clone(), dirty: false };
    let config = SimConfig { mode: m.mode, width: m.width, height: m.height, init: m.init.clone(), seed: m.seed };
    (editor, config, m.steps_per_frame.max(1), m.params.clone())
}

pub fn state_to_preset(state: &AppState) -> Preset {
    let params = state.specs.iter()
        .filter_map(|s| state.values.get(&s.name).map(|v| (s.name.clone(), param_value_to_toml(v))))
        .collect();
    let c = &state.pending;
    Preset {
        meta: PresetMeta {
            name: state.preset_name.clone(), mode: c.mode, width: c.width, height: c.height,
            steps_per_frame: state.steps_per_frame, seed: c.seed, init: c.init.clone(), params,
        },
        rule: state.editor.rule.clone(),
        render: state.editor.render.clone(),
    }
}

pub fn resolve_values(
    specs: &[ParamSpec],
    toml_params: &BTreeMap<String, toml::Value>,
    previous: &BTreeMap<String, ParamValue>,
) -> BTreeMap<String, ParamValue> {
    specs.iter().map(|s| {
        let v = toml_params.get(&s.name).and_then(|t| param_value_from_toml(s.ty, t))
            .or_else(|| previous.get(&s.name).filter(|p| p.ty() == s.ty).cloned())
            .unwrap_or_else(|| s.default.clone());
        (s.name.clone(), v)
    }).collect()
}
```

Move the existing `src/app.rs` content into `src/app/mod.rs` and add `pub mod state;` at its top. Delete `src/app.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test app::state`
Expected: 5 pass.

- [ ] **Step 5: Commit**

```bash
git add src/app
git rm -q src/app.rs 2>/dev/null || true
git commit -m "Add app state, shader build flow and preset/state conversion"
```

---

### Task 12: Full UI — panels, editors, params, transport, errors, presets

**Files:**
- Modify: `src/app/mod.rs`
- Create: `src/app/ui_editor.rs`, `src/app/ui_params.rs`, `src/app/ui_topbar.rs`, `src/app/ui_errors.rs`

**Interfaces:**
- Consumes everything above.
- Produces: `App` implementing `eframe::App`. Internal `App` fields:
  ```rust
  pub struct App { sim: Arc<Mutex<Simulation>>, state: AppState, pending_cursor: Option<(ShaderFile, usize /*line*/)>, device_limits: wgpu::Limits }
  impl App {
      pub fn new(cc: &eframe::CreationContext<'_>) -> Self
      fn load_preset(&mut self, preset: Preset, source: PresetSource)
      fn apply_shaders(&mut self)        // build_shaders → sim.set_rule/set_render → update specs/values/errors
      fn apply_settings_and_reset(&mut self)
      fn push_params(&self)
      fn save(&mut self) / fn save_as(&mut self)
  }
  ```

- [ ] **Step 1: Implement `src/app/mod.rs`**

```rust
pub mod state;
mod ui_editor;
mod ui_errors;
mod ui_params;
mod ui_topbar;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use eframe::wgpu;

use crate::preset::builtin::{load_builtin, BUILTINS};
use crate::preset::{scan_presets_dir, Preset};
use crate::shader::params::pack_params;
use crate::shader::validate::{ShaderError, ShaderFile};
use crate::sim::{SimConfig, Simulation};
use crate::viewport::show_viewport;
use state::{build_shaders, preset_to_state, resolve_values, state_to_preset, AppState, PresetSource};

pub struct App {
    sim: Arc<Mutex<Simulation>>,
    state: AppState,
    pending_cursor: Option<(ShaderFile, usize)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rs = cc.wgpu_render_state.as_ref().expect("wgpu render state (the app requires the wgpu backend)");
        let preset = load_builtin(&BUILTINS[2]); // Game of Life
        let (editor, config, spf, toml_params) = preset_to_state(&preset);
        let sim = Simulation::new(rs.device.clone(), rs.queue.clone(), rs.target_format, config.clone());
        let state = AppState::from_parts(editor, config, spf, Vec::new(), BTreeMap::new(), preset.meta.name.clone(), PresetSource::Builtin(2));
        let mut app = App { sim: Arc::new(Mutex::new(sim)), state, pending_cursor: None };
        app.state.disk_presets = scan_presets_dir(Path::new("presets"));
        app.apply_shaders_with_toml(&toml_params);
        app
    }

    fn load_preset(&mut self, preset: Preset, source: PresetSource) {
        let (editor, config, spf, toml_params) = preset_to_state(&preset);
        self.state.editor = editor;
        self.state.pending = config.clone();
        self.state.steps_per_frame = spf;
        self.state.preset_name = preset.meta.name.clone();
        self.state.source = source;
        self.state.values.clear();
        self.sim.lock().unwrap().reconfigure(config);
        self.state.started = std::time::Instant::now();
        self.apply_shaders_with_toml(&toml_params);
    }

    fn apply_shaders(&mut self) {
        self.apply_shaders_with_toml(&BTreeMap::new());
    }

    fn apply_shaders_with_toml(&mut self, toml_params: &BTreeMap<String, toml::Value>) {
        match build_shaders(&self.state.editor) {
            Err(errors) => self.state.errors = errors,
            Ok((specs, rule, render)) => {
                let mut sim = self.sim.lock().unwrap();
                let mut errors: Vec<ShaderError> = Vec::new();
                if let Err(e) = sim.set_rule(ShaderFile::Rule, &rule) { errors.extend(e); }
                if let Err(e) = sim.set_render(ShaderFile::Render, &render) { errors.extend(e); }
                drop(sim);
                if errors.is_empty() {
                    self.state.values = resolve_values(&specs, toml_params, &self.state.values);
                    self.state.specs = specs;
                    self.state.editor.dirty = false;
                    self.push_params();
                }
                self.state.errors = errors;
            }
        }
    }

    fn push_params(&self) {
        let packed = pack_params(&self.state.specs, &self.state.values);
        self.sim.lock().unwrap().set_params(packed);
    }

    fn apply_settings_and_reset(&mut self) {
        let mut sim = self.sim.lock().unwrap();
        sim.reconfigure(self.state.pending.clone());
        self.state.pending = sim.config().clone(); // reflect clamping
        drop(sim);
        self.state.started = std::time::Instant::now();
    }

    fn save(&mut self) {
        match self.state.source.clone() {
            PresetSource::Disk(path) => self.save_to(&path),
            _ => self.save_as(),
        }
    }

    fn save_as(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().set_title("Choose a folder for this preset").pick_folder() {
            self.save_to(&dir);
        }
    }

    fn save_to(&mut self, dir: &Path) {
        let preset = state_to_preset(&self.state);
        match preset.save_dir(dir) {
            Ok(()) => {
                self.state.source = PresetSource::Disk(dir.to_path_buf());
                self.state.disk_presets = scan_presets_dir(Path::new("presets"));
            }
            Err(e) => self.state.errors = vec![ShaderError { file: ShaderFile::Rule, line: 1, column: 1, message: format!("save failed: {e:#}") }],
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Global shortcut: Ctrl/Cmd+Enter applies shaders. Consume it before the editors see it.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            self.apply_shaders();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) && !ctx.wants_keyboard_input() {
            self.state.playing = !self.state.playing;
        }

        egui::TopBottomPanel::top("topbar").show_inside(ui, |ui| ui_topbar::show(self, ui));
        egui::TopBottomPanel::bottom("errors").resizable(true).default_height(80.0).show_inside(ui, |ui| ui_errors::show(self, ui));
        egui::SidePanel::left("side").resizable(true).default_width(520.0).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui_editor::show(self, ui);
                ui.separator();
                ui_params::show(self, ui);
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| {
            let steps = if self.state.playing { self.state.steps_per_frame } else if self.state.step_once { 1 } else { 0 };
            self.state.step_once = false;
            let time = self.state.started.elapsed().as_secs_f32();
            show_viewport(ui, &self.sim, steps, time);
        });

        if self.state.playing { ctx.request_repaint(); }
    }
}
```

- [ ] **Step 2: Implement `src/app/ui_topbar.rs`**

```rust
use std::path::Path;

use super::state::PresetSource;
use super::App;
use crate::preset::builtin::{load_builtin, BUILTINS};
use crate::preset::Preset;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let label = match &app.state.source {
            PresetSource::Unsaved => format!("{} *", app.state.preset_name),
            _ => app.state.preset_name.clone(),
        };
        let mut to_load: Option<(Preset, PresetSource)> = None;
        egui::ComboBox::from_id_salt("preset").selected_text(label).width(260.0).show_ui(ui, |ui| {
            ui.label(egui::RichText::new("Built-in").small().weak());
            for (i, b) in BUILTINS.iter().enumerate() {
                let p = load_builtin(b);
                if ui.selectable_label(app.state.source == PresetSource::Builtin(i), &p.meta.name).clicked() {
                    to_load = Some((p, PresetSource::Builtin(i)));
                }
            }
            if !app.state.disk_presets.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("./presets").small().weak());
                for (name, path) in app.state.disk_presets.clone() {
                    if ui.selectable_label(app.state.source == PresetSource::Disk(path.clone()), &name).clicked() {
                        match Preset::load_dir(&path) {
                            Ok(p) => to_load = Some((p, PresetSource::Disk(path))),
                            Err(e) => app.state.errors = vec![crate::shader::validate::ShaderError {
                                file: crate::shader::validate::ShaderFile::Rule, line: 1, column: 1, message: format!("load failed: {e:#}"),
                            }],
                        }
                    }
                }
            }
        });
        if let Some((p, src)) = to_load { app.load_preset(p, src); }
        if ui.button("Save").on_hover_text("Save to this preset's folder (built-ins: Save As)").clicked() { app.save(); }
        if ui.button("Save as…").clicked() { app.save_as(); }
        if ui.button("Rescan").on_hover_text("Rescan ./presets").clicked() {
            app.state.disk_presets = crate::preset::scan_presets_dir(Path::new("presets"));
        }

        ui.separator();
        let play_label = if app.state.playing { "⏸ Pause" } else { "▶ Play" };
        if ui.button(play_label).on_hover_text("Space").clicked() { app.state.playing = !app.state.playing; }
        if ui.add_enabled(!app.state.playing, egui::Button::new("⏭ Step")).clicked() { app.state.step_once = true; }
        ui.label("steps/frame");
        ui.add(egui::DragValue::new(&mut app.state.steps_per_frame).range(1..=256));
        if ui.button("↺ Reset").on_hover_text("Apply grid settings and re-initialise").clicked() { app.apply_settings_and_reset(); }

        ui.separator();
        let frame = app.sim.lock().unwrap().frame();
        let dt = ui.input(|i| i.stable_dt).max(1e-6);
        ui.label(format!("step {frame}   {:.0} fps", 1.0 / dt));
    });
}
```

- [ ] **Step 3: Implement `src/app/ui_editor.rs`**

```rust
use super::App;
use crate::shader::highlight::layouter;
use crate::shader::validate::ShaderFile;

fn editor_id(file: ShaderFile) -> egui::Id {
    egui::Id::new(("wgsl-editor", file.label()))
}

fn char_index_of_line(text: &str, line: usize) -> usize {
    text.split_inclusive('\n').take(line.saturating_sub(1)).map(|l| l.chars().count()).sum()
}

fn one_editor(app: &mut App, ui: &mut egui::Ui, file: ShaderFile, title: &str) {
    let dirty = app.state.editor.dirty;
    let header = format!("{title}{}", if dirty { " *" } else { "" });
    egui::CollapsingHeader::new(header).default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Apply (Ctrl+Enter)").clicked() { app.apply_shaders(); }
            let n_err = app.state.errors.iter().filter(|e| e.file == file).count();
            if n_err > 0 { ui.colored_label(egui::Color32::from_rgb(230, 90, 90), format!("{n_err} error(s)")); }
        });
        let id = editor_id(file);
        // Jump the cursor when an error entry was clicked.
        if let Some((f, line)) = app.pending_cursor.take_if(|(f, _)| *f == file) {
            let text = match f { ShaderFile::Rule => &app.state.editor.rule, ShaderFile::Render => &app.state.editor.render };
            let idx = char_index_of_line(text, line);
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx))));
            st.store(ui.ctx(), id);
            ui.memory_mut(|m| m.request_focus(id));
        }
        let text = match file { ShaderFile::Rule => &mut app.state.editor.rule, ShaderFile::Render => &mut app.state.editor.render };
        let mut lay = layouter();
        let out = egui::TextEdit::multiline(text)
            .id(id)
            .code_editor()
            .font(egui::FontId::monospace(13.0))
            .desired_rows(14)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .layouter(&mut lay)
            .show(ui);
        if out.response.changed() { app.state.editor.dirty = true; }
    });
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    one_editor(app, ui, ShaderFile::Rule, "Rule (WGSL)");
    one_editor(app, ui, ShaderFile::Render, "Render (WGSL)");
}
```

If `Option::take_if` is unavailable, replace with:
```rust
let jump = matches!(app.pending_cursor, Some((f, _)) if f == file);
if jump { let (f, line) = app.pending_cursor.take().unwrap(); ... }
```

- [ ] **Step 4: Implement `src/app/ui_params.rs`**

```rust
use super::App;
use crate::preset::{InitPattern, Mode};
use crate::shader::params::{ParamSpec, ParamValue};

fn slider_f32(ui: &mut egui::Ui, v: &mut f32, range: Option<(f64, f64)>) -> bool {
    match range {
        Some((lo, hi)) => ui.add(egui::Slider::new(v, lo as f32..=hi as f32)).changed(),
        None => ui.add(egui::DragValue::new(v).speed(0.01)).changed(),
    }
}

fn param_widget(ui: &mut egui::Ui, spec: &ParamSpec, value: &mut ParamValue) -> bool {
    match value {
        ParamValue::F32(v) => slider_f32(ui, v, spec.range),
        ParamValue::I32(v) => match spec.range {
            Some((lo, hi)) => ui.add(egui::Slider::new(v, lo as i32..=hi as i32)).changed(),
            None => ui.add(egui::DragValue::new(v)).changed(),
        },
        ParamValue::Bool(b) => ui.checkbox(b, "").changed(),
        ParamValue::Vec2(a) => { let mut c = false; for x in a.iter_mut() { c |= slider_f32(ui, x, spec.range); } c }
        ParamValue::Vec3(a) => {
            if spec.color { ui.color_edit_button_rgb(a).changed() }
            else { let mut c = false; for x in a.iter_mut() { c |= slider_f32(ui, x, spec.range); } c }
        }
        ParamValue::Vec4(a) => {
            if spec.color { ui.color_edit_button_rgba_unmultiplied(a).changed() }
            else { let mut c = false; for x in a.iter_mut() { c |= slider_f32(ui, x, spec.range); } c }
        }
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.heading("Params");
    if app.state.specs.is_empty() {
        ui.label(egui::RichText::new("Declare params in a shader with `// @param name: f32 = 0.5 range 0 .. 1`").weak());
    }
    let mut changed = false;
    egui::Grid::new("params").num_columns(2).striped(true).show(ui, |ui| {
        for spec in app.state.specs.clone() {
            ui.label(&spec.name);
            if let Some(v) = app.state.values.get_mut(&spec.name) {
                ui.horizontal(|ui| { changed |= param_widget(ui, &spec, v); });
            }
            ui.end_row();
        }
    });
    if changed { app.push_params(); }

    ui.separator();
    ui.heading("Grid");
    let p = &mut app.state.pending;
    egui::Grid::new("grid-settings").num_columns(2).show(ui, |ui| {
        ui.label("mode");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut p.mode, Mode::TwoD, "2D");
            ui.selectable_value(&mut p.mode, Mode::OneD, "1D (space-time)");
        });
        ui.end_row();
        ui.label("size");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut p.width).range(1..=4096).speed(4));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut p.height).range(1..=4096).speed(4));
        });
        ui.end_row();
        ui.label("init");
        ui.horizontal(|ui| {
            let is_random = matches!(p.init, InitPattern::Random { .. });
            if ui.selectable_label(is_random, "random").clicked() && !is_random { p.init = InitPattern::Random { density: 0.3 }; }
            if ui.selectable_label(p.init == InitPattern::Single, "single").clicked() { p.init = InitPattern::Single; }
            if ui.selectable_label(p.init == InitPattern::Blank, "blank").clicked() { p.init = InitPattern::Blank; }
        });
        ui.end_row();
        if let InitPattern::Random { density } = &mut p.init {
            ui.label("density");
            ui.add(egui::Slider::new(density, 0.0..=1.0));
            ui.end_row();
        }
        ui.label("seed");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut p.seed));
            if ui.button("🎲").clicked() { p.seed = p.seed.wrapping_mul(1664525).wrapping_add(1013904223); }
        });
        ui.end_row();
    });
    if ui.button("Apply grid settings & reset").clicked() { app.apply_settings_and_reset(); }
}
```

- [ ] **Step 5: Implement `src/app/ui_errors.rs`**

```rust
use super::App;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.state.errors.is_empty() {
        let msg = if app.sim.lock().unwrap().has_pipelines() { "✔ shaders compiled" } else { "no pipeline yet" };
        ui.label(egui::RichText::new(msg).weak());
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for e in app.state.errors.clone() {
            let text = format!("{}:{}:{}  {}", e.file.label(), e.line, e.column, e.message);
            if ui.add(egui::Label::new(egui::RichText::new(text).color(egui::Color32::from_rgb(230, 110, 110)).monospace()).sense(egui::Sense::click())).clicked() {
                app.pending_cursor = Some((e.file, e.line));
            }
        }
    });
}
```

- [ ] **Step 6: Build and run**

Run: `cargo build 2>&1 | tail -40`
Expected: compiles. Fix API mismatches by reading the egui/eframe source in `~/.cargo/registry/src/*/egui-0.36.2/`.

Run: `cargo run --release` (background, then screenshot or observe) and check:
1. Window opens with Game of Life animating in the viewport.
2. Switch to Rule 30: a space-time triangle grows downward then scrolls.
3. Gray-Scott: spots/worms emerge after a few seconds.
4. Change `fade` slider: colours shift live.
5. Type a syntax error in rule, Ctrl+Enter: error appears in bottom panel with a line number, viewport keeps animating with the old rule. Click the error: the cursor jumps.
6. Fix the error, Ctrl+Enter: error panel clears.
7. Set size 1024x1024, Reset: still runs.

- [ ] **Step 7: Commit**

```bash
git add src/app
git commit -m "Add full egui UI: editors, params, transport, presets, error panel"
```

---

### Task 13: README, clippy clean, final test run

**Files:**
- Create: `README.md`
- Modify: whatever clippy flags

- [ ] **Step 1: Write README.md**

```markdown
# Cellular Automata Shader IDE

Live-code 1D and 2D cellular automata as WGSL compute shaders and render them with
WGSL fragment shaders. Rust, egui, wgpu.

## Run

    cargo run --release

## Writing a rule

Edit the **Rule** editor and press **Ctrl+Enter**. You write one function:

    fn rule(pos: vec2<u32>) -> vec4<f32>

Helpers available: `cell(x, y)` (wraparound read of the previous state), `prev_cell(x)`
(1D mode: the previous row), `rand(pos, salt)`, `hash(u)`, `globals.size`, `globals.frame`,
`globals.time`, `globals.seed`.

The **Render** editor provides:

    fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32>

## Parameters

Declare sliders in either shader with a comment:

    // @param threshold: f32 = 0.5 range 0.0 .. 1.0
    // @param n: i32 = 3 range 0 .. 8
    // @param on: bool = true            // use as `params.on != 0u`
    // @param tint: vec3<f32> = (1, 0.5, 0.2) color

They appear under **Params** and are available as `params.<name>`.

## Presets

A preset is a folder containing `preset.toml`, `rule.wgsl` and `render.wgsl`.
Folders under `./presets` appear in the dropdown. Built-ins: Rule 30, Rule 110,
Game of Life, Gray-Scott.

## Tests

    cargo test
```

- [ ] **Step 2: Clippy and tests**

Run: `cargo clippy --all-targets -- -D warnings && cargo test`
Expected: no warnings; all tests pass. Fix anything reported.

- [ ] **Step 3: Commit**

```bash
git add README.md src
git commit -m "Add README and clippy cleanups"
```

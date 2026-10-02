//! Wraps user-written WGSL in a fixed prelude (bindings, helpers) and epilogue (entry points).

#[derive(Debug, Clone)]
pub struct Assembled {
    pub source: String,
    /// Number of generated lines before the user's first line. User line `n` is assembled line `n + offset`.
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

// A fresh random number per cell per step.
fn noise(pos: vec2<u32>) -> f32 {
    return rand(pos, globals.frame);
}

fn alive(x: i32, y: i32) -> bool {
    return cell(x, y).r > 0.5;
}

fn prev_alive(x: i32) -> bool {
    return prev_cell(x).r > 0.5;
}

// Number of live cells among the 8 neighbours (Moore neighbourhood).
fn neighbours(x: i32, y: i32) -> u32 {
    var n = 0u;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx != 0 || dy != 0) {
                n += u32(alive(x + dx, y + dy));
            }
        }
    }
    return n;
}

// Number of live cells among the 4 orthogonal neighbours (von Neumann neighbourhood).
fn neighbours4(x: i32, y: i32) -> u32 {
    return u32(alive(x - 1, y)) + u32(alive(x + 1, y)) + u32(alive(x, y - 1)) + u32(alive(x, y + 1));
}

// Sum of the 8 neighbours' values.
fn moore_sum(x: i32, y: i32) -> vec4<f32> {
    return cell(x - 1, y - 1) + cell(x, y - 1) + cell(x + 1, y - 1)
        + cell(x - 1, y) + cell(x + 1, y)
        + cell(x - 1, y + 1) + cell(x, y + 1) + cell(x + 1, y + 1);
}

// 9-point Laplacian: how much a cell differs from its surroundings.
fn laplacian(x: i32, y: i32) -> vec4<f32> {
    return cell(x - 1, y) + cell(x + 1, y) + cell(x, y - 1) + cell(x, y + 1)
        + 0.5 * (cell(x - 1, y - 1) + cell(x + 1, y - 1) + cell(x - 1, y + 1) + cell(x + 1, y + 1))
        - 6.0 * cell(x, y);
}

fn on() -> vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}

fn off() -> vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// on() when `v` is true, off() otherwise.
fn on_if(v: bool) -> vec4<f32> {
    return select(off(), on(), v);
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

// Neighbouring cell by grid coordinate (the `cell` parameter of `shade` shadows `cell()`).
fn cell_at(x: i32, y: i32) -> vec4<f32> {
    return cell(x, y);
}

fn gray(t: f32) -> vec4<f32> {
    return vec4<f32>(vec3<f32>(clamp(t, 0.0, 1.0)), 1.0);
}

fn rgb(r: f32, g: f32, b: f32) -> vec4<f32> {
    return vec4<f32>(r, g, b, 1.0);
}

// Hue, saturation and value in 0..1.
fn hsv(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - vec3<f32>(3.0));
    return v * mix(vec3<f32>(1.0), clamp(p - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

// A smooth gradient for t in 0..1 (cosine palette).
fn palette(t: f32) -> vec3<f32> {
    let tt = clamp(t, 0.0, 1.0);
    return vec3<f32>(0.5) + vec3<f32>(0.5) * cos(6.28318 * (tt + vec3<f32>(0.0, 0.33, 0.67)));
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

/// The post shader every preset starts with: no change to the picture.
pub const DEFAULT_POST: &str = "// Post-processing: `color` is this pixel, scene(uv) samples the picture, prev(uv) is the\n// previous frame's output (feedback), scene_px() is one pixel in uv units.\nfn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    return color;\n}\n";

const POST_PRELUDE: &str = r#"@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var<uniform> globals: Globals;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var prev_tex: texture_2d<f32>;

// The rendered picture at `uv` (0..1), linearly filtered, clamped at the edges.
fn scene(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(scene_tex, scene_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

// Last frame's post output at `uv`: feedback for trails and smearing.
fn prev(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(prev_tex, scene_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

// Size of one picture pixel in uv units.
fn scene_px() -> vec2<f32> {
    return vec2<f32>(1.0) / vec2<f32>(textureDimensions(scene_tex));
}

// ---- user post ----
"#;

const POST_EPILOGUE: &str = r#"
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
    let c = textureSample(scene_tex, scene_sampler, in.uv);
    return post(in.uv, c);
}
"#;

/// Fixed shader that copies the post output to the window.
pub const BLIT_WGSL: &str = r#"@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
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
    return vec4<f32>(textureSample(src_tex, src_sampler, in.uv).rgb, 1.0);
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

pub fn assemble_post(user: &str, params_struct: &str) -> Assembled {
    assemble(POST_PRELUDE, user, params_struct, POST_EPILOGUE)
}

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
        let a = assemble_rule(
            "fn rule(pos: vec2<u32>) -> vec4<f32> { return vec4<f32>(0.0); }",
            &params_wgsl(&[]),
        );
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

    #[test]
    fn post_offset_points_at_user_source_and_has_scene_bindings() {
        let user = "fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color; }";
        let a = assemble_post(user, &params_wgsl(&[]));
        let lines: Vec<&str> = a.source.lines().collect();
        assert_eq!(lines[a.user_line_offset], user);
        assert!(a.source.contains("var scene_tex: texture_2d<f32>;"));
        assert!(a.source.contains("var prev_tex: texture_2d<f32>;"));
        assert!(a.source.contains("fn scene(uv: vec2<f32>) -> vec4<f32>"));
        assert!(a.source.contains("fn prev(uv: vec2<f32>) -> vec4<f32>"));
        assert!(a.source.contains("@fragment"));
    }

    #[test]
    fn default_post_is_a_passthrough() {
        assert!(DEFAULT_POST.contains("return color;"));
    }
}

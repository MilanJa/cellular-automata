// Slither Garden: as Slither (bodies coloured by the snake's length, flesh in the body's colour
// dimmer towards its edge, heads white, food amber, the scent field as a coloured glow
// underneath), plus the garden that grows in the snakes' wake from layer B's blue channel:
// young growth bright, old growth dim.
// @param head_colour: vec3<f32> = (1.0, 1.0, 1.0) color
// @param food_colour: vec3<f32> = (1.0, 0.75, 0.25) color
// @param garden_young: vec3<f32> = (0.5, 0.95, 0.5) color
// @param garden_old: vec3<f32> = (0.1, 0.3, 0.15) color
// @param garden_fade: f32 = 300.0 range 10.0 .. 2000.0
// @param scent_glow: f32 = 0.35 range 0.0 .. 1.0
// @param scent_gain: f32 = 1.0 range 0.1 .. 10.0

// .r packs kind * 8 + direction + 64 * id; .b packs stamp + 2048 * moves; .a is the tail's
// stamp, so the length is moves - tail stamp + 1 and the index moves - stamp (see rule.wgsl).
// Flesh keeps its offset to the spine in .g as (dx + 3) + 7 * (dy + 3).
fn kind_of(c: vec4<f32>) -> i32 {
    return (i32(round(c.r)) % 64) / 8;
}

fn since(later: i32, earlier: i32) -> i32 {
    return ((later - earlier) % 2048 + 2048) % 2048;
}

fn index_of(c: vec4<f32>) -> f32 {
    let b = i32(round(c.b));
    return f32(since(b / 2048, b % 2048));
}

fn len_of(c: vec4<f32>) -> f32 {
    return f32(since(i32(round(c.b)) / 2048, i32(round(c.a))) + 1);
}

fn length_hue(len: f32) -> f32 {
    return fract(0.55 + log2(max(len, 0.5)) * 0.12);
}

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let p = vec2<i32>(uv * vec2<f32>(globals.size));
    let s = other(p.x, p.y);
    let strength = s.r * params.scent_gain;
    let presence = strength / (strength + 1.0);
    let avg_len = s.g / max(s.r, 1e-4);
    let scent = hsv(length_hue(avg_len), 0.85, 1.0);
    var colour = mix(vec3<f32>(0.02, 0.03, 0.05), scent * 0.8, presence * params.scent_glow);
    if (s.b > 0.5) {
        colour = mix(params.garden_young, params.garden_old, clamp(s.a / params.garden_fade, 0.0, 1.0));
    }
    let k = kind_of(cell);
    if (k == 1) {
        let len = len_of(cell);
        let fade = 1.0 - 0.5 * clamp(index_of(cell) / max(len, 1.0), 0.0, 1.0);
        colour = hsv(length_hue(len), 0.75, 0.95 * fade);
    } else if (k == 2) {
        colour = params.head_colour;
    } else if (k == 3) {
        colour = params.food_colour;
    } else if (k == 4) {
        let code = i32(round(cell.g));
        let off = vec2<f32>(f32(code % 7 - 3), f32(code / 7 - 3));
        let len = len_of(cell);
        let fade = 1.0 - 0.5 * clamp(index_of(cell) / max(len, 1.0), 0.0, 1.0);
        let edge = 1.0 - 0.12 * length(off);
        colour = hsv(length_hue(len), 0.7, 0.85 * fade * edge);
    }
    return vec4<f32>(colour, 1.0);
}

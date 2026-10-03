// Slither: bodies coloured by the snake's length (short snakes cyan, long ones towards red),
// fading towards the tail; heads white; food amber. Underneath, the scent field from layer B:
// brightness is how strong the smell is, and the hue is the average length of what left it,
// on the same scale as the bodies, so a snake's trail carries its own colour and you can see
// what a head is smelling when it turns.
// @param head_colour: vec3<f32> = (1.0, 1.0, 1.0) color
// @param food_colour: vec3<f32> = (1.0, 0.75, 0.25) color
// @param scent_glow: f32 = 0.7 range 0.0 .. 1.0
// @param scent_gain: f32 = 2.0 range 0.1 .. 10.0

// .r packs kind * 8 + direction + 32 * id; .b packs stamp + 2048 * moves; .a is the tail's
// stamp, so the length is moves - tail stamp + 1 and the index moves - stamp (see rule.wgsl).
fn kind_of(c: vec4<f32>) -> i32 {
    return (i32(round(c.r)) % 32) / 8;
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
    let k = kind_of(cell);
    if (k == 1) {
        let len = len_of(cell);
        let fade = 1.0 - 0.5 * clamp(index_of(cell) / max(len, 1.0), 0.0, 1.0);
        colour = hsv(length_hue(len), 0.75, 0.95 * fade);
    } else if (k == 2) {
        colour = params.head_colour;
    } else if (k == 3) {
        colour = params.food_colour;
    }
    return vec4<f32>(colour, 1.0);
}

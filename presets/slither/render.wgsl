// Slither: bodies coloured by the snake's length (short snakes cyan, long ones towards red),
// fading towards the tail; heads white; food amber. Underneath, the scent field from layer B:
// brightness is how strong the smell is, and the hue is the average length of what left it,
// on the same scale as the bodies, so a snake's trail carries its own colour and you can see
// what a head is smelling when it turns.
// @param head_colour: vec3<f32> = (1.0, 1.0, 1.0) color
// @param food_colour: vec3<f32> = (1.0, 0.75, 0.25) color
// @param scent_glow: f32 = 0.7 range 0.0 .. 1.0
// @param scent_gain: f32 = 2.0 range 0.1 .. 10.0

fn length_hue(len: f32) -> f32 {
    return fract(0.55 + log2(max(len, 0.5)) * 0.12);
}

// .r packs kind * 8 + direction + 32 * id; .b packs stamp + 2048 * moves (see rule.wgsl).
fn kind_of(c: vec4<f32>) -> i32 {
    return (i32(round(c.r)) % 32) / 8;
}

fn index_of(c: vec4<f32>) -> i32 {
    let b = i32(round(c.b));
    return (b / 2048 - b % 2048 + 2048) % 2048;
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
        let hue = length_hue(cell.a);
        let fade = 1.0 - 0.5 * clamp(f32(index_of(cell)) / max(cell.a, 1.0), 0.0, 1.0);
        colour = hsv(hue, 0.75, 0.95 * fade);
    } else if (k == 2) {
        colour = params.head_colour;
    } else if (k == 3) {
        colour = params.food_colour;
    }
    return vec4<f32>(colour, 1.0);
}

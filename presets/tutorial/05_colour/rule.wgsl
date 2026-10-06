// TUTORIAL 5: the rule stores more than on/off so the render shader has something to draw.
//   .r alive (0 or 1)
//   .g age in steps
//   .b "ember": set to 1.0 the step a cell dies, then multiplied by `decay` every step
// The rule itself is still plain Life.
// @param decay: f32 = 0.9 range 0.5 .. 0.99

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    let was = c.r > 0.5;
    let n = neighbours(x, y);
    let next = (was && (n == 2u || n == 3u)) || (!was && n == 3u);
    let age = select(0.0, c.g + 1.0, next && was);
    let ember = select(c.b * params.decay, 1.0, was && !next);
    return vec4<f32>(f32(next), age, ember, 1.0);
}

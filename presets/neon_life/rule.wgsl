// Game of Life, unchanged. Two extra channels exist only to drive the look:
//   .r alive (0/1)   .g age in steps   .b "ember": set to 1 when a cell dies, then fades.
// @param trail_decay: f32 = 0.92 range 0.5 .. 0.995

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let me = cell(x, y);
    let was = me.r > 0.5;
    let n = neighbours(x, y);
    let next = (was && (n == 2u || n == 3u)) || (!was && n == 3u);
    let age = select(0.0, me.g + 1.0, next && was);
    let ember = select(me.b * params.trail_decay, 1.0, was && !next);
    return vec4<f32>(f32(next), age, ember, 1.0);
}

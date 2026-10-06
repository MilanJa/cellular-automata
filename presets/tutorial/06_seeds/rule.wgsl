// TUTORIAL 6: Life that never settles. A tiny chance of a spontaneous birth keeps the grid
// alive, and the Grid's seed value decides which random numbers you get, so the same seed
// always replays the same history.
//   noise(pos)      -> f32   a fresh random number 0 .. 1 for this cell, this step
//   rand(pos, salt) -> f32   the same number every step for a given salt (for fixed patterns)
// @param spark: f32 = 0.0002 range 0.0 .. 0.005

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours(x, y);
    let me = alive(x, y);
    let survives = me && (n == 2u || n == 3u);
    let born = !me && n == 3u;
    let spark = !me && noise(pos) < params.spark;
    let next = survives || born || spark;
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

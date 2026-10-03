// TUTORIAL 7: a continuous seed. The grid starts as a bath of U (.r = 1) with no V, except a
// square in the middle that holds V. A sprinkle of noise breaks the symmetry so the pattern
// does not stay a perfect square.
// @param square: i32 = 10 range 1 .. 60

fn seed(pos: vec2<u32>) -> vec4<f32> {
    let p = centred(pos);
    var v = 0.0;
    if (in_box(p, vec2<i32>(-params.square), vec2<i32>(2 * params.square))) {
        v = 1.0;
    }
    v += rand(pos, 1u) * 0.02;
    return vec4<f32>(1.0 - 0.5 * v, v, 0.0, 1.0);
}

// Gray-Scott grown from a ring of discs: a central drop of the catalyst V surrounded by
// smaller ones, in a bath of U. The fronts meet symmetrically and interfere.
//
// A seed can declare sliders too; these take effect on the next Reset.
// @param discs: i32 = 6 range 1 .. 12
// @param ring: f32 = 90.0 range 10.0 .. 240.0

fn seed(pos: vec2<u32>) -> vec4<f32> {
    let p = centred(pos);
    var v = 0.0;
    if (in_disc(p, vec2<i32>(0, 0), 10.0)) {
        v = 1.0;
    }
    for (var i = 0; i < params.discs; i++) {
        let angle = 6.28318 * f32(i) / f32(params.discs);
        let centre = vec2<i32>(vec2<f32>(cos(angle), sin(angle)) * params.ring);
        if (in_disc(p, centre, 7.0)) {
            v = 1.0;
        }
    }
    // The rule reads r as U and g as V.
    return vec4<f32>(1.0 - 0.5 * v, v, 0.0, 1.0);
}

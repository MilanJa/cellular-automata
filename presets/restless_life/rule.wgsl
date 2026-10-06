// Restless Life: Game of Life with a built-in dislike of standing still.
//
// Plain Life runs down: the soup burns out into still lifes and blinkers and then nothing
// happens any more. Here every cell counts how long it has been unchanged (.g). Once that
// passes `patience`, the cell wants to change, and flips with a probability that ramps up to
// `restlessness` over the next `patience` steps. Oscillators are left alone (they do change);
// still lifes and the frozen cells around them get nudged until something new happens.
// Empty space far from anything only gets a rare `spark`, so the background does not boil.
//   .r alive   .g steps since the cell last changed   .b flash: 1.0 on a restless flip, fading
// @param patience: f32 = 30.0 range 1.0 .. 300.0
// @param restlessness: f32 = 0.03 range 0.0 .. 0.3
// @param spark: f32 = 0.0002 range 0.0 .. 0.005

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    let me = c.r > 0.5;
    let n = neighbours(x, y);

    // Life as usual.
    var next = (me && (n == 2u || n == 3u)) || (!me && n == 3u);
    var flash = c.b * 0.9;

    // The restless part: unchanged for too long, so change anyway.
    if (next == me && c.g >= params.patience) {
        let bored = clamp((c.g - params.patience) / params.patience, 0.0, 1.0);
        let something_here = me || n > 0u;
        let p = select(params.spark, params.restlessness * bored, something_here);
        if (noise(pos) < p) {
            next = !me;
            flash = 1.0;
        }
    }

    let still = select(0.0, c.g + 1.0, next == me);
    return vec4<f32>(f32(next), still, flash, 1.0);
}

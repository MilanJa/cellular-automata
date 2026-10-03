// Slither, layer B: the scent the snakes hunt by. Every snake cell in layer A (read through
// other()) emits scent, which diffuses and decays. Two channels:
//   .r  presence   how much snake is nearby
//   .g  weighted   the same, weighted by each emitter's length
// so .g / .r is the average length of whatever a head smells, which is how it decides whether
// a smell means prey or danger. Diffusion must stay below 0.25 for this stencil.
// @param diffusion: f32 = 0.2 range 0.0 .. 0.25
// @param decay: f32 = 0.002 range 0.0 .. 0.05
// @param emit: f32 = 0.05 range 0.0 .. 0.5

// A layer A cell's kind and length (see the cell layout in the Slither rule): kind is packed
// into .r, the length is the head's move count (.b / 2048) minus the tail's stamp (.a) plus one;
// food counts as one half.
fn snake_length(c: vec4<f32>) -> f32 {
    let kind = (i32(round(c.r)) % 32) / 8;
    if (kind == 3) {
        return 0.5;
    }
    let moves = i32(round(c.b)) / 2048;
    let tail_stamp = i32(round(c.a));
    return f32(((moves - tail_stamp) % 2048 + 2048) % 2048 + 1);
}

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    let lap = laplacian(x, y);
    var s = (c.rg + params.diffusion * lap.rg) * (1.0 - params.decay);
    let snake = other(x, y);
    if (snake.r > 0.5) {
        s += vec2<f32>(params.emit, params.emit * snake_length(snake));
    }
    return vec4<f32>(max(s, vec2<f32>(0.0)), 0.0, 1.0);
}

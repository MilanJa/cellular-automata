// TEMPLATE: one automaton driven by another.
//
// Load a second preset as **Layer B** (Params panel, "Layer B"); try Gray-Scott. This rule is
// Life, but cells are only born where layer B's .g channel is above a threshold, so the
// reaction-diffusion pattern shapes where Life can grow.
//   other(x, y)       -> vec4   layer B's cell at (x, y) (zeros when there is no layer B)
//   other_alive(x, y) -> bool   layer B's .r > 0.5
// @param gate: f32 = 0.15 range 0.0 .. 1.0
// @param channel: i32 = 1 range 0 .. 3

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours(x, y);
    let me = alive(x, y);
    let b = other(x, y);
    let drive = select(select(select(b.r, b.g, params.channel == 1), b.b, params.channel == 2), b.a, params.channel == 3);
    let fertile = drive > params.gate;
    let next = (me && (n == 2u || n == 3u)) || (!me && n == 3u && fertile);
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, drive, 1.0);
}

// TEMPLATE: continuous-state automaton (values between 0 and 1 instead of on/off).
//
// Every cell holds four floats (.r .g .b .a). This template diffuses .r with a little
// reaction term, giving blurry blobs; replace the two "physics" lines with your own idea.
//   laplacian(x, y) -> vec4   how different a cell is from its surroundings (9-point)
//   moore_sum(x, y) -> vec4   sum of the 8 neighbours
//   noise(pos)      -> f32    random 0..1 per cell per step
// @param diffusion: f32 = 0.15 range 0.0 .. 0.25
// @param growth: f32 = 0.05 range -0.2 .. 0.2
// @param jitter: f32 = 0.0 range 0.0 .. 0.05

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    let lap = laplacian(x, y);

    // --- physics: edit from here ---
    var v = c.r + params.diffusion * lap.r;                 // spread out
    v += params.growth * v * (1.0 - v) * (v - 0.3);         // grow when above 0.3, fade below
    v += (noise(pos) - 0.5) * params.jitter;                // optional randomness
    // --- to here ---

    return vec4<f32>(clamp(v, 0.0, 1.0), c.g, c.b, 1.0);
}

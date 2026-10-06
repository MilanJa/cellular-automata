// TUTORIAL 7: continuous cells. Nothing is "on" or "off" here: .r holds the amount of a
// chemical U and .g the amount of a chemical V, both between 0 and 1, and every step nudges
// them a little. This is the Gray-Scott reaction-diffusion model.
//   laplacian(x, y) -> vec4   how much each channel differs from its surroundings
//                             (positive where the neighbours hold more than the cell)
//
// Three things happen each step:
//   diffusion   both chemicals spread out: move towards the neighbourhood's average
//   reaction    U + 2V -> 3V: V eats U and makes more V (the u * v * v term)
//   feed/kill   U is topped up towards 1 at rate `feed`; V drains away at rate feed + kill
// Try feed 0.025 / kill 0.06 (worms), 0.03 / 0.062 (spots), 0.055 / 0.062 (coral).
// @param feed: f32 = 0.037 range 0.0 .. 0.1
// @param kill: f32 = 0.06 range 0.0 .. 0.1
// @param du: f32 = 0.2 range 0.0 .. 0.5
// @param dv: f32 = 0.1 range 0.0 .. 0.5

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    let lap = laplacian(x, y);
    let u = c.r;
    let v = c.g;
    let reaction = u * v * v;
    let next_u = u + params.du * lap.r - reaction + params.feed * (1.0 - u);
    let next_v = v + params.dv * lap.g + reaction - (params.feed + params.kill) * v;
    // Keep the values in range: floating point drift would otherwise run away.
    return vec4<f32>(clamp(next_u, 0.0, 1.0), clamp(next_v, 0.0, 1.0), 0.0, 1.0);
}

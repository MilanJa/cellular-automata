// Gray-Scott reaction-diffusion. r = U, g = V. Init: random cells seed V.
// laplacian(x, y) is the 9-point Laplacian of the neighbourhood.
// @param feed: f32 = 0.037 range 0.0 .. 0.1
// @param kill: f32 = 0.06 range 0.0 .. 0.1
// @param du: f32 = 0.2 range 0.0 .. 0.5
// @param dv: f32 = 0.1 range 0.0 .. 0.5

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let c = cell(x, y);
    // On the first step the init pattern has r = 1 where "alive"; convert to U/V fields.
    if (globals.frame == 0u) {
        let v = c.r;
        return vec4<f32>(1.0 - v * 0.5, v, 0.0, 1.0);
    }
    let lap = laplacian(x, y);
    let u = c.r;
    let v = c.g;
    let uvv = u * v * v;
    let nu = u + params.du * lap.r - uvv + params.feed * (1.0 - u);
    let nv = v + params.dv * lap.g + uvv - (params.feed + params.kill) * v;
    return vec4<f32>(clamp(nu, 0.0, 1.0), clamp(nv, 0.0, 1.0), 0.0, 1.0);
}

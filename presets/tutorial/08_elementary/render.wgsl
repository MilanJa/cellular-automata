// TUTORIAL 8: ink on paper. In 1D mode uv.y runs from the oldest row (top) to the newest
// (bottom); the `fade` slider lets the oldest rows yellow a little.
// @param ink: vec3<f32> = (0.1, 0.1, 0.12) color
// @param paper: vec3<f32> = (0.96, 0.94, 0.88) color
// @param fade: f32 = 0.15 range 0.0 .. 0.5

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let aged = params.paper * (1.0 - params.fade * (1.0 - uv.y) * vec3<f32>(0.0, 0.2, 0.6));
    return vec4<f32>(mix(aged, params.ink, cell.r), 1.0);
}

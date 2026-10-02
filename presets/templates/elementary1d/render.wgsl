// TEMPLATE: render for a 1D diagram. `uv.y` runs 0 (top, oldest) to 1 (bottom, newest).
// @param ink: vec3<f32> = (0.1, 0.1, 0.12) color
// @param paper: vec3<f32> = (0.96, 0.94, 0.88) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(mix(params.paper, params.ink, cell.r), 1.0);
}

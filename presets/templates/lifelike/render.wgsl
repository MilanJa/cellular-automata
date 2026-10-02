// TEMPLATE: simple two-colour rendering.
// @param on_colour: vec3<f32> = (0.9, 0.95, 1.0) color
// @param off_colour: vec3<f32> = (0.03, 0.03, 0.06) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(mix(params.off_colour, params.on_colour, cell.r), 1.0);
}

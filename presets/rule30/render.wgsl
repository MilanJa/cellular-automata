// @param fg: vec3<f32> = (0.95, 0.95, 0.9) color
// @param bg: vec3<f32> = (0.08, 0.08, 0.1) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(mix(params.bg, params.fg, cell.r), 1.0);
}

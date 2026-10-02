// Live cells in warm colours over a dim view of layer B's driving field (stored in .b by the rule).
// @param live: vec3<f32> = (1.0, 0.85, 0.4) color
// @param field: vec3<f32> = (0.1, 0.25, 0.5) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let bg = params.field * clamp(cell.b * 2.0, 0.0, 1.0);
    return vec4<f32>(mix(bg, params.live, cell.r), 1.0);
}

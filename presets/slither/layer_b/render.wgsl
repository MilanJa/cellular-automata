// The scent field on its own (only seen when this layer is loaded as a preset in its own right).
fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let s = cell.r / (cell.r + 1.0);
    return vec4<f32>(palette(s * 0.8), 1.0);
}

// The scent and the garden on their own (only seen when this layer is loaded as a preset in
// its own right): scent as a palette, garden cells in green.
fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let s = cell.r / (cell.r + 1.0);
    var colour = palette(s * 0.8) * 0.6;
    if (cell.b > 0.5) {
        colour = vec3<f32>(0.4, 1.0, 0.5);
    }
    return vec4<f32>(colour, 1.0);
}

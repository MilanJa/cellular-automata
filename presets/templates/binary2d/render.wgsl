// TEMPLATE: colour each cell. `shade` receives the cell's value and returns an RGBA colour.
// Helpers: gray(t), rgb(r, g, b), hsv(h, s, v), palette(t), and cell_at(x, y) for neighbours.
// @param on_colour: vec3<f32> = (0.95, 0.9, 0.6) color
// @param off_colour: vec3<f32> = (0.05, 0.05, 0.08) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    // mix(a, b, t) blends between two colours; cell.r is 0 or 1 here.
    return vec4<f32>(mix(params.off_colour, params.on_colour, cell.r), 1.0);
}

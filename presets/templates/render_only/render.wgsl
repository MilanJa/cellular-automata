// TEMPLATE: write your own colouring. Press Ctrl+Enter after each change.
//
// You get:  uv   (0..1 position across the grid, .x right, .y down)
//           cell (the cell's four floats; for Life .r = alive, .g = age)
// Helpers:  gray(t), rgb(r, g, b), hsv(h, s, v), palette(t), cell_at(x, y), globals.time
//
// Ideas: colour by age, add a vignette from uv, pulse with sin(globals.time), or read
// cell_at(x, y) around the pixel for a glow.
// @param fade: f32 = 40.0 range 1.0 .. 200.0
// @param glow: f32 = 0.3 range 0.0 .. 1.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let age = clamp(cell.g / params.fade, 0.0, 1.0);
    var col = palette(age) * cell.r;

    // Soft glow: average the 4 orthogonal neighbours' alive values.
    let p = vec2<i32>(uv * vec2<f32>(globals.size));
    let around = (cell_at(p.x - 1, p.y).r + cell_at(p.x + 1, p.y).r
        + cell_at(p.x, p.y - 1).r + cell_at(p.x, p.y + 1).r) * 0.25;
    col += vec3<f32>(params.glow * around * 0.5);

    return vec4<f32>(col, 1.0);
}

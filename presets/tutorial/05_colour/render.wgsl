// TUTORIAL 5: a render shader with sliders, colours, a glow and time.
//   uv             where this pixel is on the grid, 0 .. 1 (x to the right, y down)
//   cell_at(x, y)  another cell, by grid coordinate (wraps around)
//   hsv(h, s, v), palette(t), gray(t), rgb(r, g, b)   colour helpers
//   globals.time   seconds since the last Reset
//
// Slider types: f32, i32, bool (read as params.name != 0u), vec2/vec3/vec4 (add `color` for
// a colour picker). Values survive re-applies as long as the name and type stay the same.
// @param hue: f32 = 0.55 range 0.0 .. 1.0
// @param ember_colour: vec3<f32> = (1.0, 0.3, 0.1) color
// @param glow: f32 = 0.4 range 0.0 .. 1.0
// @param pulse: f32 = 0.0 range 0.0 .. 1.0
// @param vignette: bool = true

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    // Live cells: a hue that drifts with age. `var` because we keep adding to it.
    let age = clamp(cell.g / 60.0, 0.0, 1.0);
    var colour = hsv(fract(params.hue + age * 0.25), 0.7, 1.0) * cell.r;

    // Dead cells show the fading ember the rule left in .b.
    colour += params.ember_colour * cell.b * (1.0 - cell.r);

    // Glow: the average of the four neighbours' alive values, found by grid coordinate.
    let p = vec2<i32>(uv * vec2<f32>(globals.size));
    let around = (cell_at(p.x - 1, p.y).r + cell_at(p.x + 1, p.y).r
        + cell_at(p.x, p.y - 1).r + cell_at(p.x, p.y + 1).r) * 0.25;
    colour += vec3<f32>(params.glow * around * 0.5);

    // Breathe with time.
    colour *= 1.0 + params.pulse * 0.3 * sin(globals.time * 3.0);

    // Vignette: darker towards the edges. A bool slider arrives as 0u or 1u.
    if (params.vignette != 0u) {
        let d = distance(uv, vec2<f32>(0.5));
        colour *= 1.0 - smoothstep(0.4, 0.75, d);
    }
    return vec4<f32>(colour, 1.0);
}

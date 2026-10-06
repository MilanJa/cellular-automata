// TUTORIAL 3: the simplest render shader. `cell` is the cell under this pixel; for an on/off
// rule its .r is 1.0 or 0.0, so mix() picks between two colours.
// @param on_colour: vec3<f32> = (0.95, 0.9, 0.6) color
// @param off_colour: vec3<f32> = (0.08, 0.1, 0.2) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(mix(params.off_colour, params.on_colour, cell.r), 1.0);
}

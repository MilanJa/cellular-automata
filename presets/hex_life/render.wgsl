// Hexagonal beads: hex_cell(uv) finds the (offset-row) cell under a pixel, hex_local(uv) the
// position inside it, hex_dist() how far that is from the centre on a hexagon metric.
// @param young: vec3<f32> = (0.95, 0.75, 0.2) color
// @param old: vec3<f32> = (0.9, 0.25, 0.5) color
// @param gap: f32 = 0.12 range 0.0 .. 0.5

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let c = hex_cell(uv);
    let v = cell_at(c.x, c.y);
    let d = hex_dist(hex_local(uv));
    let inside = 1.0 - smoothstep(1.0 - params.gap - 0.1, 1.0 - params.gap, d);
    let t = clamp(v.g / 40.0, 0.0, 1.0);
    let col = mix(params.young, params.old, t) * v.r * inside;
    return vec4<f32>(col + vec3<f32>(0.03, 0.03, 0.05) * (1.0 - v.r * inside), 1.0);
}

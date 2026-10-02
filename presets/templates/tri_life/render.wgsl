// TEMPLATE: draw triangles. tri_cell(uv) finds the triangle under a pixel; up and down
// triangles get slightly different shades so the lattice reads.
// @param up_col: vec3<f32> = (0.3, 0.9, 1.0) color
// @param down_col: vec3<f32> = (0.2, 0.6, 0.9) color
// @param old: vec3<f32> = (1.0, 0.5, 0.2) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let c = tri_cell(uv);
    let v = cell_at(c.x, c.y);
    let base = select(params.down_col, params.up_col, tri_is_up(c.x, c.y));
    let t = clamp(v.g / 50.0, 0.0, 1.0);
    let col = mix(base, params.old, t) * v.r;
    return vec4<f32>(col + vec3<f32>(0.02, 0.02, 0.04) * (1.0 - v.r), 1.0);
}

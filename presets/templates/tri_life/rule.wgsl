// TEMPLATE: an automaton on a triangular grid.
// Cell (x, y) is an upward triangle when x + y is even and a downward one otherwise
// (tri_is_up). Two neighbourhoods are available:
//   neighbours_tri(x, y)   the 3 triangles sharing an edge
//   neighbours_tri12(x, y) the 12 triangles sharing at least a corner
// @param birth_lo: i32 = 4 range 0 .. 12
// @param birth_hi: i32 = 5 range 0 .. 12
// @param keep_lo: i32 = 4 range 0 .. 12
// @param keep_hi: i32 = 6 range 0 .. 12

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = i32(neighbours_tri12(x, y));
    let me = alive(x, y);
    let born = !me && n >= params.birth_lo && n <= params.birth_hi;
    let keep = me && n >= params.keep_lo && n <= params.keep_hi;
    let next = born || keep;
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

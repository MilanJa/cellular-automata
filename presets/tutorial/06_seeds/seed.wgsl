// TUTORIAL 6: the starting state, written as code. `seed` runs once per cell on Reset when the
// Grid's init is `code`, and returns the cell's first value just like `rule` returns the next.
//   centred(pos)                 the position relative to the middle of the grid
//   in_disc(p, centre, radius)   true inside a disc; in_box(p, origin, size) likewise
//   row_bit(row, x, width)       column x of a pattern row written as bits (see below)
//   chance(pos, p)               true for a fraction p of the cells (follows the Grid's seed)
//
// Here: an R-pentomino in the middle (five cells that keep Life busy for over a thousand
// generations) inside a ring of random cells.
const R_PENTOMINO = array<u32, 3>(
    0x3u, // .XX
    0x6u, // XX.
    0x2u  // .X.
);
// @param ring_density: f32 = 0.35 range 0.0 .. 1.0

fn seed(pos: vec2<u32>) -> vec4<f32> {
    let p = centred(pos);

    // The pattern, with its top-left corner one cell up and left of the centre.
    let q = p + vec2<i32>(1, 1);
    if (q.y >= 0 && q.y < 3 && row_bit(R_PENTOMINO[q.y], q.x, 3)) {
        return on();
    }

    // The ring: between 60 and 80 cells from the centre, about a third of the cells on.
    let r = length(vec2<f32>(p));
    if (r > 60.0 && r < 80.0 && chance(pos, params.ring_density)) {
        return on();
    }
    return off();
}

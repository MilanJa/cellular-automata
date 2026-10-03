// Acorn: seven cells that take 5206 generations to settle, growing an "oak" of hundreds of
// cells and sending out thirteen gliders on the way. On this wrapping grid the debris keeps
// meeting itself, so it never quite ends.
//
// Small patterns are easiest to draw as rows of bits, most significant bit first. WGSL has no
// binary literals, so each row is written in hex with the picture alongside:
const ACORN = array<u32, 3>(
    0x20u, // .X.....
    0x08u, // ...X...
    0x67u  // XX..XXX
);

fn seed(pos: vec2<u32>) -> vec4<f32> {
    // Shift so the pattern's top-left corner is at p = (0, 0) and its middle is the grid's middle.
    let p = centred(pos) + vec2<i32>(3, 1);
    if (p.y < 0 || p.y >= 3) {
        return off();
    }
    return on_if(row_bit(ACORN[p.y], p.x, 7));
}

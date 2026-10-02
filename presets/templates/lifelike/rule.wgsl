// TEMPLATE: Life-like automaton driven entirely by checkboxes.
//
// "B3/S23" notation: a dead cell is Born with exactly 3 live neighbours, a live cell
// Survives with 2 or 3. Tick b* and s* in the Params panel to try other rules:
//   HighLife B36/S23, Day & Night B3678/S34678, Seeds B2/S, Diamoeba B35678/S5678.
// You do not need to edit this code at all; it is here so you can see how it works.
// @param b0: bool = false
// @param b1: bool = false
// @param b2: bool = false
// @param b3: bool = true
// @param b4: bool = false
// @param b5: bool = false
// @param b6: bool = false
// @param b7: bool = false
// @param b8: bool = false
// @param s0: bool = false
// @param s1: bool = false
// @param s2: bool = true
// @param s3: bool = true
// @param s4: bool = false
// @param s5: bool = false
// @param s6: bool = false
// @param s7: bool = false
// @param s8: bool = false

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours(x, y);
    // bool params are 0 or 1 in WGSL (type u32). Pack them into one bit per neighbour count.
    let birth = params.b0 | (params.b1 << 1u) | (params.b2 << 2u) | (params.b3 << 3u) | (params.b4 << 4u)
        | (params.b5 << 5u) | (params.b6 << 6u) | (params.b7 << 7u) | (params.b8 << 8u);
    let survive = params.s0 | (params.s1 << 1u) | (params.s2 << 2u) | (params.s3 << 3u) | (params.s4 << 4u)
        | (params.s5 << 5u) | (params.s6 << 6u) | (params.s7 << 7u) | (params.s8 << 8u);
    let me = alive(x, y);
    let mask = select(birth, survive, me);  // select(if_false, if_true, condition)
    return on_if(((mask >> n) & 1u) == 1u);
}

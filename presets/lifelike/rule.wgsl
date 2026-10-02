// Life-like automaton, no code changes needed: tick the neighbour counts that make a
// dead cell be born (b*) and a live cell survive (s*). Defaults are Life's B3/S23.
// Try B36/S23 (HighLife), B3678/S34678 (Day & Night), B2/S (Seeds).
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
    // Bool params arrive as 0/1; pack them into bitmasks indexed by neighbour count.
    let birth = params.b0 | (params.b1 << 1u) | (params.b2 << 2u) | (params.b3 << 3u) | (params.b4 << 4u)
        | (params.b5 << 5u) | (params.b6 << 6u) | (params.b7 << 7u) | (params.b8 << 8u);
    let survive = params.s0 | (params.s1 << 1u) | (params.s2 << 2u) | (params.s3 << 3u) | (params.s4 << 4u)
        | (params.s5 << 5u) | (params.s6 << 6u) | (params.s7 << 7u) | (params.s8 << 8u);
    let me = alive(x, y);
    let mask = select(birth, survive, me);
    let next = ((mask >> n) & 1u) == 1u;
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

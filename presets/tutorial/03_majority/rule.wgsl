// TUTORIAL 3: a first real rule. Every cell looks at itself and its four orthogonal
// neighbours and goes with the majority. Regions of equal cells grow and smooth out, then
// the picture freezes: press Reset to watch it again from new random noise.
//
//   alive(x, y) -> bool   was the cell at (x, y) on in the previous generation?
//   on_if(flag) -> vec4   the "on" cell value when flag is true, the "off" value otherwise
// The grid wraps around: x - 1 at the left edge reads the right edge.
// @param threshold: i32 = 3 range 1 .. 5

fn rule(pos: vec2<u32>) -> vec4<f32> {
    // pos is unsigned (u32). Convert to i32 so that x - 1 can be negative at the edge.
    let x = i32(pos.x);
    let y = i32(pos.y);

    // u32(true) is 1u and u32(false) is 0u, so these add up to a count.
    let me = u32(alive(x, y));
    let around = u32(alive(x - 1, y)) + u32(alive(x + 1, y)) + u32(alive(x, y - 1)) + u32(alive(x, y + 1));
    let votes = me + around; // 0 .. 5

    // params.threshold is an i32 slider; WGSL will not compare i32 with u32, so convert.
    return on_if(votes >= u32(params.threshold));
}

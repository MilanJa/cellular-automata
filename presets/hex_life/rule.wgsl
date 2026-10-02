// Life on a hexagonal grid. Rows are offset by half a cell (odd-r layout) and every cell has six
// neighbours: neighbours_hex(x, y). B2/S34 is a classic hex rule with gliders and oscillators.
// @param b2: bool = true
// @param b3: bool = false
// @param s3: bool = true
// @param s4: bool = true
// @param s2: bool = false

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours_hex(x, y);
    let me = alive(x, y);
    let born = !me && ((n == 2u && params.b2 != 0u) || (n == 3u && params.b3 != 0u));
    let keep = me && ((n == 2u && params.s2 != 0u) || (n == 3u && params.s3 != 0u) || (n == 4u && params.s4 != 0u));
    let next = born || keep;
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

// Conway's Game of Life. r = alive (0/1), g = age in steps (for colouring).
// neighbours(x, y) counts the live cells among the 8 around (x, y).

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours(x, y);
    let me = alive(x, y);
    let next = (me && (n == 2u || n == 3u)) || (!me && n == 3u);
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

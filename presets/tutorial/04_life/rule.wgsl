// TUTORIAL 4: Conway's Game of Life, plus an age counter.
//   neighbours(x, y) -> u32   how many of the 8 surrounding cells were on (0 .. 8)
//   cell(x, y)       -> vec4  the full previous value of a cell
//
// A cell is four numbers (.r .g .b .a). Life only needs .r (1.0 = alive); this rule also keeps
// the number of steps the cell has been alive in .g, for the render shader to colour.
//
// Life is "B3/S23": a dead cell is Born with exactly 3 live neighbours, a live cell Survives
// with 2 or 3. Try HighLife (B36/S23: also born with 6) or Seeds (B2/S: born with 2, never
// survives) by editing the two marked lines.

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours(x, y);
    let me = alive(x, y);

    let survives = me && (n == 2u || n == 3u); // S23
    let born = !me && n == 3u;                 // B3
    let next = survives || born;

    // select(if_false, if_true, condition): one more step of age while it stays alive.
    let age = select(0.0, cell(x, y).g + 1.0, next && me);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

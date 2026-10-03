// TUTORIAL 8: a one-dimensional automaton drawn as a space-time diagram. The grid is in 1D
// mode (Grid popover): each step computes one new ROW from the row above it, and the picture
// scrolls once it reaches the bottom.
//   prev_alive(x) -> bool   was column x on in the previous row?
//   prev_cell(x)  -> vec4   its full value
// Never read the previous row with cell(x, y - 1) here: once the diagram scrolls, that is the
// wrong row. prev_alive always reads the right one.
//
// An "elementary" automaton looks at three cells of the previous row (left, centre, right).
// That is 8 possible patterns, and the 8 bits of `rule_number` say which of them turn the
// new cell on. 30 is chaotic, 90 draws a Sierpinski triangle, 110 is Turing complete,
// 184 models traffic (try it from a random init).
// @param rule_number: i32 = 30 range 0 .. 255

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let l = u32(prev_alive(x - 1));
    let c = u32(prev_alive(x));
    let r = u32(prev_alive(x + 1));
    let pattern = (l << 2u) | (c << 1u) | r;              // 0 .. 7, e.g. 110 (binary) = 6
    let bit = (u32(params.rule_number) >> pattern) & 1u;  // that bit of the rule number
    return on_if(bit == 1u);
}

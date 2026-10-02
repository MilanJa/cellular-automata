// TEMPLATE: 1D automaton drawn as a space-time diagram (each step adds one row).
//
// In 1D mode `rule` runs for every cell of the NEW row. Read the previous row with:
//   prev_alive(x) -> bool   was column x on in the previous row?
//   prev_cell(x)  -> vec4   its full value
// Do not use cell(x, y - 1) here: once the diagram scrolls that is the wrong row.
//
// This is an "elementary" automaton: the new cell depends on its left, centre and right
// predecessors (8 combinations), and `rule_number` says which combinations turn it on.
// @param rule_number: i32 = 90 range 0 .. 255

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let l = u32(prev_alive(x - 1));
    let c = u32(prev_alive(x));
    let r = u32(prev_alive(x + 1));
    let idx = (l << 2u) | (c << 1u) | r;          // 0..7
    let bit = (u32(params.rule_number) >> idx) & 1u;
    return on_if(bit == 1u);
    // Ideas: look further left/right (prev_alive(x - 2)), or use more than two states by
    // storing a float in .r and thresholding it differently.
}

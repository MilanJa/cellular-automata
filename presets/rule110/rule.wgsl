// Elementary cellular automaton. Change `rule_number` to explore all 256 rules.
// prev_alive(x) reads the previous row (the generation before this one).
// @param rule_number: i32 = 30 range 0 .. 255

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let l = u32(prev_alive(x - 1));
    let c = u32(prev_alive(x));
    let r = u32(prev_alive(x + 1));
    let idx = (l << 2u) | (c << 1u) | r;
    let bit = (u32(params.rule_number) >> idx) & 1u;
    return on_if(bit == 1u);
}

// Elementary cellular automaton. Change `rule_number` to explore all 256 rules.
// @param rule_number: i32 = 30 range 0 .. 255

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let l = u32(prev_cell(x - 1).r > 0.5);
    let c = u32(prev_cell(x).r > 0.5);
    let r = u32(prev_cell(x + 1).r > 0.5);
    let idx = (l << 2u) | (c << 1u) | r;
    let on = (u32(params.rule_number) >> idx) & 1u;
    return vec4<f32>(f32(on), 0.0, 0.0, 1.0);
}

// TEMPLATE: 2D binary automaton (each cell is on or off).
//
// `rule` is called once per cell per step and returns the cell's NEXT state.
// A cell is a vec4<f32>: .r is used as on/off (1.0 or 0.0); .g .b .a are yours to use.
//
// Helpers you can call (all read the PREVIOUS generation, with wraparound edges):
//   alive(x, y)       -> bool   is the cell at (x, y) on?
//   neighbours(x, y)  -> u32    how many of the 8 surrounding cells are on (0..8)
//   neighbours4(x, y) -> u32    same for the 4 orthogonal neighbours
//   cell(x, y)        -> vec4   the full previous value
//   on_if(flag)       -> vec4   on() when flag is true, off() otherwise
//   noise(pos)        -> f32    a random number 0..1, different every cell and step
//
// Sliders: declare them in a comment, then read them as params.<name>:
// @param birth_min: i32 = 3 range 0 .. 8
// @param survive_min: i32 = 2 range 0 .. 8
// @param survive_max: i32 = 3 range 0 .. 8

fn rule(pos: vec2<u32>) -> vec4<f32> {
    // Coordinates are unsigned; convert to i32 so you can subtract without wrapping.
    let x = i32(pos.x);
    let y = i32(pos.y);

    let n = i32(neighbours(x, y));
    let me = alive(x, y);

    // Change these two lines to invent a new rule.
    let born = !me && n == params.birth_min;
    let survives = me && n >= params.survive_min && n <= params.survive_max;

    return on_if(born || survives);
}

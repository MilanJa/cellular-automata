// Gosper's glider gun: the first pattern found that grows without end. Every 30 steps it
// fires a glider towards the bottom right. The grid wraps around, so the stream eventually
// comes back and crashes into the gun.
//
// The gun is 36 cells wide, too wide for the 32-bit pattern rows the Acorn preset uses, so
// its live cells are listed as coordinates (x, y from the gun's top-left corner) instead.
const GUN_CELLS = 36;
const GUN = array<vec2<i32>, GUN_CELLS>(
    vec2<i32>(24, 0),
    vec2<i32>(22, 1), vec2<i32>(24, 1),
    vec2<i32>(12, 2), vec2<i32>(13, 2), vec2<i32>(20, 2), vec2<i32>(21, 2), vec2<i32>(34, 2), vec2<i32>(35, 2),
    vec2<i32>(11, 3), vec2<i32>(15, 3), vec2<i32>(20, 3), vec2<i32>(21, 3), vec2<i32>(34, 3), vec2<i32>(35, 3),
    vec2<i32>(0, 4), vec2<i32>(1, 4), vec2<i32>(10, 4), vec2<i32>(16, 4), vec2<i32>(20, 4), vec2<i32>(21, 4),
    vec2<i32>(0, 5), vec2<i32>(1, 5), vec2<i32>(10, 5), vec2<i32>(14, 5), vec2<i32>(16, 5), vec2<i32>(17, 5),
    vec2<i32>(22, 5), vec2<i32>(24, 5),
    vec2<i32>(10, 6), vec2<i32>(16, 6), vec2<i32>(24, 6),
    vec2<i32>(11, 7), vec2<i32>(15, 7),
    vec2<i32>(12, 8), vec2<i32>(13, 8)
);

fn seed(pos: vec2<u32>) -> vec4<f32> {
    // The gun's top-left corner sits a quarter of the way in from the grid's top-left corner.
    let origin = vec2<i32>(globals.size) / 4 - vec2<i32>(18, 4);
    let p = vec2<i32>(pos) - origin;
    if (!in_box(p, vec2<i32>(0, 0), vec2<i32>(36, 9))) {
        return off();
    }
    for (var i = 0; i < GUN_CELLS; i++) {
        if (all(GUN[i] == p)) {
            return on();
        }
    }
    return off();
}

// Conway's Game of Life. r = alive (0/1), g = age in steps (for colouring).

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    var n = 0u;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) { continue; }
            n += u32(cell(x + dx, y + dy).r > 0.5);
        }
    }
    let me = cell(x, y);
    let alive = me.r > 0.5;
    let next = (alive && (n == 2u || n == 3u)) || (!alive && n == 3u);
    let age = select(0.0, me.g + 1.0, next && alive);
    return vec4<f32>(f32(next), age, 0.0, 1.0);
}

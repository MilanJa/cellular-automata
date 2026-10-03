// Slither: scatter starter snakes. Each is a lone head with some growth owed, so it stretches
// into a body as it moves; the owed growth varies so the snakes differ in size from the start,
// which is what gets the hunting going. Every snake gets a random id (stored in .r above the
// kind and direction) so it can tell its own body from everyone else's; its counters start at
// zero (see the cell layout in rule.wgsl).
// Food (stationary one-cell morsels worth one cell of growth) is scattered too.
// @param starters: f32 = 0.0012 range 0.0 .. 0.01
// @param food_start: f32 = 0.003 range 0.0 .. 0.05

fn seed(pos: vec2<u32>) -> vec4<f32> {
    if (chance(pos, params.starters)) {
        let heading = i32(rand(pos, 3u) * 8.0) % 8;
        let growth = 1 + i32(rand(pos, 5u) * 2.0 * f32(params.spawn_length));
        let id = 1 + i32(rand(pos, 11u) * 4094.0);
        return vec4<f32>(f32(2 * 8 + heading + 32 * id), f32(growth), 0.0, 0.0);
    }
    if (rand(pos, 9u) < params.food_start) {
        return vec4<f32>(f32(3 * 8), 0.0, 0.0, 0.0); // kind 3 = food
    }
    return vec4<f32>(0.0);
}

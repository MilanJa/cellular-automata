// Slither Garden, layer B: the scent the snakes hunt by, plus a garden that grows in their
// wake. Two channels of scent as in Slither:
//   .r  presence   how much snake is nearby
//   .g  weighted   the same, weighted by each emitter's length (so .g / .r is an average length)
// and two for the garden, a Life-like automaton sown by the snakes:
//   .b  alive      1.0 where a garden cell lives
//   .a  age        steps it has been alive
//
// What sows the garden (`sow`):   0 nothing   1 every body cell a snake occupies, so each
// snake drags a thick wake of soup   2 only the last four segments, a lighter wake   3 only
// cells that are being cut off, so a dying chain is released into the garden one cell at a
// time and battles leave carcasses that settle into Life's blinkers, blocks and gliders.
// What grows (`grow`):   0 Life B3/S23   1 HighLife B36/S23 (replicators)   2 Seeds B2/S
// (fireworks)   3 Day & Night B3678/S34678 (solid debris).
// Sown cells arrive one at a time, so for their first `grace` steps Life may not kill them;
// that is what lets a trail accumulate into a line before it starts to evolve. `lifetime`
// lets garden cells die of old age (0 = never); `trample` clears the garden under a snake's
// head, which is also how heads eat it when layer A has `eat_life` on.
// The garden is also scent terrain: inside living garden cells diffusion is scaled by
// `thicket`, so a thicket holds smells in and keeps them out. Prey hiding in debris is hard to
// smell, hunters lose trails at a garden's edge, and the gaps between structures carry scent
// like corridors. 1 means the garden is transparent to scent.
// @param thicket: f32 = 0.1 range 0.0 .. 1.0
// @param sow: i32 = 3 range 0 .. 3
// @param grace: i32 = 12 range 0 .. 60
// @param grow: i32 = 0 range 0 .. 3
// @param lifetime: i32 = 600 range 0 .. 2000
// @param trample: bool = true
// @param diffusion: f32 = 0.2 range 0.0 .. 0.25
// @param decay: f32 = 0.002 range 0.0 .. 0.05
// @param emit: f32 = 0.05 range 0.0 .. 0.5

const DIRS = array<vec2<i32>, 8>(
    vec2<i32>(1, 0), vec2<i32>(1, 1), vec2<i32>(0, 1), vec2<i32>(-1, 1),
    vec2<i32>(-1, 0), vec2<i32>(-1, -1), vec2<i32>(0, -1), vec2<i32>(1, -1)
);
// Birth and survival masks (bit n = n live neighbours) for the four rules.
const BIRTH = array<u32, 4>(8u, 72u, 4u, 456u);
const SURVIVE = array<u32, 4>(12u, 12u, 0u, 472u);

// Layer A's cell layout (see the Slither rule): kind and id in .r, stamp + 2048 * moves in .b,
// the tail's stamp in .a.
fn kind_of(c: vec4<f32>) -> i32 {
    return (i32(round(c.r)) % 64) / 8;
}

fn dir_of(c: vec4<f32>) -> i32 {
    return i32(round(c.r)) % 8;
}

fn id_of(c: vec4<f32>) -> i32 {
    return i32(round(c.r)) / 64;
}

fn snake_length(c: vec4<f32>) -> f32 {
    if (kind_of(c) == 3) {
        return 0.5;
    }
    let moves = i32(round(c.b)) / 2048;
    let tail_stamp = i32(round(c.a));
    return f32(((moves - tail_stamp) % 2048 + 2048) % 2048 + 1);
}

// Is this body cell one of the last four of its snake (its stamp within four of the tail's)?
fn is_near_tail(c: vec4<f32>) -> bool {
    let from_tail = ((i32(round(c.b)) % 2048 - i32(round(c.a))) % 2048 + 2048) % 2048;
    return kind_of(c) == 1 && from_tail < 4;
}

// Is this body cell about to die, cut off from the segment ahead of it?
fn is_cut_off(p: vec2<i32>, c: vec4<f32>) -> bool {
    if (kind_of(c) != 1) {
        return false;
    }
    let ahead = p + DIRS[dir_of(c)];
    let ac = other(ahead.x, ahead.y);
    let ak = kind_of(ac);
    let attached = id_of(ac) == id_of(c) && (ak == 1 || (ak == 2 && dir_of(ac) == dir_of(c)));
    return !attached;
}

fn garden(x: i32, y: i32) -> u32 {
    return u32(cell(x, y).b > 0.5);
}

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let p = vec2<i32>(pos);
    let c = cell(x, y);

    // Scent: diffuse (slowly inside the garden), decay, and take in what the snakes emit.
    let lap = laplacian(x, y);
    let diffusion = params.diffusion * select(1.0, params.thicket, c.b > 0.5);
    var s = (c.rg + diffusion * lap.rg) * (1.0 - params.decay);
    let snake = other(x, y);
    let kind = kind_of(snake);
    if (snake.r > 0.5) {
        s += vec2<f32>(params.emit, params.emit * snake_length(snake));
    }

    // Garden: a Life-like step over .b with the chosen rule.
    let n = garden(x - 1, y - 1) + garden(x, y - 1) + garden(x + 1, y - 1) + garden(x - 1, y) + garden(x + 1, y)
        + garden(x - 1, y + 1) + garden(x, y + 1) + garden(x + 1, y + 1);
    let r = clamp(params.grow, 0, 3);
    let was = c.b > 0.5;
    let mask = select(BIRTH[r], SURVIVE[r], was);
    var alive = ((mask >> n) & 1u) == 1u;
    // Sown cells start with a negative age and may not die until it reaches zero; cells born
    // by the rule start at zero and get no such protection, or growth would run away.
    if (was && c.a < 0.0) {
        alive = true;
    }
    if (was && params.lifetime > 0 && c.a >= f32(params.lifetime)) {
        alive = false;
    }
    var age = select(0.0, c.a + 1.0, was);
    // Sowing by the snakes.
    let sow = (params.sow == 1 && kind == 1) || (params.sow == 2 && is_near_tail(snake))
        || (params.sow == 3 && is_cut_off(p, snake));
    if (sow) {
        if (!was) {
            age = -f32(params.grace);
        }
        alive = true;
    }
    // A head tramples (and, in layer A, eats) whatever grows under it. Food replaces garden:
    // that is how a ripened garden cell turns into a morsel (layer A's `ripen`).
    if ((kind == 2 && params.trample != 0u) || kind == 3) {
        alive = false;
    }
    return vec4<f32>(max(s, vec2<f32>(0.0)), f32(alive), select(0.0, age, alive));
}

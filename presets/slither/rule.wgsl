// Slither: snakes that hunt each other, as a cellular automaton.
//
// A snake is a chain of cells. Each body cell stores the direction to the segment ahead of it,
// so the chain is a linked list laid on the grid; the head stores the direction it last moved.
// Every `every` steps the snakes move: the cell in front of a head becomes the new head and
// the tail cell (the one nobody points at) empties, unless the snake still owes itself growth.
// Heads steer towards the smell of shorter snakes and away from longer ones (the scent field is
// layer B, read with other()). A head that moves into a cell of a shorter snake eats it: the
// eater grows by the bitten-off part and the victim's cut-off chain dies from the break, one
// cell per step. A head that runs into a clearly longer snake dies; one of about its own size
// only blocks it. A snake never bites or runs into itself: its own body only ever blocks it,
// and the steering avoids dead ends.
//
// Food is a fourth kind of cell: it never moves, smells like a very small snake so every head
// is drawn to it, and feeds one cell of growth to the head that moves onto it.
//
// Cell layout:  .r = kind * 8 + direction + 32 * id   (kind 0 empty, 1 body, 2 head, 3 food;
//                    the id tells a snake its own cells from everyone else's)
//               .g = growth this cell will spend once it is the tail: the size of the meal
//                    that created it (a cell keeps this value for life, so it reaches the
//                    tail exactly once; starter snakes carry their initial growth this way)
//               .b = stamp + 2048 * moves   The head counts its moves. Every cell is stamped
//                    with the count at its birth and keeps hearing the latest count from the
//                    cells ahead, so its index from the head is (moves - stamp): a difference
//                    of counters, which a waiting head cannot inflate. (Both wrap at 2048.)
//               .a = the snake's length as this cell knows it (flows tail -> head)
// Everything a cell decides, its neighbours can recompute from the same previous state, which
// is how a head and the cell it moves into agree without talking.
// @param every: i32 = 3 range 1 .. 20
// @param hunger: f32 = 6.0 range 0.0 .. 20.0
// @param appetite: f32 = 3.0 range 0.0 .. 10.0
// @param wander: f32 = 0.5 range 0.0 .. 3.0
// @param straight: f32 = 0.1 range 0.0 .. 3.0
// @param spawn: f32 = 0.000002 range 0.0 .. 0.00005
// @param spawn_length: i32 = 6 range 1 .. 30
// @param food: f32 = 0.00002 range 0.0 .. 0.0005

const EMPTY = 0;
const BODY = 1;
const HEAD = 2;
const FOOD = 3;
// Food counts as a snake of length one half, so even a lone head may eat it.
const FOOD_LENGTH = 0.5;
const NOBODY = vec2<i32>(-9999, -9999);
const WRAP = 2048;
const DIRS = array<vec2<i32>, 8>(
    vec2<i32>(1, 0), vec2<i32>(1, 1), vec2<i32>(0, 1), vec2<i32>(-1, 1),
    vec2<i32>(-1, 0), vec2<i32>(-1, -1), vec2<i32>(0, -1), vec2<i32>(1, -1)
);

fn kind_of(c: vec4<f32>) -> i32 {
    return (i32(round(c.r)) % 32) / 8;
}

fn dir_of(c: vec4<f32>) -> i32 {
    return i32(round(c.r)) % 8;
}

fn id_of(c: vec4<f32>) -> i32 {
    return i32(round(c.r)) / 32;
}

fn growth_of(c: vec4<f32>) -> i32 {
    return i32(round(c.g));
}

fn stamp_of(c: vec4<f32>) -> i32 {
    return i32(round(c.b)) % WRAP;
}

fn moves_of(c: vec4<f32>) -> i32 {
    return i32(round(c.b)) / WRAP;
}

fn index_of(c: vec4<f32>) -> i32 {
    return (moves_of(c) - stamp_of(c) + WRAP) % WRAP;
}

fn snake(kind: i32, dir: i32, id: i32, growth: i32, stamp: i32, moves: i32, len: f32) -> vec4<f32> {
    let counters = ((stamp % WRAP) + WRAP) % WRAP + WRAP * (((moves % WRAP) + WRAP) % WRAP);
    return vec4<f32>(f32(kind * 8 + dir + 32 * id), f32(growth), f32(counters), len);
}

fn empty() -> vec4<f32> {
    return vec4<f32>(0.0);
}

fn food() -> vec4<f32> {
    return snake(FOOD, 0, 0, 0, 0, 0, FOOD_LENGTH);
}

fn moving() -> bool {
    return (globals.frame % u32(max(params.every, 1))) == 0u;
}

// Grid coordinates wrapped into range, for hashing (cell() and other() wrap by themselves).
fn wrapped(p: vec2<i32>) -> vec2<u32> {
    return vec2<u32>(wrap(p.x, globals.size.x), wrap(p.y, globals.size.y));
}

// Can the snake `me` (id, length) move into the cell holding `tc`? Empty cells and food, yes;
// another snake only when it is clearly shorter (lengths are estimates that lag behind growth,
// so "clearly" leaves a margin); its own body and anything of about its size or longer, no.
fn can_enter(tc: vec4<f32>, my_id: i32, my_len: f32) -> bool {
    let k = kind_of(tc);
    return k == EMPTY || k == FOOD || (id_of(tc) != my_id && tc.a * 1.15 + 1.0 < my_len);
}

// Would entering the cell holding `tc` kill the snake `me`? Only another snake that is clearly
// longer, half again plus two cells; anything between that and "clearly shorter" only blocks.
fn is_deadly(tc: vec4<f32>, my_id: i32, my_len: f32) -> bool {
    let k = kind_of(tc);
    return (k == BODY || k == HEAD) && id_of(tc) != my_id && tc.a > my_len * 1.5 + 2.0;
}

// Where the head at `p` (state `h`) moves this step. Every direction but straight back is a
// candidate, scored by what is there (food, a block or death), by how open the cell beyond it
// is (so the snake does not steer into a dead end), by the scent four cells ahead (prey smells
// good, bigger snakes smell bad), by a little momentum and a little randomness.
fn decide(p: vec2<i32>, h: vec4<f32>) -> i32 {
    let d = dir_of(h);
    let my_id = id_of(h);
    let my_len = h.a;
    var best = d;
    var best_score = -1e30;
    for (var k = -3; k <= 3; k++) {
        let c = (d + k + 8) % 8;
        let t = p + DIRS[c];
        let tc = cell(t.x, t.y);
        // Sharp turns only when the gentler options are bad.
        var score = -0.1 * f32(abs(k)) - select(0.0, 2.0, abs(k) == 3);
        if (k == 0) {
            score += params.straight;
        }
        if (!can_enter(tc, my_id, my_len)) {
            score -= select(500.0, 1000.0, is_deadly(tc, my_id, my_len));
        } else if (kind_of(tc) != EMPTY) {
            score += params.appetite;
        }
        // Look one cell further: how many of the three cells beyond `t` could I enter next?
        var open = 0;
        for (var j = -1; j <= 1; j++) {
            let beyond = t + DIRS[(c + j + 8) % 8];
            if (can_enter(cell(beyond.x, beyond.y), my_id, my_len)) {
                open += 1;
            }
        }
        score -= select(0.3 * f32(3 - open), 50.0, open == 0);
        let ahead = p + DIRS[c] * 4;
        let s = other(ahead.x, ahead.y);
        let avg_len = s.g / max(s.r, 1e-4);
        let relative = (my_len - avg_len) / (my_len + avg_len + 1.0);
        score += params.hunger * (s.r / (s.r + 1.0)) * relative;
        score += (rand(wrapped(p), globals.frame * 8u + u32(c)) - 0.5) * params.wander;
        if (score > best_score) {
            best_score = score;
            best = c;
        }
    }
    return best;
}

// The head that enters cell `t` this step, or NOBODY. Only heads that aim at `t` and may enter
// it count; the longest wins, ties by hash.
fn taker_of(t: vec2<i32>) -> vec2<i32> {
    let tc = cell(t.x, t.y);
    var best = NOBODY;
    var best_len = -1.0;
    var best_tie = -1.0;
    for (var i = 0; i < 8; i++) {
        let n = t + DIRS[i];
        let nc = cell(n.x, n.y);
        if (kind_of(nc) != HEAD) {
            continue;
        }
        if (!can_enter(tc, id_of(nc), nc.a)) {
            continue;
        }
        let c = decide(n, nc);
        if (any(n + DIRS[c] != t)) {
            continue;
        }
        let tie = rand(wrapped(n), globals.frame);
        if (nc.a > best_len || (nc.a == best_len && tie > best_tie)) {
            best = n;
            best_len = nc.a;
            best_tie = tie;
        }
    }
    return best;
}

// The head that `taker` becomes when it enters a cell, having gained `gained` cells of growth
// (this new cell will spend them when it has become the tail). One more move on the counter.
fn arriving_head(taker: vec2<i32>, gained: i32) -> vec4<f32> {
    let tc = cell(taker.x, taker.y);
    let moves = moves_of(tc) + 1;
    return snake(HEAD, decide(taker, tc), id_of(tc), gained, moves, moves, tc.a);
}

// True when `c` is a cell of snake `id` (body, or head) that can pass the move count on.
fn same_snake(c: vec4<f32>, id: i32) -> bool {
    let k = kind_of(c);
    return (k == BODY || k == HEAD) && id_of(c) == id;
}

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let p = vec2<i32>(pos);
    let c = cell(p.x, p.y);
    let k = kind_of(c);
    let advancing = moving();

    if (k == EMPTY) {
        if (!advancing) {
            return empty();
        }
        let taker = taker_of(p);
        if (taker.x > -9000) {
            return arriving_head(taker, 0);
        }
        // Nothing around: now and then a new snake starts here, or a morsel of food appears.
        if (neighbours(p.x, p.y) == 0u) {
            let roll = noise(pos);
            if (roll < params.spawn) {
                let heading = i32(rand(pos, 77u) * 8.0) % 8;
                let id = 1 + i32(rand(pos, 78u) * 4094.0);
                return snake(HEAD, heading, id, params.spawn_length, 0, 0, 1.0);
            }
            if (roll < params.spawn + params.food) {
                return food();
            }
        }
        return empty();
    }

    if (k == FOOD) {
        // Food waits to be eaten; the head that takes it gains one cell.
        if (advancing) {
            let taker = taker_of(p);
            if (taker.x > -9000) {
                return arriving_head(taker, 1);
            }
        }
        return c;
    }

    if (k == HEAD) {
        let my_id = id_of(c);
        let moves = moves_of(c);
        // My length is whatever the segment behind me knows; a lone head is a snake of one.
        let back = p - DIRS[dir_of(c)];
        let bc = cell(back.x, back.y);
        let has_body = kind_of(bc) == BODY && dir_of(bc) == dir_of(c) && id_of(bc) == my_id;
        let len = select(1.0, bc.a, has_body);
        if (advancing && !has_body && growth_of(c) == 0) {
            return empty(); // nothing behind it and nothing to grow: a bitten-off remnant starves
        }
        if (!advancing) {
            return snake(HEAD, dir_of(c), my_id, growth_of(c), moves, moves, len);
        }
        // Eaten by a longer head moving in? It takes this cell and gains the body behind it.
        let taker = taker_of(p);
        if (taker.x > -9000) {
            return arriving_head(taker, max(i32(round(c.a)) - 1, 0));
        }
        let d = decide(p, c);
        let t = p + DIRS[d];
        let tc = cell(t.x, t.y);
        if (is_deadly(tc, my_id, c.a)) {
            return empty(); // ran into something clearly longer than myself: dead
        }
        if (all(taker_of(t) == p)) {
            // Moved on: this cell is now my neck, born at move `moves`, and it already knows
            // the head has made one more.
            return snake(BODY, d, my_id, growth_of(c), moves, moves + 1, len);
        }
        // Blocked: lost the race for that cell, or it holds something I may not enter (my own
        // body, or a snake of about my size). Wait for the way to clear; the counter stays.
        return snake(HEAD, dir_of(c), my_id, growth_of(c), moves, moves, len);
    }

    // BODY. Still attached? The segment ahead must be my body, or my head that came from here.
    let my_id = id_of(c);
    let d = dir_of(c);
    let ahead = p + DIRS[d];
    let ac = cell(ahead.x, ahead.y);
    let ak = kind_of(ac);
    let attached = id_of(ac) == my_id && (ak == BODY || (ak == HEAD && dir_of(ac) == d));
    if (!attached) {
        return empty(); // cut off: the break travels down the chain
    }
    // Is there a segment behind me (one of mine that points at me)? Length news comes from the
    // tail along the chain; taking it from two segments back, when there are two, carries it
    // twice as fast, so a bitten snake learns its new size and a growing one its growth sooner.
    var has_tail = false;
    var behind = p;
    var tail_len = 0.0;
    for (var i = 0; i < 8; i++) {
        let n = p + DIRS[i];
        let nc = cell(n.x, n.y);
        if (kind_of(nc) == BODY && id_of(nc) == my_id && all(n + DIRS[dir_of(nc)] == p)) {
            has_tail = true;
            behind = n;
            tail_len = nc.a;
        }
    }
    if (has_tail) {
        for (var i = 0; i < 8; i++) {
            let m = behind + DIRS[i];
            let mc = cell(m.x, m.y);
            if (kind_of(mc) == BODY && id_of(mc) == my_id && all(m + DIRS[dir_of(mc)] == behind)) {
                tail_len = mc.a;
            }
        }
    }
    // The latest move count travels down the chain; reading two segments ahead carries it
    // twice as fast too.
    var moves = moves_of(ac);
    if (ak == BODY) {
        let further = ahead + DIRS[dir_of(ac)];
        let fc = cell(further.x, further.y);
        if (same_snake(fc, my_id) && ((moves_of(fc) - moves + WRAP) % WRAP) < WRAP / 2) {
            moves = moves_of(fc);
        }
    }
    let stamp = stamp_of(c);
    let index = (moves - stamp + WRAP) % WRAP;
    let len = select(f32(index + 1), tail_len, has_tail);
    if (advancing) {
        let taker = taker_of(p);
        if (taker.x > -9000) {
            // Bitten: the eater takes this cell and gains everything behind it, which dies.
            return arriving_head(taker, max(i32(round(c.a)) - index_of(c) - 1, 0));
        }
        if (!has_tail) {
            // The tail: spend one of the growth this cell carries, or move on.
            if (growth_of(c) == 0) {
                return empty();
            }
            return snake(BODY, d, my_id, growth_of(c) - 1, stamp, moves, len);
        }
    }
    return snake(BODY, d, my_id, growth_of(c), stamp, moves, len);
}

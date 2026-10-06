// Slither Garden: Slither's snakes, with a garden growing in their wake.
//
// The snakes are exactly Slither's (see that preset's rule for the full story): chains of
// cells with ids and move counters that hunt by scent, eat clearly shorter snakes and food,
// die against clearly longer ones, never run into themselves, grow flesh around their spine
// as they get long, and burn cells by metabolism. Layer B carries the scent and, new here, a
// Life-like garden the snakes sow as they pass (what is sown and what grows are layer B's
// sliders, in the Layer B section). Two things connect the snakes to the garden:
//   eat_life     a head moving onto a living garden cell gains a cell of growth, so snakes
//                farm their own trails and gliders become roaming snacks
//   wall_below   living garden cells block the heads of snakes shorter than this length, so
//                small snakes have to thread between the big ones' wreckage
//   ripen        a garden cell that has lived this many steps turns into food, so only the
//                stable structures bear fruit: blocks and beehives become orchards a few
//                hundred steps after the kill that sowed them, and are eaten away by it
// And unlike Slither, snakes here have momentum:
//   momentum     a snake has to go straight for momentum * length / 8 moves before it can turn
//                at all, and then turns by at most 45 degrees per move beyond that, so a long
//                snake sweeps wide arcs and cannot always swerve away from a bigger snake or
//                from prey it would rather not hit; 0 turns as freely as Slither's snakes
//
// Cell layout:  .r = kind * 8 + direction + 64 * id   (kind 0 empty, 1 body, 2 head, 3 food,
//                    4 flesh)
//               .g = growth this cell will spend once it is the tail; for flesh, the offset to
//                    its spine cell as (dx + 3) + 7 * (dy + 3)
//               .b = stamp + 2048 * moves  (index from the head = moves - stamp)
//               .a = the tail's stamp as last heard here  (length = moves - tail stamp + 1)
// @param every: i32 = 3 range 1 .. 20
// @param hunger: f32 = 6.0 range 0.0 .. 20.0
// @param appetite: f32 = 3.0 range 0.0 .. 10.0
// @param wander: f32 = 0.5 range 0.0 .. 3.0
// @param straight: f32 = 0.1 range 0.0 .. 3.0
// @param spawn: f32 = 0.000002 range 0.0 .. 0.00005
// @param spawn_length: i32 = 6 range 1 .. 30
// @param food: f32 = 0.00002 range 0.0 .. 0.0005
// @param metabolism: i32 = 40 range 0 .. 120
// @param width: f32 = 1.0 range 0.0 .. 3.0
// @param eat_life: bool = true
// @param wall_below: i32 = 0 range 0 .. 60
// @param ripen: i32 = 200 range 0 .. 2000
// @param momentum: f32 = 1.0 range 0.0 .. 4.0

const EMPTY = 0;
const BODY = 1;
const HEAD = 2;
const FOOD = 3;
const FLESH = 4;
const NOBODY = vec2<i32>(-9999, -9999);
const WRAP = 2048;
const MAX_WIDTH = 3;
const MAX_RUN = 16;
const DIRS = array<vec2<i32>, 8>(
    vec2<i32>(1, 0), vec2<i32>(1, 1), vec2<i32>(0, 1), vec2<i32>(-1, 1),
    vec2<i32>(-1, 0), vec2<i32>(-1, -1), vec2<i32>(0, -1), vec2<i32>(1, -1)
);

fn kind_of(c: vec4<f32>) -> i32 {
    return (i32(round(c.r)) % 64) / 8;
}

fn dir_of(c: vec4<f32>) -> i32 {
    return i32(round(c.r)) % 8;
}

fn id_of(c: vec4<f32>) -> i32 {
    return i32(round(c.r)) / 64;
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

fn tail_stamp_of(c: vec4<f32>) -> i32 {
    return i32(round(c.a));
}

fn since(later: i32, earlier: i32) -> i32 {
    return ((later - earlier) % WRAP + WRAP) % WRAP;
}

fn len_of(c: vec4<f32>) -> f32 {
    if (kind_of(c) == FOOD) {
        return 0.5;
    }
    return f32(since(moves_of(c), tail_stamp_of(c)) + 1);
}

fn width_of(len: f32) -> i32 {
    return clamp(i32(floor(params.width * 0.5 * log2(max(len, 1.0) / 8.0))), 0, MAX_WIDTH);
}

fn in_ring(off: vec2<i32>, w: i32) -> bool {
    let d2 = off.x * off.x + off.y * off.y;
    return d2 > 0 && d2 <= w * w + 1;
}

fn is_spine(c: vec4<f32>) -> bool {
    let k = kind_of(c);
    return k == BODY || k == HEAD;
}

fn snake(kind: i32, dir: i32, id: i32, growth: i32, stamp: i32, moves: i32, tail_stamp: i32) -> vec4<f32> {
    let counters = since(stamp, 0) + WRAP * since(moves, 0);
    return vec4<f32>(f32(kind * 8 + dir + 64 * id), f32(growth), f32(counters), f32(since(tail_stamp, 0)));
}

fn empty() -> vec4<f32> {
    return vec4<f32>(0.0);
}

fn food() -> vec4<f32> {
    return snake(FOOD, 0, 0, 0, 0, 0, 0);
}

fn flesh(spine: vec4<f32>, off: vec2<i32>) -> vec4<f32> {
    let code = (off.x + 3) + 7 * (off.y + 3);
    return vec4<f32>(f32(FLESH * 8 + 64 * id_of(spine)), f32(code), spine.b, spine.a);
}

fn flesh_offset(c: vec4<f32>) -> vec2<i32> {
    let code = i32(round(c.g));
    return vec2<i32>(code % 7 - 3, code / 7 - 3);
}

fn moving() -> bool {
    return (globals.frame % u32(max(params.every, 1))) == 0u;
}

fn wrapped(p: vec2<i32>) -> vec2<u32> {
    return vec2<u32>(wrap(p.x, globals.size.x), wrap(p.y, globals.size.y));
}

// Does the garden live at `t` (layer B's blue channel)?
fn garden_at(t: vec2<i32>) -> bool {
    return other(t.x, t.y).b > 0.5;
}

// Can the snake `me` (id, length) move into cell `t` holding `tc`? Empty cells yes, unless the
// garden grows there and I am too short to plough through it; food yes; my own flesh yes;
// another snake's flesh only when I am clearly longer; another snake's spine only when it is
// clearly shorter; my own spine and anything of about my size or longer, no.
fn can_enter(t: vec2<i32>, tc: vec4<f32>, my_id: i32, my_len: f32) -> bool {
    let k = kind_of(tc);
    if (k == EMPTY) {
        return !(garden_at(t) && my_len < f32(params.wall_below));
    }
    if (k == FOOD) {
        return true;
    }
    if (k == FLESH) {
        return id_of(tc) == my_id || my_len > len_of(tc) * 1.15 + 1.0;
    }
    return id_of(tc) != my_id && len_of(tc) * 1.15 + 1.0 < my_len;
}

fn is_deadly(tc: vec4<f32>, my_id: i32, my_len: f32) -> bool {
    let k = kind_of(tc);
    return (k == BODY || k == HEAD || k == FLESH) && id_of(tc) != my_id && len_of(tc) > my_len * 1.5 + 2.0;
}

// How many moves the head at `p` (state `h`) has gone straight: the spine cells right behind
// it that point the way it is heading, counted up to `limit`.
fn straight_run(p: vec2<i32>, h: vec4<f32>, limit: i32) -> i32 {
    let d = dir_of(h);
    let my_id = id_of(h);
    var q = p;
    var run = 0;
    for (; run < limit; run++) {
        q -= DIRS[d];
        let c = cell(q.x, q.y);
        if (kind_of(c) != BODY || id_of(c) != my_id || dir_of(c) != d) {
            break;
        }
    }
    return run;
}

// How far the head may turn this move, in 45-degree steps: freely while the snake is short,
// otherwise only once it has run straight for momentum * length / 8 moves, one more step for
// each move beyond that.
fn max_turn(p: vec2<i32>, h: vec4<f32>) -> i32 {
    let need = min(i32(params.momentum * len_of(h) / 8.0), MAX_RUN);
    if (need == 0) {
        return 3;
    }
    return clamp(straight_run(p, h, need + 3) - need, 0, 3);
}

// Where the head at `p` (state `h`) moves this step: every direction its momentum allows
// (never straight back), scored by what is there (food, garden to graze, a block or death),
// by how open the cell beyond is, by the scent four cells ahead, by a bias for going straight
// and by a little randomness.
fn decide(p: vec2<i32>, h: vec4<f32>) -> i32 {
    let d = dir_of(h);
    let my_id = id_of(h);
    let my_len = len_of(h);
    let turn = max_turn(p, h);
    var best = d;
    var best_score = -1e30;
    for (var k = -turn; k <= turn; k++) {
        let c = (d + k + 8) % 8;
        let t = p + DIRS[c];
        let tc = cell(t.x, t.y);
        var score = -0.1 * f32(abs(k)) - select(0.0, 2.0, abs(k) == 3);
        if (k == 0) {
            score += params.straight;
        }
        if (!can_enter(t, tc, my_id, my_len)) {
            score -= select(500.0, 1000.0, is_deadly(tc, my_id, my_len));
        } else if (kind_of(tc) == FOOD || is_spine(tc)) {
            score += params.appetite;
        } else if (kind_of(tc) == EMPTY && params.eat_life != 0u && garden_at(t)) {
            score += params.appetite * 0.5;
        }
        var open = 0;
        for (var j = -1; j <= 1; j++) {
            let beyond = t + DIRS[(c + j + 8) % 8];
            if (can_enter(beyond, cell(beyond.x, beyond.y), my_id, my_len)) {
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
        let n_len = len_of(nc);
        if (!can_enter(t, tc, id_of(nc), n_len)) {
            continue;
        }
        let c = decide(n, nc);
        if (any(n + DIRS[c] != t)) {
            continue;
        }
        let tie = rand(wrapped(n), globals.frame);
        if (n_len > best_len || (n_len == best_len && tie > best_tie)) {
            best = n;
            best_len = n_len;
            best_tie = tie;
        }
    }
    return best;
}

fn arriving_head(taker: vec2<i32>, gained: i32) -> vec4<f32> {
    let tc = cell(taker.x, taker.y);
    let moves = moves_of(tc) + 1;
    return snake(HEAD, decide(taker, tc), id_of(tc), gained, moves, moves, tail_stamp_of(tc));
}

fn same_snake(c: vec4<f32>, id: i32) -> bool {
    return is_spine(c) && id_of(c) == id;
}

// The flesh an empty cell at `p` becomes, if a spine cell's ring reaches it (nearest spine
// cell, ties to the longer snake); empty otherwise.
fn claim(p: vec2<i32>) -> vec4<f32> {
    var best = vec4<f32>(0.0);
    var best_off = vec2<i32>(0, 0);
    var best_d2 = 1000;
    var best_len = -1.0;
    for (var dy = -MAX_WIDTH; dy <= MAX_WIDTH; dy++) {
        for (var dx = -MAX_WIDTH; dx <= MAX_WIDTH; dx++) {
            let off = vec2<i32>(dx, dy);
            let q = p + off;
            let c = cell(q.x, q.y);
            if (!is_spine(c)) {
                continue;
            }
            let len = len_of(c);
            if (!in_ring(off, width_of(len))) {
                continue;
            }
            let d2 = dx * dx + dy * dy;
            if (d2 < best_d2 || (d2 == best_d2 && len > best_len)) {
                best = c;
                best_off = off;
                best_d2 = d2;
                best_len = len;
            }
        }
    }
    if (best_len < 0.0) {
        return empty();
    }
    return flesh(best, best_off);
}

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let p = vec2<i32>(pos);
    let c = cell(p.x, p.y);
    let k = kind_of(c);
    let advancing = moving();

    if (k == EMPTY) {
        if (advancing) {
            let taker = taker_of(p);
            if (taker.x > -9000) {
                // Grazing: a head arriving on living garden gains a cell (layer B tramples it).
                let grazed = params.eat_life != 0u && garden_at(p);
                return arriving_head(taker, select(0, 1, grazed));
            }
            if (neighbours(p.x, p.y) == 0u) {
                let roll = noise(pos);
                if (roll < params.spawn) {
                    let heading = i32(rand(pos, 77u) * 8.0) % 8;
                    let id = 1 + i32(rand(pos, 78u) * 4094.0);
                    return snake(HEAD, heading, id, params.spawn_length, 0, 0, 0);
                }
                if (roll < params.spawn + params.food) {
                    return food();
                }
            }
        }
        let claimed = claim(p);
        if (kind_of(claimed) != EMPTY) {
            return claimed;
        }
        // Ripening: garden that has stood long enough bears fruit.
        let g = other(p.x, p.y);
        if (params.ripen > 0 && g.b > 0.5 && g.a >= f32(params.ripen)) {
            return food();
        }
        return empty();
    }

    if (k == FOOD) {
        if (advancing) {
            let taker = taker_of(p);
            if (taker.x > -9000) {
                return arriving_head(taker, 1);
            }
        }
        return c;
    }

    if (k == FLESH) {
        if (advancing) {
            let taker = taker_of(p);
            if (taker.x > -9000) {
                return arriving_head(taker, 0);
            }
        }
        let off = flesh_offset(c);
        let s = cell(p.x + off.x, p.y + off.y);
        if (same_snake(s, id_of(c)) && in_ring(off, width_of(len_of(s)))) {
            return flesh(s, off);
        }
        return claim(p);
    }

    if (k == HEAD) {
        let my_id = id_of(c);
        let moves = moves_of(c);
        let back = p - DIRS[dir_of(c)];
        let bc = cell(back.x, back.y);
        let has_body = kind_of(bc) == BODY && dir_of(bc) == dir_of(c) && id_of(bc) == my_id;
        let tail_stamp = select(moves, tail_stamp_of(bc), has_body);
        if (!advancing) {
            return snake(HEAD, dir_of(c), my_id, growth_of(c), moves, moves, tail_stamp);
        }
        // Being eaten comes before anything that would remove this cell on its own.
        let taker = taker_of(p);
        if (taker.x > -9000) {
            return arriving_head(taker, max(i32(len_of(c)) - 1, 0));
        }
        if (!has_body && growth_of(c) == 0) {
            return empty(); // a bitten-off remnant starves
        }
        let d = decide(p, c);
        let t = p + DIRS[d];
        let tc = cell(t.x, t.y);
        if (is_deadly(tc, my_id, len_of(c))) {
            return empty();
        }
        if (all(taker_of(t) == p)) {
            return snake(BODY, d, my_id, growth_of(c), moves, moves + 1, tail_stamp);
        }
        return snake(HEAD, dir_of(c), my_id, growth_of(c), moves, moves, tail_stamp);
    }

    // BODY. Bitten? Checked first, for the same reason as above.
    let my_id = id_of(c);
    let stamp = stamp_of(c);
    if (advancing) {
        let taker = taker_of(p);
        if (taker.x > -9000) {
            return arriving_head(taker, max(since(stamp, tail_stamp_of(c)) - 1, 0));
        }
    }
    let d = dir_of(c);
    let ahead = p + DIRS[d];
    let ac = cell(ahead.x, ahead.y);
    let ak = kind_of(ac);
    let attached = id_of(ac) == my_id && (ak == BODY || (ak == HEAD && dir_of(ac) == d));
    if (!attached) {
        return empty();
    }
    var has_tail = false;
    var behind = p;
    var behind_growth = 0;
    var tail_stamp = stamp;
    for (var i = 0; i < 8; i++) {
        let n = p + DIRS[i];
        let nc = cell(n.x, n.y);
        if (kind_of(nc) == BODY && id_of(nc) == my_id && all(n + DIRS[dir_of(nc)] == p)) {
            has_tail = true;
            behind = n;
            behind_growth = growth_of(nc);
            tail_stamp = tail_stamp_of(nc);
        }
    }
    var behind_is_tail = has_tail;
    if (has_tail) {
        for (var i = 0; i < 8; i++) {
            let m = behind + DIRS[i];
            let mc = cell(m.x, m.y);
            if (kind_of(mc) == BODY && id_of(mc) == my_id && all(m + DIRS[dir_of(mc)] == behind)) {
                behind_is_tail = false;
                tail_stamp = tail_stamp_of(mc);
            }
        }
    }
    if (advancing && behind_is_tail && behind_growth == 0 && params.metabolism > 0) {
        let period = max(1, (32 * params.metabolism) / max(i32(len_of(c)), 1));
        let move_index = i32(globals.frame / u32(max(params.every, 1)));
        if (move_index % period == 0) {
            return empty();
        }
    }
    var moves = moves_of(ac);
    if (ak == BODY) {
        let further = ahead + DIRS[dir_of(ac)];
        let fc = cell(further.x, further.y);
        if (same_snake(fc, my_id) && since(moves_of(fc), moves) < WRAP / 2) {
            moves = moves_of(fc);
        }
    }
    if (advancing && !has_tail) {
        if (growth_of(c) == 0) {
            return empty();
        }
        return snake(BODY, d, my_id, growth_of(c) - 1, stamp, moves, tail_stamp);
    }
    return snake(BODY, d, my_id, growth_of(c), stamp, moves, tail_stamp);
}

// Plain Game of Life; this template is about the Post editor below.

fn rule(pos: vec2<u32>) -> vec4<f32> {
    let x = i32(pos.x);
    let y = i32(pos.y);
    let n = neighbours(x, y);
    let me = alive(x, y);
    return on_if((me && (n == 2u || n == 3u)) || (!me && n == 3u));
}

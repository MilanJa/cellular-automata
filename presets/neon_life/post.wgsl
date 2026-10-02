// Post-processing runs on the finished picture, not on cells. `color` is this pixel,
// scene(uv) samples the picture anywhere, prev(uv) is last frame's output (for feedback).
// @param bloom: f32 = 0.6 range 0.0 .. 2.0
// @param bloom_size: f32 = 2.5 range 0.5 .. 8.0
// @param feedback: f32 = 0.25 range 0.0 .. 0.95

fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    let px = scene_px() * params.bloom_size;
    // 9-tap blur of the bright parts as a cheap bloom.
    var acc = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let s = scene(uv + vec2<f32>(f32(x), f32(y)) * px).rgb;
            acc += max(s - vec3<f32>(0.35), vec3<f32>(0.0));
        }
    }
    let glow = acc / 9.0 * params.bloom;
    // Feedback: blend in a slightly faded copy of the previous frame for motion trails.
    let trail = prev(uv).rgb * params.feedback;
    let out = max(color.rgb + glow, trail);
    return vec4<f32>(out, 1.0);
}

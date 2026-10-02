// TEMPLATE: post-processing. This stage sees the finished picture, not cells.
//   color       this pixel as the Render editor produced it
//   scene(uv)   the picture at any position (0..1), linearly filtered
//   prev(uv)    LAST frame's post output: blend it in for trails and feedback
//   scene_px()  the size of one picture pixel in uv units
// Everything below is optional; set a slider to 0 to switch an effect off.
// @param curvature: f32 = 0.12 range 0.0 .. 0.5
// @param aberration: f32 = 1.5 range 0.0 .. 6.0
// @param scanlines: f32 = 0.35 range 0.0 .. 1.0
// @param feedback: f32 = 0.6 range 0.0 .. 0.97
// @param vignette: f32 = 0.5 range 0.0 .. 1.0

fn post(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
    // Barrel distortion: bend uv towards the corners.
    let c = uv - vec2<f32>(0.5);
    let r2 = dot(c, c);
    let warped = vec2<f32>(0.5) + c * (1.0 + params.curvature * r2 * 2.0);
    if (any(warped < vec2<f32>(0.0)) || any(warped > vec2<f32>(1.0))) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // Chromatic aberration: sample each channel slightly offset.
    let off = scene_px() * params.aberration * r2 * 4.0;
    let r = scene(warped + off).r;
    let g = scene(warped).g;
    let b = scene(warped - off).b;
    var col = vec3<f32>(r, g, b);
    // Feedback trail from the previous frame.
    col = max(col, prev(warped).rgb * params.feedback);
    // Scanlines (one per picture row pair) and vignette.
    let line = 0.5 + 0.5 * sin(warped.y / scene_px().y * 3.14159);
    col *= 1.0 - params.scanlines * 0.6 * line;
    col *= 1.0 - params.vignette * r2 * 2.5;
    return vec4<f32>(col, 1.0);
}

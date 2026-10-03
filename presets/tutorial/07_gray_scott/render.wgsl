// TUTORIAL 7: colouring a continuous value. V (cell.g) is small, so `gain` stretches it to
// 0 .. 1 before it is turned into a colour by palette(t).
//   palette(t) -> vec3   a smooth built-in gradient for t in 0 .. 1
// @param gain: f32 = 3.0 range 0.5 .. 10.0
// @param shift: f32 = 0.6 range 0.0 .. 1.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = clamp(cell.g * params.gain, 0.0, 1.0);
    let colour = palette(fract(params.shift + t * 0.5)) * t;
    return vec4<f32>(colour, 1.0);
}

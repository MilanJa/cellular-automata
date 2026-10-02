// TEMPLATE: map a 0..1 value to colour.
//   gray(t)     -> vec4   black to white
//   palette(t)  -> vec3   a built-in gradient
//   hsv(h,s,v)  -> vec3   hue 0..1, saturation, value
// @param hue_shift: f32 = 0.6 range 0.0 .. 1.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = cell.r;
    let col = hsv(fract(params.hue_shift + t * 0.3), 0.8, t);
    return vec4<f32>(col, 1.0);
}

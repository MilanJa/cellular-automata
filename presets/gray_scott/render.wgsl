// @param a: vec3<f32> = (0.02, 0.02, 0.08) color
// @param b: vec3<f32> = (0.1, 0.7, 0.9) color
// @param c: vec3<f32> = (1.0, 1.0, 0.8) color
// @param gain: f32 = 3.0 range 0.5 .. 10.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = clamp(cell.g * params.gain, 0.0, 1.0);
    let col = select(mix(params.a, params.b, t * 2.0), mix(params.b, params.c, t * 2.0 - 1.0), t > 0.5);
    return vec4<f32>(col, 1.0);
}

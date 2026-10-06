// @param young: vec3<f32> = (1.0, 0.9, 0.3) color
// @param old: vec3<f32> = (0.2, 0.5, 1.0) color
// @param fade: f32 = 40.0 range 1.0 .. 200.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = clamp(cell.g / params.fade, 0.0, 1.0);
    let col = mix(params.young, params.old, t) * cell.r;
    return vec4<f32>(col, 1.0);
}

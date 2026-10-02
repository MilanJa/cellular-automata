// @param fade: f32 = 60.0 range 1.0 .. 300.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    // palette(t) maps 0..1 to a pleasant gradient; older cells drift along it.
    let t = clamp(cell.g / params.fade, 0.0, 1.0);
    return vec4<f32>(palette(t) * cell.r, 1.0);
}

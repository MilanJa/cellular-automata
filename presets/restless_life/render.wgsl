// Live cells cool from `fresh` to `settled` the longer they stay unchanged, so a frozen
// structure visibly ages before restlessness breaks it; a restless flip flashes `flash`.
// @param fresh: vec3<f32> = (1.0, 0.95, 0.6) color
// @param settled: vec3<f32> = (0.25, 0.35, 0.7) color
// @param flash: vec3<f32> = (1.0, 0.3, 0.2) color
// @param fade: f32 = 40.0 range 1.0 .. 300.0

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let t = clamp(cell.g / params.fade, 0.0, 1.0);
    var colour = mix(params.fresh, params.settled, t) * cell.r;
    colour += params.flash * cell.b;
    return vec4<f32>(colour, 1.0);
}

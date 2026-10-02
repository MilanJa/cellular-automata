// Green phosphor cells; the CRT look is added in the Post editor.
// @param phosphor: vec3<f32> = (0.4, 1.0, 0.5) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(params.phosphor * cell.r, 1.0);
}

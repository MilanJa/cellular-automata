// Neon Life. The state is plain Life; everything in here is appearance:
// round glowing beads instead of squares, colour that drifts with age and crowding, a halo
// from live neighbours, warm fading trails where cells died, a slow pulse, vignette, scanlines.
// `uv` is the pixel position (0..1), so we know where inside its cell each pixel sits.
// @param dot_radius: f32 = 0.6 range 0.2 .. 1.0
// @param glow: f32 = 1.2 range 0.0 .. 2.0
// @param pulse: f32 = 0.15 range 0.0 .. 1.0
// @param vignette: f32 = 0.35 range 0.0 .. 1.0
// @param scanlines: f32 = 0.0 range 0.0 .. 1.0
// @param young: vec3<f32> = (0.35, 1.0, 0.9) color
// @param old: vec3<f32> = (1.0, 0.3, 0.8) color
// @param ember: vec3<f32> = (1.0, 0.45, 0.1) color

fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let size = vec2<f32>(globals.size);
    let p = uv * size;                   // position in cell units
    let c = vec2<i32>(floor(p));         // the cell this pixel belongs to
    let f = fract(p) - vec2<f32>(0.5);   // -0.5..0.5 inside the cell
    let d = length(f) * 2.0;             // 0 at the centre, 1 at the inscribed circle

    // Live cell drawn as a soft bead; dot_radius 1.0 turns it back into a square.
    let spot = 1.0 - smoothstep(params.dot_radius - 0.25, params.dot_radius + 0.05, d);
    let age_t = clamp(cell.g / 60.0, 0.0, 1.0);
    let tint = mix(params.young, params.old, age_t);

    // Halo from live neighbours, each glowing from its own centre, plus a crowding measure.
    var halo = 0.0;
    var crowd = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) { continue; }
            let alive_n = cell_at(c.x + dx, c.y + dy).r;
            let off = vec2<f32>(f32(dx), f32(dy));
            let dist = length(f - off);
            halo += alive_n * exp(-dist * dist * 2.5);
            crowd += alive_n;
        }
    }
    crowd /= 8.0;

    var col = vec3<f32>(0.02, 0.02, 0.05);                       // deep background
    col += 1.5 * tint * mix(1.0, 0.6, crowd) * spot * cell.r;    // the bead
    col += params.glow * 0.22 * halo * mix(tint, params.young, 0.5); // neighbour halo
    col += 0.7 * params.ember * cell.b * cell.b * cell.b * (1.0 - cell.r) * (1.0 - smoothstep(0.0, 0.7, d)); // trail

    // Slow pulse that travels with age, a vignette, optional scanlines.
    col *= 1.0 + params.pulse * 0.5 * sin(globals.time * 2.0 + age_t * 6.0);
    let v = uv - vec2<f32>(0.5);
    col *= 1.0 - params.vignette * dot(v, v) * 2.0;
    col *= 1.0 - params.scanlines * 0.5 * (0.5 + 0.5 * sin(p.y * 6.28318));

    return vec4<f32>(col, 1.0);
}

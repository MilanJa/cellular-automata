//! Mouse painting: pure helpers plus the uniform layout of the built-in paint compute pass.

use bytemuck::{Pod, Zeroable};

use crate::viewport::letterbox;

/// One brush dab in grid coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub x: i32,
    pub y: i32,
    pub radius: f32,
    pub value: [f32; 4],
}

/// Matches `PAINT_WGSL`'s `Paint` struct (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct PaintUniform {
    pub origin: [i32; 2],
    pub center: [f32; 2],
    pub radius: f32,
    pub _pad: [f32; 3],
    pub value: [f32; 4],
}

pub const PAINT_WGSL: &str = r#"struct Paint {
    origin: vec2<i32>,
    center: vec2<f32>,
    radius: f32,
    _p0: f32,
    _p1: f32,
    _p2: f32,
    value: vec4<f32>,
}
@group(0) @binding(0) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(1) var<uniform> paint: Paint;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = paint.origin + vec2<i32>(gid.xy);
    let dims = vec2<i32>(textureDimensions(dst));
    if (p.x < 0 || p.y < 0 || p.x >= dims.x || p.y >= dims.y) { return; }
    let d = distance(vec2<f32>(p) + vec2<f32>(0.5), paint.center + vec2<f32>(0.5));
    if (d <= paint.radius) {
        textureStore(dst, vec2<u32>(p), paint.value);
    }
}
"#;

/// Maps a pointer position (screen units) inside the viewport `rect = (x, y, w, h)` to a grid
/// cell, or `None` when it falls in the letterbox bars.
pub fn pointer_to_cell(px: f32, py: f32, rect: (f32, f32, f32, f32), grid_w: u32, grid_h: u32) -> Option<(i32, i32)> {
    let (rx, ry, rw, rh) = rect;
    let (lx, ly, lw, lh) = letterbox(rw, rh, grid_w as f32 / grid_h.max(1) as f32);
    let u = (px - rx - lx) / lw;
    let v = (py - ry - ly) / lh;
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return None;
    }
    Some(((u * grid_w as f32) as i32, (v * grid_h as f32) as i32))
}

/// Bounding box `(x0, y0, w, h)` of a brush, clipped to the grid; `None` when fully outside.
pub fn brush_bbox(x: i32, y: i32, radius: f32, grid_w: u32, grid_h: u32) -> Option<(u32, u32, u32, u32)> {
    let r = radius.ceil() as i32;
    let x0 = (x - r).max(0);
    let y0 = (y - r).max(0);
    let x1 = (x + r).min(grid_w as i32 - 1);
    let y1 = (y + r).min(grid_h as i32 - 1);
    if x1 < x0 || y1 < y0 {
        return None;
    }
    Some((x0 as u32, y0 as u32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_maps_through_the_letterbox_to_a_cell() {
        // 400x200 viewport at (100, 50), square 10x10 grid -> drawn in a 200x200 box at x=200.
        let rect = (100.0, 50.0, 400.0, 200.0);
        assert_eq!(pointer_to_cell(200.0, 50.0, rect, 10, 10), Some((0, 0)));
        assert_eq!(pointer_to_cell(399.9, 249.9, rect, 10, 10), Some((9, 9)));
        assert_eq!(pointer_to_cell(310.0, 150.0, rect, 10, 10), Some((5, 5)));
        // In the black bars: no cell.
        assert_eq!(pointer_to_cell(150.0, 100.0, rect, 10, 10), None);
        assert_eq!(pointer_to_cell(450.0, 100.0, rect, 10, 10), None);
    }

    #[test]
    fn brush_bbox_is_clipped_to_the_grid() {
        assert_eq!(brush_bbox(10, 10, 2.0, 32, 32), Some((8, 8, 5, 5)));
        assert_eq!(brush_bbox(0, 0, 2.0, 32, 32), Some((0, 0, 3, 3)));
        assert_eq!(brush_bbox(31, 31, 2.0, 32, 32), Some((29, 29, 3, 3)));
        assert_eq!(brush_bbox(40, 10, 2.0, 32, 32), None);
        assert_eq!(brush_bbox(10, -5, 2.0, 32, 32), None);
    }

    #[test]
    fn paint_uniform_is_48_bytes() {
        assert_eq!(std::mem::size_of::<PaintUniform>(), 48);
    }
}

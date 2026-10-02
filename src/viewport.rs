//! The egui paint callback that steps the simulation and draws it into the central panel.

use std::sync::{Arc, Mutex};

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use egui::PaintCallbackInfo;

use crate::sim::paint::Stroke;
use crate::sim::Simulation;

/// A second automaton that layer A can read through `other()`, and that reads layer A back.
pub struct LayerB {
    pub sim: Simulation,
    pub name: String,
    /// A's state mirrored for B to read.
    pub mirror_a: eframe::wgpu::Texture,
    /// B's state mirrored for A to read.
    pub mirror_b: eframe::wgpu::Texture,
}

/// Largest `grid_aspect` rectangle centred inside a `viewport_w x viewport_h` box.
/// Returns `(x, y, w, h)` in the same units as the inputs.
pub fn letterbox(viewport_w: f32, viewport_h: f32, grid_aspect: f32) -> (f32, f32, f32, f32) {
    let view_aspect = viewport_w / viewport_h;
    if view_aspect > grid_aspect {
        let w = viewport_h * grid_aspect;
        ((viewport_w - w) / 2.0, 0.0, w, viewport_h)
    } else {
        let h = viewport_w / grid_aspect;
        (0.0, (viewport_h - h) / 2.0, viewport_w, h)
    }
}

pub struct ViewportCallback {
    pub sim: Arc<Mutex<Simulation>>,
    pub steps: u32,
    pub time: f32,
    pub grid_aspect: f32,
    /// Brush strokes to apply before this frame's steps.
    pub strokes: Vec<Stroke>,
    /// Size in physical pixels of the letterboxed grid area (the scene resolution).
    pub scene_pixels: (u32, u32),
    /// Rewind request: restore this snapshot (oldest first) before anything else this frame.
    pub restore: Option<usize>,
    pub layer_b: Option<Arc<Mutex<LayerB>>>,
}

impl CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mut sim = self.sim.lock().unwrap();
        sim.set_time(self.time);
        if let Some(index) = self.restore {
            sim.restore_snapshot(egui_encoder, index);
        }
        if !self.strokes.is_empty() {
            sim.paint(egui_encoder, &self.strokes);
        }
        if let Some(b) = &self.layer_b {
            // Both layers see each other's state from before this frame's steps.
            let mut b = b.lock().unwrap();
            sim.mirror_into(egui_encoder, &b.mirror_a);
            b.sim.mirror_into(egui_encoder, &b.mirror_b);
            b.sim.set_time(self.time);
            b.sim.step(egui_encoder, self.steps);
        }
        sim.step(egui_encoder, self.steps);
        if self.steps > 0 || !self.strokes.is_empty() {
            sim.collect_stats(egui_encoder);
        }
        sim.ensure_scene_size(self.scene_pixels.0, self.scene_pixels.1);
        sim.render_scene(egui_encoder);
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _resources: &CallbackResources,
    ) {
        let vp = info.viewport_in_pixels();
        let (x, y, w, h) = letterbox(vp.width_px as f32, vp.height_px as f32, self.grid_aspect);
        if w < 1.0 || h < 1.0 {
            return;
        }
        pass.set_viewport(vp.left_px as f32 + x, vp.top_px as f32 + y, w, h, 0.0, 1.0);
        let sim = self.sim.lock().unwrap();
        sim.draw(pass);
    }
}

/// Allocates the remaining space in `ui`, paints a dark background and schedules the GPU callback.
/// Returns the viewport rect and its interaction response (click and drag for painting).
pub fn show_viewport(
    ui: &mut egui::Ui,
    sim: &Arc<Mutex<Simulation>>,
    steps: u32,
    time: f32,
    strokes: Vec<Stroke>,
    restore: Option<usize>,
    layer_b: Option<Arc<Mutex<LayerB>>>,
) -> (egui::Rect, egui::Response) {
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    ui.painter().rect_filled(rect, 0.0, crate::app::theme::VIEWPORT_BG);
    let grid_aspect = {
        let s = sim.lock().unwrap();
        let c = s.config();
        c.width as f32 / c.height.max(1) as f32
    };
    // A hairline around the grid so it reads as a framed picture against the letterbox.
    let (fx, fy, fw, fh) = letterbox(rect.width(), rect.height(), grid_aspect);
    let frame = egui::Rect::from_min_size(rect.min + egui::vec2(fx, fy), egui::vec2(fw, fh));
    ui.painter().rect_stroke(
        frame,
        0.0,
        egui::Stroke::new(1.0, crate::app::theme::VIEWPORT_FRAME),
        egui::StrokeKind::Outside,
    );
    let ppp = ui.ctx().pixels_per_point();
    let (_, _, lw, lh) = letterbox(rect.width() * ppp, rect.height() * ppp, grid_aspect);
    let scene_pixels = ((lw.round() as u32).max(1), (lh.round() as u32).max(1));
    let cb = ViewportCallback { sim: sim.clone(), steps, time, grid_aspect, strokes, scene_pixels, restore, layer_b };
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    (rect, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_viewport_pillarboxes_square_grid() {
        let (x, y, w, h) = letterbox(400.0, 200.0, 1.0);
        assert_eq!((x, y, w, h), (100.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn tall_viewport_letterboxes_wide_grid() {
        let (x, y, w, h) = letterbox(200.0, 400.0, 2.0);
        assert_eq!((x, y, w, h), (0.0, 150.0, 200.0, 100.0));
    }
}

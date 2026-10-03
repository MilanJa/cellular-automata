//! GPU simulation: two ping-pong `rgba32float` textures, a compute pipeline built from the
//! user's rule shader and a render pipeline built from the user's render shader. The fixed
//! GPU objects every simulation on a device shares live in [`GpuContext`].

use std::sync::{Arc, Mutex};

use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::preset::{InitPattern, Mode};
use crate::shader::params::MAX_PARAMS;
use crate::shader::validate::{ShaderError, ShaderFile, Validated};
use crate::sim::export::{ExportedImage, clamp_scale, padded_bytes_per_row, to_rgba, unpad_rows};
use crate::sim::gpu::{GpuContext, SCENE_FORMAT, fullscreen_pipeline};
use crate::sim::history::{
    DEFAULT_BUDGET_BYTES, DEFAULT_INTERVAL, MAX_SNAPSHOTS, SnapshotMeta, SnapshotRing, should_snapshot,
    snapshot_capacity,
};
use crate::sim::init::generate_init;
use crate::sim::paint::{PaintUniform, Stroke, brush_bbox};
use crate::sim::row::{RowPlan, WORKGROUP, clamp_size, plan_row};
use crate::sim::stats::StatsSample;
use crate::sim::uniforms::{Globals, ParamsData};
use crate::util::lock;

#[derive(Clone, Debug, PartialEq)]
pub struct SimConfig {
    pub mode: Mode,
    pub width: u32,
    pub height: u32,
    pub init: InitPattern,
    pub seed: u32,
}

struct Textures {
    tex: [wgpu::Texture; 2],
    /// `[i]` reads `tex[i]`, writes `tex[1 - i]`.
    compute_bind_groups: [wgpu::BindGroup; 2],
    /// `[i]` reads `tex[i]`.
    render_bind_groups: [wgpu::BindGroup; 2],
    /// `[i]` writes `tex[i]` (mouse painting into the current state).
    paint_bind_groups: [wgpu::BindGroup; 2],
    /// `[i]` reads `tex[i]` as current and `tex[1 - i]` as previous (statistics).
    stats_bind_groups: [wgpu::BindGroup; 2],
}

/// Rendered picture (`scene`) and the ping-pong post-processing outputs at viewport resolution.
struct SceneTargets {
    width: u32,
    height: u32,
    scene_view: wgpu::TextureView,
    post_views: [wgpu::TextureView; 2],
    /// `[k]` writes `post[k]` reading `scene` and `post[1 - k]` as the previous frame.
    post_bind_groups: [wgpu::BindGroup; 2],
    /// `[k]` blits `post[k]` to the window.
    blit_bind_groups: [wgpu::BindGroup; 2],
    /// Index of the post output holding the latest frame.
    cur: usize,
}

/// The user-shader pipelines; always swapped in together so they agree on `Params`.
struct Pipelines {
    compute: wgpu::ComputePipeline,
    render: wgpu::RenderPipeline,
    post: wgpu::RenderPipeline,
    /// The initial-state shader, when the preset has one; run by `reset` for a `Code` init.
    seed: Option<wgpu::ComputePipeline>,
}

/// Browser only: pipelines built but not yet confirmed by the backend's asynchronous error
/// scopes. They are installed once the scopes resolve clean, or discarded with the errors.
#[cfg(target_arch = "wasm32")]
struct PendingPipelines {
    pipelines: Pipelines,
    result: Arc<Mutex<Option<Vec<ShaderError>>>>,
}

/// One slot of the statistics readback pool.
struct StatsSlot {
    buffer: wgpu::Buffer,
    state: StatsSlotState,
    step: u32,
    mapped: Arc<Mutex<Option<bool>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatsSlotState {
    Free,
    /// Copy recorded this frame; map once the frame has been submitted.
    Recorded,
    Mapping,
}

/// Offscreen render targets and the readback buffer for image export, kept between exports of
/// the same size so a recording does not allocate per frame.
struct ExportTargets {
    width: u32,
    height: u32,
    scene_view: wgpu::TextureView,
    post_view: wgpu::TextureView,
    final_tex: wgpu::Texture,
    buffer: wgpu::Buffer,
    padded_bpr: u32,
}

/// An offscreen render that has been submitted and whose readback buffer is being mapped.
struct PendingExport {
    width: u32,
    height: u32,
    padded_bpr: u32,
    filename: String,
    /// `Some(ok)` once the map callback ran.
    mapped: Arc<Mutex<Option<bool>>>,
}

pub struct Simulation {
    ctx: Arc<GpuContext>,
    config: SimConfig,
    paint_buf: wgpu::Buffer,
    stats_buf: wgpu::Buffer,
    stats_slots: Vec<StatsSlot>,
    globals_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    textures: Textures,
    /// Index of the texture holding the current state.
    cur: usize,
    pipelines: Option<Pipelines>,
    #[cfg(target_arch = "wasm32")]
    pending: Option<PendingPipelines>,
    /// Set by `reset` for a `Code` init until the seed shader has been dispatched; it may have
    /// to wait for `set_pipelines` (and, in the browser, for the backend to accept them).
    needs_seed: bool,
    /// Viewport-resolution scene and post targets; created on first frame, resized on demand.
    scene: Option<SceneTargets>,
    /// What `other()` reads: layer B's mirror, or the shared 1x1 zero texture.
    other_view: wgpu::TextureView,
    /// Rewind snapshots: one grid-sized texture per slot, allocated the first time the slot is
    /// written, plus the ring that orders them.
    history_tex: Vec<Option<wgpu::Texture>>,
    history: SnapshotRing,
    history_enabled: bool,
    snapshot_interval: u32,
    globals: Globals,
    export: Option<PendingExport>,
    export_targets: Option<ExportTargets>,
}

impl Simulation {
    pub fn new(ctx: Arc<GpuContext>, mut config: SimConfig) -> Self {
        let device = &ctx.device;
        let limits = device.limits();
        config.width = clamp_size(config.width, &limits);
        config.height = clamp_size(config.height, &limits);

        let paint_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ca paint uniform"),
            size: std::mem::size_of::<PaintUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stats_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ca stats"),
            size: 8,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stats_slots = (0..3)
            .map(|i| StatsSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("ca stats readback {i}")),
                    size: 8,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: StatsSlotState::Free,
                step: 0,
                mapped: Default::default(),
            })
            .collect();
        let globals = Globals {
            size: [config.width, config.height],
            mode: mode_code(config.mode),
            seed: config.seed,
            ..Default::default()
        };
        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        });
        let params: ParamsData = [[0; 4]; MAX_PARAMS];
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let textures = create_textures(
            &ctx,
            &globals_buf,
            &params_buf,
            &paint_buf,
            &stats_buf,
            &ctx.empty_other_view,
            config.width,
            config.height,
        );
        let history_capacity = snapshot_capacity(config.width, config.height, DEFAULT_BUDGET_BYTES, MAX_SNAPSHOTS);
        let other_view = ctx.empty_other_view.clone();
        let mut sim = Simulation {
            ctx,
            config,
            paint_buf,
            stats_buf,
            stats_slots,
            globals_buf,
            params_buf,
            textures,
            cur: 0,
            pipelines: None,
            #[cfg(target_arch = "wasm32")]
            pending: None,
            needs_seed: false,
            scene: None,
            other_view,
            history_tex: (0..history_capacity).map(|_| None).collect(),
            history: SnapshotRing::new(history_capacity),
            history_enabled: true,
            snapshot_interval: DEFAULT_INTERVAL,
            globals,
            export: None,
            export_targets: None,
        };
        sim.reset();
        sim
    }

    pub fn context(&self) -> &Arc<GpuContext> {
        &self.ctx
    }

    pub fn export_pending(&self) -> bool {
        self.export.is_some()
    }

    fn create_export_targets(&self, width: u32, height: u32) -> ExportTargets {
        let make = |label: &str, format: wgpu::TextureFormat, extra: wgpu::TextureUsages| {
            self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | extra,
                view_formats: &[],
            })
        };
        let scene_tex = make("ca export scene", SCENE_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING);
        let post_tex = make("ca export post", SCENE_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING);
        let final_tex = make("ca export final", self.ctx.target_format, wgpu::TextureUsages::COPY_SRC);
        let padded_bpr = padded_bytes_per_row(width * 4);
        let buffer = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ca export readback"),
            size: (padded_bpr * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        ExportTargets {
            width,
            height,
            scene_view: scene_tex.create_view(&Default::default()),
            post_view: post_tex.create_view(&Default::default()),
            final_tex,
            buffer,
            padded_bpr,
        }
    }

    /// Renders the current state at `scale` pixels per cell through the render and post shaders
    /// into an offscreen texture and starts reading it back. The result arrives through
    /// `poll_export` on a later frame.
    pub fn start_export(&mut self, scale: u32, filename: String) -> Result<(), String> {
        if self.export.is_some() {
            return Err("an image export is already in progress".into());
        }
        let Some(pipelines) = &self.pipelines else {
            return Err("no render pipeline: fix the shaders first".into());
        };
        let max_dim = self.ctx.device.limits().max_texture_dimension_2d;
        let s = clamp_scale(scale, self.config.width, self.config.height, max_dim);
        let (width, height) = (self.config.width * s, self.config.height * s);
        let targets = match self.export_targets.take() {
            Some(t) if t.width == width && t.height == height => t,
            _ => self.create_export_targets(width, height),
        };
        let final_view = targets.final_tex.create_view(&Default::default());
        // Feedback for the export reads the live post output when there is one, else the scene.
        let prev_view = match &self.scene {
            Some(sc) => sc.post_views[sc.cur].clone(),
            None => targets.scene_view.clone(),
        };
        let post_bg = self.post_bind_group(&targets.scene_view, &prev_view);
        let blit_bg = self.blit_bind_group(&targets.post_view);
        let mut encoder =
            self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ca export") });
        {
            let mut pass = begin_pass(&mut encoder, "ca export render", &targets.scene_view);
            pass.set_pipeline(&pipelines.render);
            pass.set_bind_group(0, &self.textures.render_bind_groups[self.cur], &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(&mut encoder, "ca export post", &targets.post_view);
            pass.set_pipeline(&pipelines.post);
            pass.set_bind_group(0, &post_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = begin_pass(&mut encoder, "ca export blit", &final_view);
            pass.set_pipeline(&self.ctx.blit_pipeline);
            pass.set_bind_group(0, &blit_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.final_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_bpr),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.ctx.queue.submit([encoder.finish()]);
        let mapped = Arc::new(Mutex::new(None));
        let flag = mapped.clone();
        targets.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            *lock(&flag) = Some(r.is_ok());
        });
        self.export = Some(PendingExport { width, height, padded_bpr: targets.padded_bpr, filename, mapped });
        self.export_targets = Some(targets);
        Ok(())
    }

    /// Call once per frame while an export is pending. Returns the PNG when the readback is done.
    pub fn poll_export(&mut self) -> Option<Result<ExportedImage, String>> {
        let state = lock(&self.export.as_ref()?.mapped).take();
        let Some(ok) = state else {
            // Give the mapping a chance to complete (a no-op in the browser, where it resolves
            // from the event loop instead).
            let _ = self.ctx.device.poll(wgpu::PollType::Poll);
            return None;
        };
        let pending = self.export.take()?;
        if !ok {
            return Some(Err("GPU readback failed".into()));
        }
        let Some(targets) = &self.export_targets else {
            return Some(Err("export targets vanished".into()));
        };
        let result = (|| -> anyhow::Result<ExportedImage> {
            let view = targets.buffer.slice(..).get_mapped_range()?;
            let mut rgba = unpad_rows(
                &view[..],
                pending.padded_bpr as usize,
                (pending.width * 4) as usize,
                pending.height as usize,
            );
            drop(view);
            targets.buffer.unmap();
            to_rgba(&mut rgba, self.ctx.target_format);
            Ok(ExportedImage { filename: pending.filename.clone(), width: pending.width, height: pending.height, rgba })
        })();
        Some(result.map_err(|e| format!("image export failed: {e:#}")))
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn frame(&self) -> u32 {
        self.globals.frame
    }

    pub fn has_pipelines(&self) -> bool {
        self.pipelines.is_some()
    }

    /// The rendered picture (before post-processing), once a scene size has been set. The view
    /// stays valid until the scene is resized, so it can be registered as an egui texture.
    pub fn scene_view(&self) -> Option<&wgpu::TextureView> {
        self.scene.as_ref().map(|s| &s.scene_view)
    }

    /// Makes sure the scene and post targets match the viewport's pixel size.
    pub fn ensure_scene_size(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if self.scene.as_ref().is_some_and(|s| s.width == width && s.height == height) {
            return;
        }
        self.scene = Some(self.create_scene_targets(width, height));
    }

    fn create_scene_targets(&self, width: u32, height: u32) -> SceneTargets {
        let make = |label: &str, extra: wgpu::TextureUsages| {
            self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra,
                view_formats: &[],
            })
        };
        let scene = make("ca scene", wgpu::TextureUsages::empty());
        let post = [make("ca post A", wgpu::TextureUsages::COPY_SRC), make("ca post B", wgpu::TextureUsages::COPY_SRC)];
        let scene_view = scene.create_view(&Default::default());
        let post_views = [post[0].create_view(&Default::default()), post[1].create_view(&Default::default())];
        let post_bg = |k: usize| self.post_bind_group(&scene_view, &post_views[1 - k]);
        let blit_bg = |k: usize| self.blit_bind_group(&post_views[k]);
        SceneTargets {
            width,
            height,
            post_bind_groups: [post_bg(0), post_bg(1)],
            blit_bind_groups: [blit_bg(0), blit_bg(1)],
            scene_view,
            post_views,
            cur: 0,
        }
    }

    fn post_bind_group(&self, scene: &wgpu::TextureView, prev: &wgpu::TextureView) -> wgpu::BindGroup {
        self.ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca post bg"),
            layout: &self.ctx.post_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scene) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.ctx.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(prev) },
            ],
        })
    }

    fn blit_bind_group(&self, src: &wgpu::TextureView) -> wgpu::BindGroup {
        self.ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca blit bg"),
            layout: &self.ctx.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.ctx.sampler) },
            ],
        })
    }

    /// Draws the state with the render shader into the scene texture, then runs the post shader
    /// into the next post output. Call once per frame after stepping.
    pub fn render_scene(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let (Some(pipelines), Some(scene)) = (&self.pipelines, &mut self.scene) else {
            return;
        };
        {
            let mut pass = begin_pass(encoder, "ca render scene", &scene.scene_view);
            pass.set_pipeline(&pipelines.render);
            pass.set_bind_group(0, &self.textures.render_bind_groups[self.cur], &[]);
            pass.draw(0..3, 0..1);
        }
        let dst = 1 - scene.cur;
        {
            let mut pass = begin_pass(encoder, "ca post", &scene.post_views[dst]);
            pass.set_pipeline(&pipelines.post);
            pass.set_bind_group(0, &scene.post_bind_groups[dst], &[]);
            pass.draw(0..3, 0..1);
        }
        scene.cur = dst;
    }

    /// Applies a new configuration (clamped to device limits), recreating textures if the size
    /// changed, and resets.
    pub fn reconfigure(&mut self, mut config: SimConfig) {
        let limits = self.ctx.device.limits();
        config.width = clamp_size(config.width, &limits);
        config.height = clamp_size(config.height, &limits);
        if config.width != self.config.width || config.height != self.config.height {
            let cap = snapshot_capacity(config.width, config.height, DEFAULT_BUDGET_BYTES, MAX_SNAPSHOTS);
            self.history_tex = (0..cap).map(|_| None).collect();
            self.history = SnapshotRing::new(cap);
            self.textures = create_textures(
                &self.ctx,
                &self.globals_buf,
                &self.params_buf,
                &self.paint_buf,
                &self.stats_buf,
                &self.other_view,
                config.width,
                config.height,
            );
        }
        self.config = config;
        self.reset();
    }

    // ---- layers ----

    /// A grid-sized texture another layer can mirror its state into for this layer to read.
    pub fn create_mirror_texture(&self) -> wgpu::Texture {
        self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ca layer mirror"),
            size: wgpu::Extent3d { width: self.config.width, height: self.config.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Copies the current state into `dst` (a mirror texture of the same size).
    pub fn mirror_into(&self, encoder: &mut wgpu::CommandEncoder, dst: &wgpu::Texture) {
        copy_whole(encoder, &self.textures.tex[self.cur], dst, self.config.width, self.config.height);
    }

    /// Makes `other()` read `mirror` (or nothing). Rebuilds the compute and render bind groups.
    pub fn set_other(&mut self, mirror: Option<&wgpu::Texture>) {
        self.other_view = match mirror {
            Some(t) => t.create_view(&Default::default()),
            None => self.ctx.empty_other_view.clone(),
        };
        self.globals.has_other = mirror.is_some() as u32;
        self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
        let views = [
            self.textures.tex[0].create_view(&Default::default()),
            self.textures.tex[1].create_view(&Default::default()),
        ];
        let (compute, render) =
            state_bind_groups(&self.ctx, &views, &self.globals_buf, &self.params_buf, &self.other_view);
        self.textures.compute_bind_groups = compute;
        self.textures.render_bind_groups = render;
    }

    // ---- rewind ----

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn history_capacity(&self) -> usize {
        self.history.capacity()
    }

    pub fn history_meta(&self, index: usize) -> Option<SnapshotMeta> {
        self.history.get(index).map(|(_, m)| m)
    }

    pub fn snapshot_interval(&self) -> u32 {
        self.snapshot_interval
    }

    pub fn set_snapshot_interval(&mut self, interval: u32) {
        self.snapshot_interval = interval.max(1);
    }

    /// Turns rewind off for good: no snapshots are taken and no history texture is ever
    /// allocated. For secondary simulations (layer B, explorer thumbnails).
    pub fn disable_history(&mut self) {
        self.history_enabled = false;
        self.history.clear();
        self.history_tex.iter_mut().for_each(|t| *t = None);
    }

    /// Copies the current state into the next history slot.
    pub fn snapshot_now(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.history_enabled {
            return;
        }
        let meta = SnapshotMeta { step: self.globals.frame, row: self.globals.row };
        let slot = self.history.push(meta);
        let (w, h) = (self.config.width, self.config.height);
        let dst = history_texture(&self.ctx.device, &mut self.history_tex, slot, w, h);
        copy_whole(encoder, &self.textures.tex[self.cur], dst, w, h);
    }

    /// Makes snapshot `index` (oldest first) the current state. The step counter and 1D row
    /// are restored too, so stepping continues from that moment.
    pub fn restore_snapshot(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> Option<SnapshotMeta> {
        let (slot, meta) = self.history.get(index)?;
        let src = self.history_tex[slot].as_ref()?;
        for tex in &self.textures.tex {
            copy_whole(encoder, src, tex, self.config.width, self.config.height);
        }
        self.cur = 0;
        self.globals.frame = meta.step;
        self.globals.row = meta.row;
        self.globals.prev_row = meta.row.saturating_sub(1);
        self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
        Some(meta)
    }

    /// Restarts from the init pattern. A `Code` init is filled in by the seed shader: right
    /// away when the pipelines are in place, otherwise as soon as they are.
    pub fn reset(&mut self) {
        let c = &self.config;
        let data = generate_init(&c.init, c.mode, c.width, c.height, c.seed);
        self.cur = 0;
        // Both textures get the init state: in 1D mode a step only copies the rows above the
        // write head, so stale rows in the other texture would otherwise show through.
        for tex in &self.textures.tex {
            self.ctx.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(c.width * 16),
                    rows_per_image: Some(c.height),
                },
                wgpu::Extent3d { width: c.width, height: c.height, depth_or_array_layers: 1 },
            );
        }
        self.history.clear();
        self.globals = Globals {
            size: [c.width, c.height],
            frame: 0,
            seed: c.seed,
            time: 0.0,
            mode: mode_code(c.mode),
            row: 1,
            prev_row: 0,
            blend: self.globals.blend,
            has_other: self.globals.has_other,
            _pad: [0; 2],
        };
        self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
        // The seed shader runs at the start of the next `step`, not here: by then the pipelines
        // are in place and the slider values it may read have been uploaded.
        self.needs_seed = c.init == InitPattern::Code;
    }

    /// Records the seed shader over the blank grid left by `reset`: read the (blank) other
    /// texture, write the current one, then mirror it so both textures hold the start state.
    fn record_seed(&self, encoder: &mut wgpu::CommandEncoder, seed: &wgpu::ComputePipeline) {
        let (w, h) = (self.config.width, self.config.height);
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ca seed"), timestamp_writes: None });
            pass.set_pipeline(seed);
            pass.set_bind_group(0, &self.textures.compute_bind_groups[1 - self.cur], &[]);
            match self.config.mode {
                Mode::TwoD => pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1),
                Mode::OneD => pass.dispatch_workgroups(w.div_ceil(WORKGROUP), 1, 1),
            }
        }
        copy_whole(encoder, &self.textures.tex[self.cur], &self.textures.tex[1 - self.cur], w, h);
    }

    /// Replaces the whole state with `data` (`width * height * 4` floats) without touching the
    /// step counter. In 1D mode the diagram is considered full, so stepping scrolls from the
    /// bottom row.
    pub fn load_state(&mut self, data: &[f32]) -> Result<(), String> {
        let c = &self.config;
        let expected = (c.width * c.height * 4) as usize;
        if data.len() != expected {
            return Err(format!("state has {} values, expected {expected}", data.len()));
        }
        self.cur = 0;
        for tex in &self.textures.tex {
            self.ctx.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(c.width * 16),
                    rows_per_image: Some(c.height),
                },
                wgpu::Extent3d { width: c.width, height: c.height, depth_or_array_layers: 1 },
            );
        }
        if c.mode == Mode::OneD {
            self.globals.row = c.height;
            self.globals.prev_row = c.height.saturating_sub(1);
        }
        self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
        Ok(())
    }

    pub fn set_params(&mut self, data: ParamsData) {
        self.ctx.queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&data));
    }

    pub fn set_time(&mut self, seconds: f32) {
        self.globals.time = seconds;
    }

    /// Crossfade between rule A and rule B (only used by a rule pair).
    pub fn set_blend(&mut self, blend: f32) {
        self.globals.blend = blend.clamp(0.0, 1.0);
    }

    fn create_module(&self, shader: &Validated) -> wgpu::ShaderModule {
        self.ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(shader.file().label()),
            source: wgpu::ShaderSource::Wgsl(shader.source().into()),
        })
    }

    fn build_compute(&self, shader: &Validated) -> wgpu::ComputePipeline {
        let module = self.create_module(shader);
        self.ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(shader.file().label()),
            layout: Some(&self.ctx.compute_pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
    }

    fn build_fullscreen(&self, shader: &Validated, layout: &wgpu::PipelineLayout) -> wgpu::RenderPipeline {
        let module = self.create_module(shader);
        fullscreen_pipeline(&self.ctx.device, shader.file().label(), layout, &module, SCENE_FORMAT)
    }

    /// Builds the compute, render, post and (optional) seed pipelines from naga-validated
    /// sources and swaps them in together, so the live pipelines always agree on the `Params`
    /// layout. Each stage is built inside its own validation error scope so a backend rejection
    /// is attributed to the right file. On the desktop the scopes resolve synchronously and an
    /// error means nothing changes. In the browser they resolve later: the new pipelines wait in
    /// `pending` and are installed by `poll_pipeline_check` once the backend accepted them.
    pub fn set_pipelines(
        &mut self,
        rule: &Validated,
        render: &Validated,
        post: &Validated,
        seed: Option<&Validated>,
    ) -> Result<(), Vec<ShaderError>> {
        let device = &self.ctx.device;
        let s_rule = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let compute = self.build_compute(rule);
        let s_render = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let render = self.build_fullscreen(render, &self.ctx.render_pipeline_layout);
        let s_post = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let post = self.build_fullscreen(post, &self.ctx.post_pipeline_layout);
        let s_seed = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let seed = seed.map(|s| self.build_compute(s));
        // Scopes are a stack: pop in reverse order. Calling `pop` now (even on the web, where the
        // result arrives later) keeps the device's scope stack balanced.
        let f_seed = s_seed.pop();
        let f_post = s_post.pop();
        let f_render = s_render.pop();
        let f_rule = s_rule.pop();
        let pipelines = Pipelines { compute, render, post, seed };
        self.finish_pipelines(pipelines, [f_rule, f_render, f_post, f_seed])
    }

    /// The error-scope results of `set_pipelines`, in the order of `SCOPE_FILES`.
    const SCOPE_FILES: [ShaderFile; 4] = [ShaderFile::Rule, ShaderFile::Render, ShaderFile::Post, ShaderFile::Seed];

    #[cfg(not(target_arch = "wasm32"))]
    fn finish_pipelines(
        &mut self,
        pipelines: Pipelines,
        scopes: [impl Future<Output = Option<wgpu::Error>>; 4],
    ) -> Result<(), Vec<ShaderError>> {
        let results = scopes.map(pollster::block_on);
        let errors = backend_errors(Self::SCOPE_FILES.into_iter().zip(results));
        if errors.is_empty() {
            self.pipelines = Some(pipelines);
            Ok(())
        } else {
            Err(errors)
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn finish_pipelines(
        &mut self,
        pipelines: Pipelines,
        scopes: [impl Future<Output = Option<wgpu::Error>> + 'static; 4],
    ) -> Result<(), Vec<ShaderError>> {
        let result: Arc<Mutex<Option<Vec<ShaderError>>>> = Default::default();
        let slot = result.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut results = Vec::with_capacity(scopes.len());
            // Scopes resolve in stack order: the innermost (last pushed) first.
            for scope in scopes.into_iter().rev() {
                results.push(scope.await);
            }
            results.reverse();
            *lock(&slot) = Some(backend_errors(Self::SCOPE_FILES.into_iter().zip(results)));
        });
        self.pending = Some(PendingPipelines { pipelines, result });
        Ok(())
    }

    /// Call once per frame. Resolves a browser-side backend check: installs the pending
    /// pipelines when the backend accepted them, or returns the errors (the previous pipelines
    /// keep running). Always `None` on the desktop, where `set_pipelines` is synchronous.
    pub fn poll_pipeline_check(&mut self) -> Option<Vec<ShaderError>> {
        #[cfg(target_arch = "wasm32")]
        {
            let errors = lock(&self.pending.as_ref()?.result).take()?;
            let pending = self.pending.take()?;
            if errors.is_empty() {
                self.pipelines = Some(pending.pipelines);
                None
            } else {
                Some(errors)
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        None
    }

    /// Records brush strokes into the *current* state texture. In 1D mode a stroke paints its
    /// x-range onto the most recently written row, which is what the next generation reads.
    pub fn paint(&mut self, encoder: &mut wgpu::CommandEncoder, strokes: &[Stroke]) {
        let (w, h) = (self.config.width, self.config.height);
        for stroke in strokes {
            let (sy, bbox) = match self.config.mode {
                Mode::TwoD => (stroke.y, brush_bbox(stroke.x, stroke.y, stroke.radius, w, h)),
                Mode::OneD => {
                    let row = self.globals.row.saturating_sub(1).min(h.saturating_sub(1));
                    let bb =
                        brush_bbox(stroke.x, row as i32, stroke.radius, w, h).map(|(x0, _, bw, _)| (x0, row, bw, 1));
                    (row as i32, bb)
                }
            };
            let Some((x0, y0, bw, bh)) = bbox else { continue };
            let uniform = PaintUniform {
                origin: [x0 as i32, y0 as i32],
                center: [stroke.x as f32, sy as f32],
                radius: stroke.radius.max(0.5),
                _pad: [0.0; 3],
                value: stroke.value,
            };
            let staging = self.ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ca paint staging"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::COPY_SRC,
            });
            encoder.copy_buffer_to_buffer(&staging, 0, &self.paint_buf, 0, std::mem::size_of::<PaintUniform>() as u64);
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ca paint"), timestamp_writes: None });
            pass.set_pipeline(&self.ctx.paint_pipeline);
            pass.set_bind_group(0, &self.textures.paint_bind_groups[self.cur], &[]);
            pass.dispatch_workgroups(bw.div_ceil(WORKGROUP), bh.div_ceil(WORKGROUP), 1);
        }
    }

    /// Records the statistics reduction for the current state into `encoder` and queues the
    /// result for `poll_stats`. Skipped when every readback slot is busy.
    pub fn collect_stats(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(slot) = self.stats_slots.iter_mut().find(|s| s.state == StatsSlotState::Free) else {
            return;
        };
        encoder.clear_buffer(&self.stats_buf, 0, None);
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ca stats"), timestamp_writes: None });
            pass.set_pipeline(&self.ctx.stats_pipeline);
            pass.set_bind_group(0, &self.textures.stats_bind_groups[self.cur], &[]);
            pass.dispatch_workgroups(self.config.width.div_ceil(WORKGROUP), self.config.height.div_ceil(WORKGROUP), 1);
        }
        encoder.copy_buffer_to_buffer(&self.stats_buf, 0, &slot.buffer, 0, 8);
        slot.state = StatsSlotState::Recorded;
        slot.step = self.globals.frame;
    }

    /// Advances the readback pool; call once per frame. Returns finished samples, oldest first.
    pub fn poll_stats(&mut self) -> Vec<StatsSample> {
        let mut out = Vec::new();
        let _ = self.ctx.device.poll(wgpu::PollType::Poll);
        let mut mapped_any = false;
        for slot in &mut self.stats_slots {
            match slot.state {
                StatsSlotState::Free => {}
                StatsSlotState::Recorded => {
                    // The copy was submitted with the previous frame; the buffer can be mapped now.
                    let flag = slot.mapped.clone();
                    *lock(&flag) = None;
                    slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                        *lock(&flag) = Some(r.is_ok());
                    });
                    slot.state = StatsSlotState::Mapping;
                    mapped_any = true;
                }
                StatsSlotState::Mapping => {
                    let done = lock(&slot.mapped).take();
                    if let Some(ok) = done {
                        if ok && let Ok(view) = slot.buffer.slice(..).get_mapped_range() {
                            let words: &[u32] = bytemuck::cast_slice(&view[..]);
                            out.push(StatsSample { step: slot.step, population: words[0], changed: words[1] });
                        }
                        slot.buffer.unmap();
                        slot.state = StatsSlotState::Free;
                    }
                }
            }
        }
        if mapped_any {
            let _ = self.ctx.device.poll(wgpu::PollType::Poll);
        }
        out.sort_by_key(|s| s.step);
        out
    }

    /// Records `n` simulation steps into `encoder`. With `n == 0` only the globals (notably
    /// `time`) are refreshed, so time-based render shaders keep moving while paused.
    pub fn step(&mut self, encoder: &mut wgpu::CommandEncoder, n: u32) {
        let Some(pipelines) = &self.pipelines else {
            self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
            return;
        };
        if self.needs_seed {
            // A `Code` init owed by `reset`. In the browser, wait for the pipelines being
            // checked: they are the ones that should seed, and stepping a blank grid with the
            // old ones would only be undone.
            #[cfg(target_arch = "wasm32")]
            if self.pending.is_some() {
                self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
                return;
            }
            self.needs_seed = false;
            if let Some(seed) = &pipelines.seed {
                self.record_seed(encoder, seed);
            }
        }
        if n == 0 {
            self.ctx.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
            return;
        }
        let (w, h) = (self.config.width, self.config.height);

        // Pass 1: decide every step's globals and row plan, advancing the CPU-side state.
        let mut snapshots: Vec<Globals> = Vec::with_capacity(n as usize);
        let mut plans: Vec<Option<RowPlan>> = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let plan = match self.config.mode {
                Mode::TwoD => None,
                Mode::OneD => {
                    let plan = plan_row(self.globals.row, h);
                    self.globals.row = plan.write_row;
                    self.globals.prev_row = plan.read_row;
                    Some(plan)
                }
            };
            snapshots.push(self.globals);
            plans.push(plan);
            if let Some(plan) = plan {
                self.globals.row = plan.next_row;
            }
            self.globals.frame = self.globals.frame.wrapping_add(1);
        }

        // One staging buffer holds every step's globals; each step copies its slice in.
        let staging = self.ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals staging"),
            contents: bytemuck::cast_slice(&snapshots),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let size = std::mem::size_of::<Globals>() as u64;

        // Pass 2: record copies and dispatches.
        let mut cur = self.cur;
        for (i, plan) in plans.iter().enumerate() {
            let (src, dst) = (cur, 1 - cur);
            if let Some(plan) = plan {
                // Carry the unchanged rows from src to dst so dst holds the full diagram.
                if plan.scroll {
                    copy_rows(encoder, &self.textures.tex[src], 1, &self.textures.tex[dst], 0, w, h - 1);
                } else {
                    copy_rows(encoder, &self.textures.tex[src], 0, &self.textures.tex[dst], 0, w, plan.write_row);
                }
            }
            encoder.copy_buffer_to_buffer(&staging, i as u64 * size, &self.globals_buf, 0, size);
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ca step"), timestamp_writes: None });
            pass.set_pipeline(&pipelines.compute);
            pass.set_bind_group(0, &self.textures.compute_bind_groups[src], &[]);
            match plan {
                None => pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1),
                Some(_) => pass.dispatch_workgroups(w.div_ceil(WORKGROUP), 1, 1),
            }
            drop(pass);
            cur = dst;
            // Rewind snapshots: the state just written, every `snapshot_interval` steps.
            let step_done = snapshots[i].frame.wrapping_add(1);
            if self.history_enabled && should_snapshot(step_done, self.snapshot_interval) {
                let row_after = plans[i].map_or(snapshots[i].row, |p| p.next_row);
                let slot = self.history.push(SnapshotMeta { step: step_done, row: row_after });
                let dst_tex = history_texture(&self.ctx.device, &mut self.history_tex, slot, w, h);
                copy_whole(encoder, &self.textures.tex[cur], dst_tex, w, h);
            }
        }
        self.cur = cur;
    }

    /// Copies the latest post output into the active render pass (the viewport).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some(scene) = &self.scene else { return };
        if self.pipelines.is_none() {
            return;
        }
        pass.set_pipeline(&self.ctx.blit_pipeline);
        pass.set_bind_group(0, &scene.blit_bind_groups[scene.cur], &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Turns per-stage error-scope results into located diagnostics (line 1 of the file: the
/// backend does not report user lines).
fn backend_errors(results: impl IntoIterator<Item = (ShaderFile, Option<wgpu::Error>)>) -> Vec<ShaderError> {
    results
        .into_iter()
        .filter_map(|(file, err)| {
            err.map(|e| ShaderError {
                file,
                line: 1,
                column: 1,
                message: format!("GPU backend rejected shader: {e}"),
                hint: None,
            })
        })
        .collect()
}

fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn mode_code(mode: Mode) -> u32 {
    match mode {
        Mode::TwoD => 0,
        Mode::OneD => 1,
    }
}

fn copy_whole(encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture, width: u32, height: u32) {
    copy_rows(encoder, src, 0, dst, 0, width, height);
}

/// The history texture for `slot`, created on first use.
fn history_texture<'a>(
    device: &wgpu::Device,
    slots: &'a mut [Option<wgpu::Texture>],
    slot: usize,
    width: u32,
    height: u32,
) -> &'a wgpu::Texture {
    slots[slot].get_or_insert_with(|| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("ca history {slot}")),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    })
}

fn copy_rows(
    encoder: &mut wgpu::CommandEncoder,
    src: &wgpu::Texture,
    src_y: u32,
    dst: &wgpu::Texture,
    dst_y: u32,
    width: u32,
    rows: u32,
) {
    if rows == 0 {
        return;
    }
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: src,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: src_y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: dst,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: dst_y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d { width, height: rows, depth_or_array_layers: 1 },
    );
}

/// Compute (`[i]` reads tex i, writes 1-i) and render (`[i]` reads tex i) bind groups.
fn state_bind_groups(
    ctx: &GpuContext,
    views: &[wgpu::TextureView; 2],
    globals_buf: &wgpu::Buffer,
    params_buf: &wgpu::Buffer,
    other_view: &wgpu::TextureView,
) -> ([wgpu::BindGroup; 2], [wgpu::BindGroup; 2]) {
    let compute_bg = |src: usize| {
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca compute bg"),
            layout: &ctx.compute_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[src]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1 - src]) },
                wgpu::BindGroupEntry { binding: 2, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(other_view) },
            ],
        })
    };
    let render_bg = |src: usize| {
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca render bg"),
            layout: &ctx.render_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[src]) },
                wgpu::BindGroupEntry { binding: 2, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(other_view) },
            ],
        })
    };
    ([compute_bg(0), compute_bg(1)], [render_bg(0), render_bg(1)])
}

#[allow(clippy::too_many_arguments)]
fn create_textures(
    ctx: &GpuContext,
    globals_buf: &wgpu::Buffer,
    params_buf: &wgpu::Buffer,
    paint_buf: &wgpu::Buffer,
    stats_buf: &wgpu::Buffer,
    other_view: &wgpu::TextureView,
    width: u32,
    height: u32,
) -> Textures {
    let make = |label: &str| {
        ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let tex = [make("ca state A"), make("ca state B")];
    let views = [tex[0].create_view(&Default::default()), tex[1].create_view(&Default::default())];
    let (compute_bind_groups, render_bind_groups) = state_bind_groups(ctx, &views, globals_buf, params_buf, other_view);
    let paint_bg = |dst: usize| {
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca paint bg"),
            layout: &ctx.paint_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[dst]) },
                wgpu::BindGroupEntry { binding: 1, resource: paint_buf.as_entire_binding() },
            ],
        })
    };
    let stats_bg = |cur: usize| {
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca stats bg"),
            layout: &ctx.stats_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[cur]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1 - cur]) },
                wgpu::BindGroupEntry { binding: 2, resource: stats_buf.as_entire_binding() },
            ],
        })
    };
    let paint_bind_groups = [paint_bg(0), paint_bg(1)];
    let stats_bind_groups = [stats_bg(0), stats_bg(1)];
    Textures { tex, compute_bind_groups, render_bind_groups, paint_bind_groups, stats_bind_groups }
}

/// GPU tests: need a real adapter, so they are `#[ignore]`d for CI. Run with
/// `cargo test gpu_tests -- --ignored`.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::preset::builtin::{BUILTINS, load_builtin};
    use crate::shader::assemble::{DEFAULT_POST, assemble_post, assemble_render, assemble_rule, assemble_seed};
    use crate::shader::params::{merge_params, pack_params, params_wgsl, parse_params};
    use crate::shader::validate::validate;

    /// Creating several devices concurrently from test threads hangs on some drivers, so every
    /// GPU test holds this lock for its whole duration.
    static GPU_LOCK: Mutex<()> = Mutex::new(());

    fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
        lock(&GPU_LOCK)
    }

    fn context() -> Option<Arc<GpuContext>> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Bgra8UnormSrgb))
    }

    fn sim_with(config: SimConfig) -> Option<Simulation> {
        Some(Simulation::new(context()?, config))
    }

    fn config(mode: Mode, w: u32, h: u32) -> SimConfig {
        SimConfig { mode, width: w, height: h, init: InitPattern::Single, seed: 1 }
    }

    fn validated_rule(src: &str, pw: &str) -> Validated {
        let a = assemble_rule(src, pw);
        validate(ShaderFile::Rule, &a).unwrap_or_else(|e| panic!("{e:?}\n{}", a.source))
    }

    fn assembled_for(rule: &str, render: &str) -> (Validated, Validated, Validated) {
        let specs = merge_params(parse_params(rule).unwrap(), parse_params(render).unwrap()).unwrap();
        let pw = params_wgsl(&specs);
        (
            validated_rule(rule, &pw),
            validate(ShaderFile::Render, &assemble_render(render, &pw)).unwrap(),
            validate(ShaderFile::Post, &assemble_post(DEFAULT_POST, &pw)).unwrap(),
        )
    }

    /// Uploads the shaders' default param values, as the app does after a successful apply.
    fn upload_default_params(sim: &mut Simulation, rule: &str, render: &str) {
        let specs = merge_params(parse_params(rule).unwrap(), parse_params(render).unwrap()).unwrap();
        sim.set_params(pack_params(&specs, &std::collections::BTreeMap::new()));
    }

    /// Runs `n` steps, submits, and returns any validation error the GPU backend reported.
    fn step_and_check(sim: &mut Simulation, n: u32) -> Option<wgpu::Error> {
        let scope = sim.ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        sim.step(&mut enc, n);
        sim.ctx.queue.submit([enc.finish()]);
        let _ = sim.ctx.device.poll(wgpu::PollType::wait_indefinitely());
        pollster::block_on(scope.pop())
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn every_builtin_compiles_on_the_real_backend_and_steps() {
        let _gpu = gpu_lock();
        for b in BUILTINS.iter() {
            let p = load_builtin(b);
            let (rule, render, post) = assembled_for(&p.rule, &p.render);
            let Some(mut sim) = sim_with(config(p.meta.mode, 64, 32)) else { return };
            sim.set_pipelines(&rule, &render, &post, None).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
            upload_default_params(&mut sim, &p.rule, &p.render);
            assert!(sim.has_pipelines());
            assert!(step_and_check(&mut sim, 40).is_none(), "{}: validation error while stepping", b.id);
            assert_eq!(sim.frame(), 40);
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn bad_shader_never_reaches_the_gpu_and_old_pipelines_stay() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::TwoD, 32, 32)) else { return };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        let pw = params_wgsl(&parse_params(&p.render).unwrap());
        let bad = assemble_rule("fn rule(pos: vec2<u32>) -> vec4<f32> { return bogus(; }", &pw);
        let errs = validate(ShaderFile::Rule, &bad).unwrap_err();
        assert_eq!((errs[0].file, errs[0].line), (ShaderFile::Rule, 1));
        let bad_render = assemble_render("fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return 1.0; }", &pw);
        let errs = validate(ShaderFile::Render, &bad_render).unwrap_err();
        assert_eq!(errs[0].file, ShaderFile::Render);
        assert!(sim.has_pipelines(), "a failed validation must not touch the live pipelines");
        assert!(step_and_check(&mut sim, 3).is_none());
    }

    /// Reads a whole `rgba32float` texture back to the CPU (width * 16 bytes must be a multiple of 256).
    fn read_back(sim: &Simulation, which: usize) -> Vec<f32> {
        let (w, h) = (sim.config.width, sim.config.height);
        let bytes = (w * h * 16) as u64;
        let buf = sim.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &sim.textures.tex[which],
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 16), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        sim.ctx.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        let _ = sim.ctx.device.poll(wgpu::PollType::wait_indefinitely());
        let view = slice.get_mapped_range().expect("buffer mapped");
        let data: Vec<f32> = bytemuck::cast_slice(&view[..]).to_vec();
        data
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn reset_leaves_no_stale_rows_in_the_second_texture() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[0]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::OneD, 16, 4)) else { return };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        upload_default_params(&mut sim, &p.rule, &p.render);
        assert!(step_and_check(&mut sim, 3).is_none());
        // Sanity: rule 30 from a single cell must have produced live cells in the other texture.
        assert!(read_back(&sim, 1)[16 * 4..].iter().step_by(4).any(|&r| r > 0.5), "steps produced nothing");
        sim.reconfigure(config(Mode::OneD, 16, 4)); // same size: textures are reused
        let a = read_back(&sim, 0);
        let b = read_back(&sim, 1);
        assert_eq!(a, b, "both textures must hold the init pattern after a reset");
        assert!(a[4 * 8] > 0.5, "row 0 centre cell is set");
        assert!(a[16 * 4..].chunks(4).all(|px| px[0] == 0.0), "rows below the head are blank");
    }

    /// Indices of the live cells (`.r > 0.5`) in a read-back texture.
    fn live_cells(data: &[f32]) -> Vec<usize> {
        data.chunks(4).enumerate().filter(|(_, p)| p[0] > 0.5).map(|(i, _)| i).collect()
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn a_code_init_runs_the_seed_shader_on_reset_into_both_textures() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let pw = params_wgsl(&parse_params(&p.render).unwrap());
        let seed = validate(
            ShaderFile::Seed,
            &assemble_seed("fn seed(pos: vec2<u32>) -> vec4<f32> { return on_if(pos.x == 3u && pos.y == 2u); }", &pw),
        )
        .unwrap();
        let cfg = SimConfig { mode: Mode::TwoD, width: 16, height: 8, init: InitPattern::Code, seed: 1 };
        let Some(mut sim) = sim_with(cfg.clone()) else { return };
        // No pipelines yet: the grid is blank and the seed is owed.
        assert!(sim.needs_seed);
        assert!(live_cells(&read_back(&sim, 0)).is_empty());
        sim.set_pipelines(&rule, &render, &post, Some(&seed)).unwrap();
        assert!(sim.needs_seed, "the seed waits for the next step, after the params upload");
        assert!(step_and_check(&mut sim, 0).is_none());
        assert!(!sim.needs_seed, "a zero-step frame pays the owed seed");
        assert_eq!(live_cells(&read_back(&sim, 0)), vec![2 * 16 + 3]);
        assert_eq!(live_cells(&read_back(&sim, 1)), vec![2 * 16 + 3], "the other texture mirrors the seed");
        // Stepping Life kills the lone cell; a reset brings it back (seeded in the same step).
        assert!(step_and_check(&mut sim, 1).is_none());
        assert!(live_cells(&read_back(&sim, sim.cur)).is_empty());
        sim.reconfigure(cfg);
        assert!(step_and_check(&mut sim, 0).is_none());
        assert_eq!(live_cells(&read_back(&sim, 0)), vec![2 * 16 + 3]);
        // Without a seed shader a code init stays blank, and nothing is left owed.
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        sim.reset();
        assert!(step_and_check(&mut sim, 0).is_none());
        assert!(!sim.needs_seed);
        assert!(live_cells(&read_back(&sim, 0)).is_empty());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_seed_shader_sees_the_slider_values_uploaded_after_the_pipelines() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let seed_src = "// @param radius: f32 = 0.0 range 0.0 .. 8.0\nfn seed(pos: vec2<u32>) -> vec4<f32> { return on_if(in_disc(centred(pos), vec2<i32>(0), params.radius)); }";
        let specs = merge_params(parse_params(&p.rule).unwrap(), parse_params(&p.render).unwrap())
            .and_then(|s| merge_params(s, parse_params(seed_src).unwrap()))
            .unwrap();
        let pw = params_wgsl(&specs);
        let rule = validated_rule(&p.rule, &pw);
        let render = validate(ShaderFile::Render, &assemble_render(&p.render, &pw)).unwrap();
        let post = validate(ShaderFile::Post, &assemble_post(DEFAULT_POST, &pw)).unwrap();
        let seed = validate(ShaderFile::Seed, &assemble_seed(seed_src, &pw)).unwrap();
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::TwoD, width: 16, height: 16, init: InitPattern::Code, seed: 1 })
        else {
            return;
        };
        // The app's order: pipelines first, slider values second, then the frame.
        sim.set_pipelines(&rule, &render, &post, Some(&seed)).unwrap();
        let mut values = std::collections::BTreeMap::new();
        values.insert("radius".to_string(), crate::shader::params::ParamValue::F32(3.0));
        sim.set_params(pack_params(&specs, &values));
        assert!(step_and_check(&mut sim, 0).is_none());
        let live = live_cells(&read_back(&sim, 0)).len();
        assert!((25..=32).contains(&live), "a radius-3 disc has about 29 cells, got {live}");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn in_one_d_mode_the_seed_shader_fills_the_first_row_only() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[0]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let pw = params_wgsl(&parse_params(&p.rule).unwrap());
        let seed =
            validate(ShaderFile::Seed, &assemble_seed("fn seed(pos: vec2<u32>) -> vec4<f32> { return on(); }", &pw))
                .unwrap();
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::OneD, width: 16, height: 4, init: InitPattern::Code, seed: 1 })
        else {
            return;
        };
        sim.set_pipelines(&rule, &render, &post, Some(&seed)).unwrap();
        assert!(step_and_check(&mut sim, 0).is_none());
        let live = live_cells(&read_back(&sim, 0));
        assert_eq!(live, (0..16).collect::<Vec<_>>(), "row 0 is all on, the rows below stay blank");
        assert_eq!(read_back(&sim, 0), read_back(&sim, 1));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn every_seeded_builtin_puts_live_cells_on_the_grid() {
        let _gpu = gpu_lock();
        for b in BUILTINS.iter().filter(|b| b.seed.is_some()) {
            let p = load_builtin(b);
            let seed_src = p.seed.clone().unwrap();
            let specs = merge_params(parse_params(&p.rule).unwrap(), parse_params(&p.render).unwrap())
                .and_then(|s| merge_params(s, parse_params(&seed_src).unwrap()))
                .unwrap();
            let pw = params_wgsl(&specs);
            let rule = validated_rule(&p.rule, &pw);
            let render = validate(ShaderFile::Render, &assemble_render(&p.render, &pw)).unwrap();
            let post = validate(ShaderFile::Post, &assemble_post(DEFAULT_POST, &pw)).unwrap();
            let seed = validate(ShaderFile::Seed, &assemble_seed(&seed_src, &pw)).unwrap();
            let cfg = SimConfig { mode: p.meta.mode, width: 256, height: 256, init: InitPattern::Code, seed: 1 };
            let Some(mut sim) = sim_with(cfg) else { return };
            sim.set_params(pack_params(&specs, &std::collections::BTreeMap::new()));
            sim.set_pipelines(&rule, &render, &post, Some(&seed)).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
            assert!(step_and_check(&mut sim, 0).is_none(), "{}: validation error while seeding", b.id);
            // Gray-Scott's seed writes U = 1 everywhere, so count the cells that differ from the bath.
            let data = read_back(&sim, 0);
            let distinct: std::collections::HashSet<[u32; 4]> =
                data.chunks(4).map(|px| [px[0].to_bits(), px[1].to_bits(), px[2].to_bits(), px[3].to_bits()]).collect();
            assert!(distinct.len() >= 2, "{}: the seed shader drew nothing", b.id);
            assert!(step_and_check(&mut sim, 10).is_none(), "{}: validation error while stepping", b.id);
        }
    }

    /// A seeded built-in, set up at its own grid size with its default sliders and seeded.
    fn seeded_builtin(builtin_id: &str) -> Option<Simulation> {
        let p = load_builtin(BUILTINS.iter().find(|b| b.id == builtin_id).unwrap());
        let seed_src = p.seed.clone().unwrap();
        let specs = merge_params(parse_params(&p.rule).unwrap(), parse_params(&p.render).unwrap())
            .and_then(|s| merge_params(s, parse_params(&seed_src).unwrap()))
            .unwrap();
        let pw = params_wgsl(&specs);
        let rule = validated_rule(&p.rule, &pw);
        let render = validate(ShaderFile::Render, &assemble_render(&p.render, &pw)).unwrap();
        let post = validate(ShaderFile::Post, &assemble_post(DEFAULT_POST, &pw)).unwrap();
        let seed = validate(ShaderFile::Seed, &assemble_seed(&seed_src, &pw)).unwrap();
        let cfg = SimConfig {
            mode: p.meta.mode,
            width: p.meta.width,
            height: p.meta.height,
            init: InitPattern::Code,
            seed: p.meta.seed,
        };
        let mut sim = sim_with(cfg)?;
        sim.set_pipelines(&rule, &render, &post, Some(&seed)).unwrap();
        sim.set_params(pack_params(&specs, &std::collections::BTreeMap::new()));
        assert!(step_and_check(&mut sim, 0).is_none(), "{}: validation error while seeding", builtin_id);
        Some(sim)
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_glider_gun_fires_one_glider_every_thirty_steps() {
        let _gpu = gpu_lock();
        let Some(mut sim) = seeded_builtin("glider_gun") else { return };
        let population = |sim: &Simulation| live_cells(&read_back(sim, sim.cur)).len();
        assert_eq!(population(&sim), 36, "Gosper's gun has 36 cells");
        for gliders in 1..=4 {
            assert!(step_and_check(&mut sim, 30).is_none());
            assert_eq!(population(&sim), 36 + 5 * gliders, "after {} steps", 30 * gliders);
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_acorn_grows() {
        let _gpu = gpu_lock();
        let Some(mut sim) = seeded_builtin("acorn") else { return };
        let population = |sim: &Simulation| live_cells(&read_back(sim, sim.cur)).len();
        assert_eq!(population(&sim), 7);
        assert!(step_and_check(&mut sim, 200).is_none());
        assert!(population(&sim) > 50, "a methuselah should be well under way after 200 steps");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn restless_life_keeps_changing_long_after_plain_life_would_have_settled() {
        let _gpu = gpu_lock();
        let p = load_builtin(BUILTINS.iter().find(|b| b.id == "restless_life").unwrap());
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let cfg = SimConfig { mode: Mode::TwoD, width: 128, height: 128, init: p.meta.init.clone(), seed: 11 };
        let Some(mut sim) = sim_with(cfg) else { return };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        upload_default_params(&mut sim, &p.rule, &p.render);
        assert!(step_and_check(&mut sim, 3000).is_none());
        let before = live_cells(&read_back(&sim, sim.cur));
        assert!(before.len() > 100, "population collapsed: {}", before.len());
        assert!(step_and_check(&mut sim, 1).is_none());
        let after = live_cells(&read_back(&sim, sim.cur));
        let changed =
            before.iter().filter(|c| !after.contains(c)).count() + after.iter().filter(|c| !before.contains(c)).count();
        assert!(changed > 20, "the grid has gone still: only {changed} cells changed in one step");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn one_d_steps_past_the_bottom_and_with_height_one_without_validation_errors() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[0]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        for h in [1u32, 2, 5] {
            let Some(mut sim) = sim_with(config(Mode::OneD, 16, h)) else { return };
            sim.set_pipelines(&rule, &render, &post, None).unwrap();
            assert!(step_and_check(&mut sim, h + 7).is_none(), "height {h}: validation error");
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn lifelike_template_runs_and_keeps_live_cells() {
        let _gpu = gpu_lock();
        let t = load_builtin(&crate::preset::builtin::TEMPLATES[1]);
        let (rule, render, post) = assembled_for(&t.rule, &t.render);
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 64,
            height: 64,
            init: InitPattern::Random { density: 0.4 },
            seed: 3,
        }) else {
            return;
        };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        upload_default_params(&mut sim, &t.rule, &t.render);
        assert!(step_and_check(&mut sim, 10).is_none());
        let live = read_back(&sim, sim.cur).iter().step_by(4).filter(|&&r| r > 0.5).count();
        assert!(live > 0, "B3/S23 from a 40% random soup should still have live cells after 10 steps");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn export_produces_a_decodable_png_with_live_pixels_and_reuses_its_targets() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 16,
            height: 16,
            init: InitPattern::Random { density: 0.5 },
            seed: 11,
        }) else {
            return;
        };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        upload_default_params(&mut sim, &p.rule, &p.render);
        assert!(step_and_check(&mut sim, 2).is_none());
        let export = |sim: &mut Simulation, name: &str| {
            sim.start_export(2, name.into()).unwrap();
            assert!(sim.export_pending());
            for _ in 0..100 {
                let _ = sim.ctx.device.poll(wgpu::PollType::wait_indefinitely());
                if let Some(r) = sim.poll_export() {
                    return r.expect("export ok");
                }
            }
            panic!("export never finished");
        };
        let img = export(&mut sim, "life.png");
        assert_eq!(img.filename, "life.png");
        assert_eq!((img.width, img.height), (32, 32));
        let decoder = png::Decoder::new(std::io::Cursor::new(img.to_png().unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (32, 32), "2x scale of a 16x16 grid");
        let lit = buf[..info.buffer_size()].chunks(4).filter(|px| px[0] > 0 || px[1] > 0 || px[2] > 0).count();
        assert!(lit > 0, "some cells must be coloured");
        assert!(!sim.export_pending());
        // A second export of the same size reuses the cached targets and buffer.
        let first_buffer = sim.export_targets.as_ref().map(|t| t.buffer.clone()).expect("targets are kept");
        let img2 = export(&mut sim, "again.png");
        assert_eq!((img2.width, img2.height), (32, 32));
        assert!(sim.export_targets.as_ref().is_some_and(|t| t.buffer == first_buffer), "same readback buffer");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn painting_writes_a_disc_into_the_current_texture() {
        let _gpu = gpu_lock();
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::TwoD, width: 32, height: 32, init: InitPattern::Blank, seed: 1 })
        else {
            return;
        };
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        sim.paint(&mut enc, &[Stroke { x: 16, y: 16, radius: 3.0, value: [1.0, 0.0, 0.0, 1.0] }]);
        sim.ctx.queue.submit([enc.finish()]);
        let data = read_back(&sim, sim.cur);
        let at = |x: usize, y: usize| data[(y * 32 + x) * 4];
        assert_eq!(at(16, 16), 1.0, "centre is painted");
        assert_eq!(at(16, 19), 1.0, "radius 3 reaches 3 cells away");
        assert_eq!(at(16, 21), 0.0, "5 cells away is untouched");
        assert_eq!(at(0, 0), 0.0);
        let painted = (0..32 * 32).filter(|i| data[i * 4] > 0.5).count();
        assert!((25..=37).contains(&painted), "about pi*r^2 cells: {painted}");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn stats_count_population_and_changes() {
        let _gpu = gpu_lock();
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::TwoD, width: 32, height: 32, init: InitPattern::Blank, seed: 1 })
        else {
            return;
        };
        // Paint a disc into the current texture; the other texture stays blank, so every
        // painted cell also counts as changed.
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        sim.paint(&mut enc, &[Stroke { x: 16, y: 16, radius: 3.0, value: [1.0, 0.0, 0.0, 1.0] }]);
        sim.collect_stats(&mut enc);
        sim.ctx.queue.submit([enc.finish()]);
        let painted = read_back(&sim, sim.cur).iter().step_by(4).filter(|&&r| r > 0.5).count() as u32;
        let mut samples = Vec::new();
        for _ in 0..50 {
            samples.extend(sim.poll_stats());
            if !samples.is_empty() {
                break;
            }
            let _ = sim.ctx.device.poll(wgpu::PollType::wait_indefinitely());
        }
        let s = samples.first().expect("a stats sample");
        assert_eq!(s.population, painted);
        assert_eq!(s.changed, painted);
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn load_state_round_trips_image_cells_into_both_textures() {
        use crate::sim::seed_image::{RgbaImage, SeedMode, image_to_cells};
        let _gpu = gpu_lock();
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::TwoD, width: 16, height: 16, init: InitPattern::Blank, seed: 1 })
        else {
            return;
        };
        // 2x2 image: white, black / black, white -> four 8x8 blocks.
        let img = RgbaImage {
            width: 2,
            height: 2,
            rgba: vec![255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255],
        };
        let cells = image_to_cells(&img, 16, 16, SeedMode::Luminance { threshold: 0.5 });
        sim.load_state(&cells).unwrap();
        let a = read_back(&sim, 0);
        let b = read_back(&sim, 1);
        assert_eq!(a, cells);
        assert_eq!(b, cells, "both textures hold the loaded state");
        assert!(sim.load_state(&cells[..8]).is_err(), "wrong size is rejected");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn a_rule_pair_compiles_and_blends() {
        use crate::shader::assemble::assemble_rule_pair;
        use crate::shader::validate::validate_pair;
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (_, render, post) = assembled_for(&p.rule, &p.render);
        // Rule B: everything on. At blend 1 the grid must become fully alive after one step.
        let all_on = "fn rule(pos: vec2<u32>) -> vec4<f32> { return on(); }";
        let specs = parse_params(&p.rule).unwrap();
        let pair = validate_pair(&assemble_rule_pair(&p.rule, all_on, &params_wgsl(&specs))).unwrap();
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::TwoD, width: 16, height: 16, init: InitPattern::Blank, seed: 1 })
        else {
            return;
        };
        sim.set_pipelines(&pair, &render, &post, None).unwrap();
        sim.set_blend(1.0);
        assert!(step_and_check(&mut sim, 1).is_none());
        let live = read_back(&sim, sim.cur).iter().step_by(4).filter(|&&r| r > 0.5).count();
        assert_eq!(live, 256, "blend 1 selects rule B (all on)");
        sim.set_blend(0.0);
        assert!(step_and_check(&mut sim, 1).is_none());
        let live = read_back(&sim, sim.cur).iter().step_by(4).filter(|&&r| r > 0.5).count();
        assert_eq!(live, 0, "blend 0 selects rule A (Life kills a full grid)");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn snapshots_can_be_restored_and_are_allocated_lazily() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) =
            sim_with(SimConfig { mode: Mode::TwoD, width: 16, height: 16, init: InitPattern::Blank, seed: 1 })
        else {
            return;
        };
        assert!(sim.history_tex.iter().all(Option::is_none), "no history texture before the first snapshot");
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        upload_default_params(&mut sim, &p.rule, &p.render);
        sim.set_snapshot_interval(1);
        // Paint a lone dot (dies in one Life step) and snapshot that state.
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        sim.paint(&mut enc, &[Stroke { x: 8, y: 8, radius: 0.5, value: [1.0, 0.0, 0.0, 1.0] }]);
        sim.snapshot_now(&mut enc);
        sim.ctx.queue.submit([enc.finish()]);
        let painted = read_back(&sim, sim.cur);
        assert!(painted.iter().step_by(4).any(|&r| r > 0.5));
        assert!(step_and_check(&mut sim, 3).is_none());
        assert_eq!(sim.history_len(), 4, "one manual plus one per step");
        assert_eq!(sim.history_tex.iter().filter(|t| t.is_some()).count(), 4, "exactly the used slots exist");
        assert_eq!(sim.frame(), 3);
        assert!(read_back(&sim, sim.cur).iter().step_by(4).all(|&r| r == 0.0), "the dot died");
        // Restore the first snapshot: the dot is back and the step counter rewinds.
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        let meta = sim.restore_snapshot(&mut enc, 0).unwrap();
        sim.ctx.queue.submit([enc.finish()]);
        assert_eq!(meta.step, 0);
        assert_eq!(sim.frame(), 0);
        assert_eq!(read_back(&sim, 0), painted);
        assert_eq!(read_back(&sim, 1), painted, "both ping-pong textures are restored");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn disabled_history_never_allocates() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::TwoD, 16, 16)) else { return };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        sim.disable_history();
        sim.set_snapshot_interval(1);
        assert!(step_and_check(&mut sim, 5).is_none());
        assert_eq!(sim.history_len(), 0);
        assert!(sim.history_tex.iter().all(Option::is_none));
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn a_layer_can_read_the_other_layers_state() {
        let _gpu = gpu_lock();
        // Both layers must live on the same device, as in the app.
        let Some(ctx) = context() else { return };
        let cfg = |seed| SimConfig { mode: Mode::TwoD, width: 16, height: 16, init: InitPattern::Blank, seed };
        let mut a = Simulation::new(ctx.clone(), cfg(1));
        let mut b = Simulation::new(ctx.clone(), cfg(2));
        // A copies B; B is everything-on.
        let p = load_builtin(&BUILTINS[2]);
        let (_, render, post) = assembled_for(&p.rule, &p.render);
        let pw = params_wgsl(&[]);
        let copy_rule = validated_rule(
            "fn rule(pos: vec2<u32>) -> vec4<f32> { return on_if(other(i32(pos.x), i32(pos.y)).r > 0.5); }",
            &pw,
        );
        let all_on = validated_rule("fn rule(pos: vec2<u32>) -> vec4<f32> { return on(); }", &pw);
        a.set_pipelines(&copy_rule, &render, &post, None).unwrap();
        b.set_pipelines(&all_on, &render, &post, None).unwrap();
        // Mirrors: each layer reads a copy of the other's state taken before stepping.
        let mirror_b = b.create_mirror_texture();
        a.set_other(Some(&mirror_b));
        let mut enc = ctx.device.create_command_encoder(&Default::default());
        b.mirror_into(&mut enc, &mirror_b); // B is still blank here
        a.step(&mut enc, 1);
        ctx.queue.submit([enc.finish()]);
        assert!(read_back(&a, a.cur).iter().step_by(4).all(|&r| r == 0.0), "blank B -> A stays off");
        let mut enc = ctx.device.create_command_encoder(&Default::default());
        b.step(&mut enc, 1); // B turns fully on
        b.mirror_into(&mut enc, &mirror_b);
        a.step(&mut enc, 1);
        ctx.queue.submit([enc.finish()]);
        let live = read_back(&a, a.cur).iter().step_by(4).filter(|&&r| r > 0.5).count();
        assert_eq!(live, 256, "A copied B's all-on state");
        a.set_other(None);
        assert!(step_and_check(&mut a, 1).is_none(), "without a layer B, other() reads zeros");
        assert!(read_back(&a, a.cur).iter().step_by(4).all(|&r| r == 0.0), "the has_other flag gates other()");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn slither_snakes_survive_and_keep_moving_with_their_scent_layer() {
        let _gpu = gpu_lock();
        let Some(ctx) = context() else { return };
        let p = load_builtin(BUILTINS.iter().find(|b| b.id == "slither").unwrap());
        let scent = p.layer_b.clone().expect("slither carries its scent layer");
        let build = |preset: &crate::preset::Preset| {
            let (editor, config, _, toml_params) = crate::app::state::preset_to_state(preset);
            let built = crate::app::state::build_shaders(&editor).unwrap();
            let mut values = crate::app::state::resolve_values(&built.specs, &toml_params, &Default::default());
            // No new snakes or food during the test, so the snake mass can only be moved by
            // eating, or gained from the food the seed scattered.
            values.insert("spawn".into(), crate::shader::params::ParamValue::F32(0.0));
            values.insert("food".into(), crate::shader::params::ParamValue::F32(0.0));
            let mut sim = Simulation::new(
                ctx.clone(),
                SimConfig { mode: Mode::TwoD, width: 256, height: 256, init: config.init, seed: config.seed },
            );
            sim.disable_history();
            sim.set_pipelines(&built.rule, &built.render, &built.post, built.seed.as_ref()).unwrap();
            sim.set_params(pack_params(&built.specs, &values));
            sim
        };
        let mut a = build(&p);
        let mut b = build(&scent);
        let mirror_a = a.create_mirror_texture();
        let mirror_b = b.create_mirror_texture();
        a.set_other(Some(&mirror_b));
        b.set_other(Some(&mirror_a));
        // `.r` packs kind * 8 + direction + 32 * id (see presets/slither/rule.wgsl).
        let kind = |c: &[f32]| ((c[0].round() as i32) % 32) / 8;
        let snake_cells = |sim: &Simulation| {
            let data = read_back(sim, sim.cur);
            let heads = data.chunks(4).filter(|c| kind(c) == 2).count();
            let bodies = data.chunks(4).filter(|c| kind(c) == 1).count();
            (heads, bodies)
        };
        let food_cells = |sim: &Simulation| read_back(sim, sim.cur).chunks(4).filter(|c| kind(c) == 3).count();
        // A snake's length is its head's move count (.b / 2048) minus the tail's stamp (.a) plus
        // one, with the counters wrapping at 2048.
        let length = |c: &[f32]| {
            let moves = c[2].round() as i32 / 2048;
            (((moves - c[3].round() as i32) % 2048 + 2048) % 2048 + 1) as f32
        };
        let longest = |sim: &Simulation| {
            read_back(sim, sim.cur).chunks(4).filter(|c| kind(c) == 2).map(length).fold(0.0, f32::max)
        };
        // Lockstep, as the viewport does it.
        let run = |a: &mut Simulation, b: &mut Simulation, steps: u32| {
            let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut enc = ctx.device.create_command_encoder(&Default::default());
            for _ in 0..steps {
                a.mirror_into(&mut enc, &mirror_a);
                b.mirror_into(&mut enc, &mirror_b);
                b.step(&mut enc, 1);
                a.step(&mut enc, 1);
            }
            ctx.queue.submit([enc.finish()]);
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
            pollster::block_on(scope.pop())
        };
        assert!(run(&mut a, &mut b, 1).is_none());
        let (heads0, bodies0) = snake_cells(&a);
        let food0 = food_cells(&a);
        assert!(heads0 >= 2, "the seed should scatter several starter heads, got {heads0}");
        assert!(food0 > heads0, "the seed should scatter food, got {food0} morsels");
        assert!(
            bodies0 <= heads0,
            "after one move each snake has at most one body cell: {heads0} heads, {bodies0} bodies"
        );
        for round in 1..=10 {
            assert!(run(&mut a, &mut b, 300).is_none(), "validation error while slithering");
            let (heads, bodies) = snake_cells(&a);
            let food = food_cells(&a);
            println!(
                "step {}: {heads} heads, {bodies} body cells, {food} food, longest snake {:.0}",
                round * 300,
                longest(&a)
            );
            assert!(heads >= 1, "every snake died");
            // A starter owes at most 1 + 2 * spawn_length (6) cells of growth, so no snake can
            // exceed fifteen cells without eating; eating another snake only moves cells between
            // snakes, and eating food turns one morsel into one cell of snake.
            assert!(
                heads + bodies + food <= heads0 * 15 + food0,
                "mass was created out of nothing: {heads0} starters and {food0} food, now {} snake cells and {food} food",
                heads + bodies
            );
        }
        let (heads, bodies) = snake_cells(&a);
        assert!(bodies > heads, "snakes should have grown bodies: {heads} heads, {bodies} body cells");
        let before = read_back(&a, a.cur);
        assert!(run(&mut a, &mut b, 3).is_none());
        assert_ne!(before, read_back(&a, a.cur), "the snakes stopped moving");
        let scent: f32 = read_back(&b, b.cur).chunks(4).map(|c| c[0]).sum();
        assert!(scent > 0.0, "the scent layer never picked up the snakes");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn slither_does_not_lose_its_longest_snake_over_a_long_run() {
        let _gpu = gpu_lock();
        let Some(ctx) = context() else { return };
        let p = load_builtin(BUILTINS.iter().find(|b| b.id == "slither").unwrap());
        let scent = p.layer_b.clone().expect("slither carries its scent layer");
        // The preset as shipped: spawning and food on.
        let build = |preset: &crate::preset::Preset| {
            let (editor, config, _, toml_params) = crate::app::state::preset_to_state(preset);
            let built = crate::app::state::build_shaders(&editor).unwrap();
            let values = crate::app::state::resolve_values(&built.specs, &toml_params, &Default::default());
            let mut sim = Simulation::new(
                ctx.clone(),
                SimConfig { mode: Mode::TwoD, width: 256, height: 256, init: config.init, seed: config.seed },
            );
            sim.disable_history();
            sim.set_pipelines(&built.rule, &built.render, &built.post, built.seed.as_ref()).unwrap();
            sim.set_params(pack_params(&built.specs, &values));
            sim
        };
        let mut a = build(&p);
        let mut b = build(&scent);
        let mirror_a = a.create_mirror_texture();
        let mirror_b = b.create_mirror_texture();
        a.set_other(Some(&mirror_b));
        b.set_other(Some(&mirror_a));
        let run = |a: &mut Simulation, b: &mut Simulation, steps: u32| {
            let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut enc = ctx.device.create_command_encoder(&Default::default());
            for _ in 0..steps {
                a.mirror_into(&mut enc, &mirror_a);
                b.mirror_into(&mut enc, &mirror_b);
                b.step(&mut enc, 1);
                a.step(&mut enc, 1);
            }
            ctx.queue.submit([enc.finish()]);
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
            pollster::block_on(scope.pop())
        };
        // Cell layout, see presets/slither/rule.wgsl: kind and id in .r, stamp + 2048 * moves
        // in .b, the tail's stamp in .a; length = moves - tail stamp + 1.
        let kind = |c: &[f32]| ((c[0].round() as i32) % 32) / 8;
        let id = |c: &[f32]| (c[0].round() as i32) / 32;
        let length = |c: &[f32]| {
            let moves = c[2].round() as i32 / 2048;
            ((moves - c[3].round() as i32) % 2048 + 2048) % 2048 + 1
        };
        let heads = |sim: &Simulation| -> Vec<(i32, i32)> {
            read_back(sim, sim.cur).chunks(4).filter(|c| kind(c) == 2).map(|c| (id(c), length(c))).collect()
        };
        let mut previous: Option<(i32, i32)> = None;
        let mut lost: Vec<(u32, i32, i32)> = Vec::new();
        for round in 1..=40u32 {
            assert!(run(&mut a, &mut b, 300).is_none(), "validation error while slithering");
            let hs = heads(&a);
            let longest = hs.iter().copied().max_by_key(|h| h.1);
            if let Some((pid, plen)) = previous
                && !hs.iter().any(|h| h.0 == pid)
            {
                lost.push((round * 300, pid, plen));
            }
            if let Some((lid, llen)) = longest {
                println!("step {}: {} snakes, longest id {lid} length {llen}", round * 300, hs.len());
            }
            previous = longest;
        }
        println!("longest snakes that vanished (step, id, length): {lost:?}");
        let big_losses: Vec<_> = lost.iter().filter(|(_, _, l)| *l >= 15).collect();
        assert!(big_losses.is_empty(), "a long snake vanished: {big_losses:?}");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn reconfigure_clamps_and_resizes() {
        let _gpu = gpu_lock();
        let Some(mut sim) = sim_with(config(Mode::TwoD, 8, 8)) else { return };
        let huge = sim.ctx.device.limits().max_texture_dimension_2d.saturating_mul(2);
        sim.reconfigure(config(Mode::TwoD, huge, 4));
        assert!(sim.config().width <= sim.ctx.device.limits().max_texture_dimension_2d);
        assert_eq!(sim.config().height, 4);
        assert_eq!(sim.frame(), 0);
    }

    fn read_globals(sim: &Simulation) -> Globals {
        let buf = sim.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = sim.ctx.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&sim.globals_buf, 0, &buf, 0, std::mem::size_of::<Globals>() as u64);
        sim.ctx.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        let _ = sim.ctx.device.poll(wgpu::PollType::wait_indefinitely());
        let view = slice.get_mapped_range().expect("buffer mapped");
        *bytemuck::from_bytes::<Globals>(&view[..])
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn time_reaches_the_gpu_while_paused_and_frame_counts_match_after_steps() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render, post) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::TwoD, 16, 16)) else { return };
        sim.set_pipelines(&rule, &render, &post, None).unwrap();
        sim.set_time(5.5);
        assert!(step_and_check(&mut sim, 0).is_none());
        assert_eq!(read_globals(&sim).time, 5.5, "paused: time must still be uploaded");
        assert!(step_and_check(&mut sim, 7).is_none());
        let g = read_globals(&sim);
        assert_eq!(g.frame, 6, "the buffer holds the globals of the last recorded step");
        assert_eq!(sim.frame(), 7);
    }
}

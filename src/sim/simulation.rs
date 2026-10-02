//! GPU simulation: two ping-pong `rgba32float` textures, a compute pipeline built from the
//! user's rule shader and a render pipeline built from the user's render shader.

use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::preset::{InitPattern, Mode};
use crate::shader::assemble::Assembled;
use crate::shader::params::MAX_PARAMS;
use crate::shader::validate::{validate, ShaderError, ShaderFile};
use crate::sim::export::{
    clamp_scale, encode_png, padded_bytes_per_row, to_rgba, unpad_rows, ExportedImage,
};
use crate::sim::init::generate_init;
use crate::sim::paint::{brush_bbox, PaintUniform, Stroke, PAINT_WGSL};
use crate::sim::stats::{StatsSample, STATS_WGSL};
use crate::sim::row::{clamp_size, plan_row, RowPlan, WORKGROUP};
use crate::sim::uniforms::{Globals, ParamsData};

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

/// One slot of the statistics readback pool.
struct StatsSlot {
    buffer: wgpu::Buffer,
    state: StatsSlotState,
    step: u32,
    mapped: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatsSlotState {
    Free,
    /// Copy recorded this frame; map once the frame has been submitted.
    Recorded,
    Mapping,
}

pub struct Simulation {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target_format: wgpu::TextureFormat,
    config: SimConfig,
    compute_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    paint_layout: wgpu::BindGroupLayout,
    paint_pipeline: wgpu::ComputePipeline,
    paint_buf: wgpu::Buffer,
    stats_layout: wgpu::BindGroupLayout,
    stats_pipeline: wgpu::ComputePipeline,
    stats_buf: wgpu::Buffer,
    stats_slots: Vec<StatsSlot>,
    compute_pipeline_layout: wgpu::PipelineLayout,
    render_pipeline_layout: wgpu::PipelineLayout,
    globals_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    textures: Textures,
    /// Index of the texture holding the current state.
    cur: usize,
    compute: Option<wgpu::ComputePipeline>,
    render: Option<wgpu::RenderPipeline>,
    globals: Globals,
    /// Set by wgpu's device-lost callback; drained by the app once per frame.
    device_lost: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    export: Option<PendingExport>,
}

/// An offscreen render that has been submitted and whose readback buffer is being mapped.
struct PendingExport {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bpr: u32,
    filename: String,
    /// `Some(ok)` once the map callback ran.
    mapped: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

impl Simulation {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
        mut config: SimConfig,
    ) -> Self {
        let limits = device.limits();
        config.width = clamp_size(config.width, &limits);
        config.height = clamp_size(config.height, &limits);

        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE
                | wgpu::ShaderStages::FRAGMENT
                | wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let sampled = |visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ca compute layout"),
            entries: &[
                sampled(wgpu::ShaderStages::COMPUTE),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                uniform(2),
                uniform(3),
            ],
        });
        let render_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ca render layout"),
            entries: &[sampled(wgpu::ShaderStages::FRAGMENT), uniform(2), uniform(3)],
        });
        let paint_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ca paint layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                uniform(1),
            ],
        });
        let paint_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ca paint"),
            source: wgpu::ShaderSource::Wgsl(PAINT_WGSL.into()),
        });
        let paint_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ca paint pipeline layout"),
            bind_group_layouts: &[Some(&paint_layout)],
            immediate_size: 0,
        });
        let paint_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ca paint pipeline"),
            layout: Some(&paint_pipeline_layout),
            module: &paint_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let paint_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ca paint uniform"),
            size: std::mem::size_of::<PaintUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stats_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ca stats layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let stats_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ca stats"),
            source: wgpu::ShaderSource::Wgsl(STATS_WGSL.into()),
        });
        let stats_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ca stats pipeline layout"),
            bind_group_layouts: &[Some(&stats_layout)],
            immediate_size: 0,
        });
        let stats_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ca stats pipeline"),
            layout: Some(&stats_pipeline_layout),
            module: &stats_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
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
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ca compute pipeline layout"),
                bind_group_layouts: &[Some(&compute_layout)],
                immediate_size: 0,
            });
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ca render pipeline layout"),
                bind_group_layouts: &[Some(&render_layout)],
                immediate_size: 0,
            });
        let globals = Globals {
            size: [config.width, config.height],
            mode: mode_code(config.mode),
            seed: config.seed,
            ..Default::default()
        };
        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });
        let params: ParamsData = [[0; 4]; MAX_PARAMS];
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let textures = create_textures(
            &device,
            &compute_layout,
            &render_layout,
            &paint_layout,
            &stats_layout,
            &globals_buf,
            &params_buf,
            &paint_buf,
            &stats_buf,
            config.width,
            config.height,
        );
        // Anything the error scopes miss is logged rather than aborting the process.
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            log::error!("uncaptured wgpu error: {e}");
        }));
        let device_lost: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
        {
            let flag = device_lost.clone();
            device.set_device_lost_callback(move |reason, message| {
                let text = format!("GPU device lost ({reason:?}): {message}");
                log::error!("{text}");
                *flag.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
            });
        }
        let mut sim = Simulation {
            device,
            queue,
            target_format,
            config,
            compute_layout,
            render_layout,
            paint_layout,
            paint_pipeline,
            paint_buf,
            stats_layout,
            stats_pipeline,
            stats_buf,
            stats_slots,
            compute_pipeline_layout,
            render_pipeline_layout,
            globals_buf,
            params_buf,
            textures,
            cur: 0,
            compute: None,
            render: None,
            globals,
            device_lost,
            export: None,
        };
        sim.reset();
        sim
    }

    pub fn export_pending(&self) -> bool {
        self.export.is_some()
    }

    /// Renders the current state at `scale` pixels per cell into an offscreen texture and starts
    /// reading it back. The result arrives through `poll_export` on a later frame.
    pub fn start_export(&mut self, scale: u32, filename: String) -> Result<(), String> {
        if self.export.is_some() {
            return Err("an image export is already in progress".into());
        }
        let Some(pipeline) = &self.render else {
            return Err("no render pipeline: fix the shaders first".into());
        };
        let max_dim = self.device.limits().max_texture_dimension_2d;
        let s = clamp_scale(scale, self.config.width, self.config.height, max_dim);
        let (width, height) = (self.config.width * s, self.config.height * s);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ca export target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let padded_bpr = padded_bytes_per_row(width * 4);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ca export readback"),
            size: (padded_bpr * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ca export"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ca export pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.textures.render_bind_groups[self.cur], &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let mapped = std::sync::Arc::new(std::sync::Mutex::new(None));
        let flag = mapped.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            *flag.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.is_ok());
        });
        self.export = Some(PendingExport { buffer, width, height, padded_bpr, filename, mapped });
        Ok(())
    }

    /// Call once per frame while an export is pending. Returns the PNG when the readback is done.
    pub fn poll_export(&mut self) -> Option<Result<ExportedImage, String>> {
        let state = self.export.as_ref()?.mapped.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(ok) = state else {
            // Give the mapping a chance to complete (a no-op in the browser, where it resolves
            // from the event loop instead).
            let _ = self.device.poll(wgpu::PollType::Poll);
            return None;
        };
        let pending = self.export.take()?;
        if !ok {
            return Some(Err("GPU readback failed".into()));
        }
        let result = (|| -> anyhow::Result<ExportedImage> {
            let view = pending.buffer.slice(..).get_mapped_range()?;
            let mut rgba = unpad_rows(
                &view[..],
                pending.padded_bpr as usize,
                (pending.width * 4) as usize,
                pending.height as usize,
            );
            drop(view);
            pending.buffer.unmap();
            to_rgba(&mut rgba, self.target_format);
            let png = encode_png(pending.width, pending.height, &rgba)?;
            Ok(ExportedImage { filename: pending.filename.clone(), png })
        })();
        Some(result.map_err(|e| format!("image export failed: {e:#}")))
    }

    /// Returns the device-lost message once, if the device was lost since the last call.
    pub fn take_device_lost(&self) -> Option<String> {
        self.device_lost.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn frame(&self) -> u32 {
        self.globals.frame
    }

    pub fn has_pipelines(&self) -> bool {
        self.compute.is_some() && self.render.is_some()
    }

    /// Applies a new configuration (clamped to device limits), recreating textures if the size
    /// changed, and resets.
    pub fn reconfigure(&mut self, mut config: SimConfig) {
        let limits = self.device.limits();
        config.width = clamp_size(config.width, &limits);
        config.height = clamp_size(config.height, &limits);
        if config.width != self.config.width || config.height != self.config.height {
            self.textures = create_textures(
                &self.device,
                &self.compute_layout,
                &self.render_layout,
                &self.paint_layout,
                &self.stats_layout,
                &self.globals_buf,
                &self.params_buf,
                &self.paint_buf,
                &self.stats_buf,
                config.width,
                config.height,
            );
        }
        self.config = config;
        self.reset();
    }

    pub fn reset(&mut self) {
        let c = &self.config;
        let data = generate_init(&c.init, c.mode, c.width, c.height, c.seed);
        self.cur = 0;
        // Both textures get the init state: in 1D mode a step only copies the rows above the
        // write head, so stale rows in the other texture would otherwise show through.
        for tex in &self.textures.tex {
            self.queue.write_texture(
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
        self.globals = Globals {
            size: [c.width, c.height],
            frame: 0,
            seed: c.seed,
            time: 0.0,
            mode: mode_code(c.mode),
            row: 1,
            prev_row: 0,
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
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
            self.queue.write_texture(
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
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
        Ok(())
    }

    pub fn set_params(&mut self, data: ParamsData) {
        self.queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&data));
    }

    pub fn set_time(&mut self, seconds: f32) {
        self.globals.time = seconds;
    }

    fn backend_error(file: ShaderFile, err: Option<wgpu::Error>) -> Result<(), Vec<ShaderError>> {
        match err {
            None => Ok(()),
            Some(e) => Err(vec![ShaderError {
                file,
                line: 1,
                column: 1,
                message: format!("GPU backend rejected shader: {e}"),
                hint: None,
            }]),
        }
    }

    /// Validates with naga, then creates the module inside a validation error scope so a
    /// backend rejection is returned instead of hitting the uncaptured-error handler.
    fn create_module(
        &self,
        file: ShaderFile,
        assembled: &Assembled,
    ) -> Result<wgpu::ShaderModule, Vec<ShaderError>> {
        validate(file, assembled)?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(file.label()),
            source: wgpu::ShaderSource::Wgsl(assembled.source.as_str().into()),
        });
        Self::backend_error(file, pop_scope(scope))?;
        Ok(module)
    }

    fn build_compute(&self, assembled: &Assembled) -> Result<wgpu::ComputePipeline, Vec<ShaderError>> {
        let file = ShaderFile::Rule;
        let module = self.create_module(file, assembled)?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ca compute"),
            layout: Some(&self.compute_pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self::backend_error(file, pop_scope(scope))?;
        Ok(pipeline)
    }

    fn build_render(&self, assembled: &Assembled) -> Result<wgpu::RenderPipeline, Vec<ShaderError>> {
        let file = ShaderFile::Render;
        let module = self.create_module(file, assembled)?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ca render"),
            layout: Some(&self.render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self::backend_error(file, pop_scope(scope))?;
        Ok(pipeline)
    }

    /// Builds both pipelines and swaps them in together. On any error nothing changes, so the
    /// live pipelines always agree on the `Params` layout. Errors from both shaders are returned.
    pub fn set_pipelines(
        &mut self,
        rule: &Assembled,
        render: &Assembled,
    ) -> Result<(), Vec<ShaderError>> {
        let compute = self.build_compute(rule);
        let render = self.build_render(render);
        match (compute, render) {
            (Ok(c), Ok(r)) => {
                self.compute = Some(c);
                self.render = Some(r);
                Ok(())
            }
            (c, r) => {
                let mut errors = Vec::new();
                if let Err(e) = c {
                    errors.extend(e);
                }
                if let Err(e) = r {
                    errors.extend(e);
                }
                Err(errors)
            }
        }
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
                    let bb = brush_bbox(stroke.x, row as i32, stroke.radius, w, h)
                        .map(|(x0, _, bw, _)| (x0, row, bw, 1));
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
            let staging = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ca paint staging"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::COPY_SRC,
            });
            encoder.copy_buffer_to_buffer(
                &staging,
                0,
                &self.paint_buf,
                0,
                std::mem::size_of::<PaintUniform>() as u64,
            );
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ca paint"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.paint_pipeline);
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
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ca stats"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.stats_pipeline);
            pass.set_bind_group(0, &self.textures.stats_bind_groups[self.cur], &[]);
            pass.dispatch_workgroups(
                self.config.width.div_ceil(WORKGROUP),
                self.config.height.div_ceil(WORKGROUP),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&self.stats_buf, 0, &slot.buffer, 0, 8);
        slot.state = StatsSlotState::Recorded;
        slot.step = self.globals.frame;
    }

    /// Advances the readback pool; call once per frame. Returns finished samples, oldest first.
    pub fn poll_stats(&mut self) -> Vec<StatsSample> {
        let mut out = Vec::new();
        let _ = self.device.poll(wgpu::PollType::Poll);
        let mut mapped_any = false;
        for slot in &mut self.stats_slots {
            match slot.state {
                StatsSlotState::Free => {}
                StatsSlotState::Recorded => {
                    // The copy was submitted with the previous frame; the buffer can be mapped now.
                    let flag = slot.mapped.clone();
                    *flag.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                        *flag.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.is_ok());
                    });
                    slot.state = StatsSlotState::Mapping;
                    mapped_any = true;
                }
                StatsSlotState::Mapping => {
                    let done = slot.mapped.lock().unwrap_or_else(|e| e.into_inner()).take();
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
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        out.sort_by_key(|s| s.step);
        out
    }

    /// Records `n` simulation steps into `encoder`. With `n == 0` only the globals (notably
    /// `time`) are refreshed, so time-based render shaders keep moving while paused.
    pub fn step(&mut self, encoder: &mut wgpu::CommandEncoder, n: u32) {
        if n == 0 || self.compute.is_none() {
            self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
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
        let staging = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals staging"),
            contents: bytemuck::cast_slice(&snapshots),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let size = std::mem::size_of::<Globals>() as u64;
        let pipeline = self.compute.as_ref().expect("checked above");

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
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ca step"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.textures.compute_bind_groups[src], &[]);
            match plan {
                None => pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1),
                Some(_) => pass.dispatch_workgroups(w.div_ceil(WORKGROUP), 1, 1),
            }
            drop(pass);
            cur = dst;
        }
        self.cur = cur;
    }

    /// Draws the current state with the render pipeline into the active render pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some(pipeline) = &self.render else { return };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.textures.render_bind_groups[self.cur], &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Resolves a validation error scope. In the browser the result is a promise that cannot be
/// awaited synchronously, so the scope is simply closed; naga validation already rejects what
/// the device cannot run and anything else reaches the uncaptured-error handler (console).
#[cfg(not(target_arch = "wasm32"))]
fn pop_scope(scope: wgpu::ErrorScopeGuard) -> Option<wgpu::Error> {
    pollster::block_on(scope.pop())
}

#[cfg(target_arch = "wasm32")]
fn pop_scope(scope: wgpu::ErrorScopeGuard) -> Option<wgpu::Error> {
    drop(scope);
    None
}

fn mode_code(mode: Mode) -> u32 {
    match mode {
        Mode::TwoD => 0,
        Mode::OneD => 1,
    }
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

#[allow(clippy::too_many_arguments)]
fn create_textures(
    device: &wgpu::Device,
    compute_layout: &wgpu::BindGroupLayout,
    render_layout: &wgpu::BindGroupLayout,
    paint_layout: &wgpu::BindGroupLayout,
    stats_layout: &wgpu::BindGroupLayout,
    globals_buf: &wgpu::Buffer,
    params_buf: &wgpu::Buffer,
    paint_buf: &wgpu::Buffer,
    stats_buf: &wgpu::Buffer,
    width: u32,
    height: u32,
) -> Textures {
    let make = |label: &str| {
        device.create_texture(&wgpu::TextureDescriptor {
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
    let compute_bg = |src: usize| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca compute bg"),
            layout: compute_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[src]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1 - src]),
                },
                wgpu::BindGroupEntry { binding: 2, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
            ],
        })
    };
    let render_bg = |src: usize| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca render bg"),
            layout: render_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[src]),
                },
                wgpu::BindGroupEntry { binding: 2, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
            ],
        })
    };
    let paint_bg = |dst: usize| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca paint bg"),
            layout: paint_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[dst]),
                },
                wgpu::BindGroupEntry { binding: 1, resource: paint_buf.as_entire_binding() },
            ],
        })
    };
    let stats_bg = |cur: usize| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ca stats bg"),
            layout: stats_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[cur]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1 - cur]),
                },
                wgpu::BindGroupEntry { binding: 2, resource: stats_buf.as_entire_binding() },
            ],
        })
    };
    let compute_bind_groups = [compute_bg(0), compute_bg(1)];
    let render_bind_groups = [render_bg(0), render_bg(1)];
    let paint_bind_groups = [paint_bg(0), paint_bg(1)];
    let stats_bind_groups = [stats_bg(0), stats_bg(1)];
    Textures { tex, compute_bind_groups, render_bind_groups, paint_bind_groups, stats_bind_groups }
}

/// GPU tests: need a real adapter, so they are `#[ignore]`d for CI. Run with
/// `cargo test gpu_tests -- --ignored`.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::preset::builtin::{load_builtin, BUILTINS};
    use crate::shader::assemble::{assemble_render, assemble_rule};
    use crate::shader::params::{merge_params, pack_params, params_wgsl, parse_params};

    /// Creating several devices concurrently from test threads hangs on some drivers, so every
    /// GPU test holds this lock for its whole duration.
    static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
        GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some((device, queue))
    }

    fn sim_with(config: SimConfig) -> Option<Simulation> {
        let (device, queue) = device()?;
        Some(Simulation::new(device, queue, wgpu::TextureFormat::Bgra8UnormSrgb, config))
    }

    fn config(mode: Mode, w: u32, h: u32) -> SimConfig {
        SimConfig { mode, width: w, height: h, init: InitPattern::Single, seed: 1 }
    }

    fn assembled_for(rule: &str, render: &str) -> (Assembled, Assembled) {
        let specs = merge_params(parse_params(rule).unwrap(), parse_params(render).unwrap()).unwrap();
        let pw = params_wgsl(&specs);
        (assemble_rule(rule, &pw), assemble_render(render, &pw))
    }

    /// Uploads the shaders' default param values, as the app does after a successful apply.
    fn upload_default_params(sim: &mut Simulation, rule: &str, render: &str) {
        let specs = merge_params(parse_params(rule).unwrap(), parse_params(render).unwrap()).unwrap();
        sim.set_params(pack_params(&specs, &std::collections::BTreeMap::new()));
    }

    /// Runs `n` steps, submits, and returns any validation error the GPU backend reported.
    fn step_and_check(sim: &mut Simulation, n: u32) -> Option<wgpu::Error> {
        let scope = sim.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = sim.device.create_command_encoder(&Default::default());
        sim.step(&mut enc, n);
        sim.queue.submit([enc.finish()]);
        let _ = sim.device.poll(wgpu::PollType::wait_indefinitely());
        pollster::block_on(scope.pop())
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn every_builtin_compiles_on_the_real_backend_and_steps() {
        let _gpu = gpu_lock();
        for b in BUILTINS {
            let p = load_builtin(b);
            let (rule, render) = assembled_for(&p.rule, &p.render);
            let Some(mut sim) = sim_with(config(p.meta.mode, 64, 32)) else { return };
            sim.set_pipelines(&rule, &render).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
            upload_default_params(&mut sim, &p.rule, &p.render);
            assert!(sim.has_pipelines());
            assert!(step_and_check(&mut sim, 40).is_none(), "{}: validation error while stepping", b.id);
            assert_eq!(sim.frame(), 40);
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn bad_shader_is_rejected_and_both_old_pipelines_stay() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::TwoD, 32, 32)) else { return };
        sim.set_pipelines(&rule, &render).unwrap();
        let (bad_rule, _) = assembled_for("fn rule(pos: vec2<u32>) -> vec4<f32> { return bogus(; }", &p.render);
        let errs = sim.set_pipelines(&bad_rule, &render).unwrap_err();
        assert_eq!((errs[0].file, errs[0].line), (ShaderFile::Rule, 1));
        let (_, bad_render) = assembled_for(&p.rule, "fn shade(uv: vec2<f32>, cell: vec4<f32>) -> vec4<f32> { return 1.0; }");
        let errs = sim.set_pipelines(&rule, &bad_render).unwrap_err();
        assert_eq!(errs[0].file, ShaderFile::Render);
        assert!(sim.has_pipelines(), "a failed build must not remove the live pipelines");
        assert!(step_and_check(&mut sim, 3).is_none());
    }

    /// Reads a whole `rgba32float` texture back to the CPU (width * 16 bytes must be a multiple of 256).
    fn read_back(sim: &Simulation, which: usize) -> Vec<f32> {
        let (w, h) = (sim.config.width, sim.config.height);
        let bytes = (w * h * 16) as u64;
        let buf = sim.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = sim.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &sim.textures.tex[which], mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 16), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        sim.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        let _ = sim.device.poll(wgpu::PollType::wait_indefinitely());
        let view = slice.get_mapped_range().expect("buffer mapped");
        let data: Vec<f32> = bytemuck::cast_slice(&view[..]).to_vec();
        data
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn reset_leaves_no_stale_rows_in_the_second_texture() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[0]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::OneD, 16, 4)) else { return };
        sim.set_pipelines(&rule, &render).unwrap();
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

    #[test]
    #[ignore = "needs a GPU"]
    fn one_d_steps_past_the_bottom_and_with_height_one_without_validation_errors() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[0]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        for h in [1u32, 2, 5] {
            let Some(mut sim) = sim_with(config(Mode::OneD, 16, h)) else { return };
            sim.set_pipelines(&rule, &render).unwrap();
            assert!(step_and_check(&mut sim, h + 7).is_none(), "height {h}: validation error");
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn lifelike_template_runs_and_keeps_live_cells() {
        let _gpu = gpu_lock();
        let t = load_builtin(&crate::preset::builtin::TEMPLATES[1]);
        let (rule, render) = assembled_for(&t.rule, &t.render);
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 64,
            height: 64,
            init: InitPattern::Random { density: 0.4 },
            seed: 3,
        }) else {
            return;
        };
        sim.set_pipelines(&rule, &render).unwrap();
        upload_default_params(&mut sim, &t.rule, &t.render);
        assert!(step_and_check(&mut sim, 10).is_none());
        let live = read_back(&sim, sim.cur).iter().step_by(4).filter(|&&r| r > 0.5).count();
        assert!(live > 0, "B3/S23 from a 40% random soup should still have live cells after 10 steps");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn export_produces_a_decodable_png_with_live_pixels() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 16,
            height: 16,
            init: InitPattern::Random { density: 0.5 },
            seed: 11,
        }) else {
            return;
        };
        sim.set_pipelines(&rule, &render).unwrap();
        upload_default_params(&mut sim, &p.rule, &p.render);
        assert!(step_and_check(&mut sim, 2).is_none());
        sim.start_export(2, "life.png".into()).unwrap();
        assert!(sim.export_pending());
        let mut result = None;
        for _ in 0..100 {
            let _ = sim.device.poll(wgpu::PollType::wait_indefinitely());
            if let Some(r) = sim.poll_export() {
                result = Some(r);
                break;
            }
        }
        let img = result.expect("export finished").expect("export ok");
        assert_eq!(img.filename, "life.png");
        let decoder = png::Decoder::new(std::io::Cursor::new(img.png));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (32, 32), "2x scale of a 16x16 grid");
        let lit = buf[..info.buffer_size()].chunks(4).filter(|px| px[0] > 0 || px[1] > 0 || px[2] > 0).count();
        assert!(lit > 0, "some cells must be coloured");
        assert!(!sim.export_pending());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn painting_writes_a_disc_into_the_current_texture() {
        let _gpu = gpu_lock();
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 32,
            height: 32,
            init: InitPattern::Blank,
            seed: 1,
        }) else {
            return;
        };
        let mut enc = sim.device.create_command_encoder(&Default::default());
        sim.paint(&mut enc, &[Stroke { x: 16, y: 16, radius: 3.0, value: [1.0, 0.0, 0.0, 1.0] }]);
        sim.queue.submit([enc.finish()]);
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
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 32,
            height: 32,
            init: InitPattern::Blank,
            seed: 1,
        }) else {
            return;
        };
        // Paint a disc into the current texture; the other texture stays blank, so every
        // painted cell also counts as changed.
        let mut enc = sim.device.create_command_encoder(&Default::default());
        sim.paint(&mut enc, &[Stroke { x: 16, y: 16, radius: 3.0, value: [1.0, 0.0, 0.0, 1.0] }]);
        sim.collect_stats(&mut enc);
        sim.queue.submit([enc.finish()]);
        let painted = read_back(&sim, sim.cur).iter().step_by(4).filter(|&&r| r > 0.5).count() as u32;
        let mut samples = Vec::new();
        for _ in 0..50 {
            samples.extend(sim.poll_stats());
            if !samples.is_empty() {
                break;
            }
            let _ = sim.device.poll(wgpu::PollType::wait_indefinitely());
        }
        let s = samples.first().expect("a stats sample");
        assert_eq!(s.population, painted);
        assert_eq!(s.changed, painted);
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn load_state_round_trips_image_cells_into_both_textures() {
        use crate::sim::seed_image::{image_to_cells, RgbaImage, SeedMode};
        let _gpu = gpu_lock();
        let Some(mut sim) = sim_with(SimConfig {
            mode: Mode::TwoD,
            width: 16,
            height: 16,
            init: InitPattern::Blank,
            seed: 1,
        }) else {
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
    fn reconfigure_clamps_and_resizes() {
        let _gpu = gpu_lock();
        let Some(mut sim) = sim_with(config(Mode::TwoD, 8, 8)) else { return };
        let huge = sim.device.limits().max_texture_dimension_2d.saturating_mul(2);
        sim.reconfigure(config(Mode::TwoD, huge, 4));
        assert!(sim.config().width <= sim.device.limits().max_texture_dimension_2d);
        assert_eq!(sim.config().height, 4);
        assert_eq!(sim.frame(), 0);
    }

    fn read_globals(sim: &Simulation) -> Globals {
        let buf = sim.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = sim.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&sim.globals_buf, 0, &buf, 0, std::mem::size_of::<Globals>() as u64);
        sim.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        let _ = sim.device.poll(wgpu::PollType::wait_indefinitely());
        let view = slice.get_mapped_range().expect("buffer mapped");
        *bytemuck::from_bytes::<Globals>(&view[..])
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn time_reaches_the_gpu_while_paused_and_frame_counts_match_after_steps() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::TwoD, 16, 16)) else { return };
        sim.set_pipelines(&rule, &render).unwrap();
        sim.set_time(5.5);
        assert!(step_and_check(&mut sim, 0).is_none());
        assert_eq!(read_globals(&sim).time, 5.5, "paused: time must still be uploaded");
        assert!(step_and_check(&mut sim, 7).is_none());
        let g = read_globals(&sim);
        assert_eq!(g.frame, 6, "the buffer holds the globals of the last recorded step");
        assert_eq!(sim.frame(), 7);
    }
}

//! GPU simulation: two ping-pong `rgba32float` textures, a compute pipeline built from the
//! user's rule shader and a render pipeline built from the user's render shader.

use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::preset::{InitPattern, Mode};
use crate::shader::assemble::Assembled;
use crate::shader::params::MAX_PARAMS;
use crate::shader::validate::{validate, ShaderError, ShaderFile};
use crate::sim::init::generate_init;
use crate::sim::row::{clamp_size, plan_row, WORKGROUP};
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
}

pub struct Simulation {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target_format: wgpu::TextureFormat,
    config: SimConfig,
    compute_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
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
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
            &globals_buf,
            &params_buf,
            config.width,
            config.height,
        );
        let mut sim = Simulation {
            device,
            queue,
            target_format,
            config,
            compute_layout,
            render_layout,
            compute_pipeline_layout,
            render_pipeline_layout,
            globals_buf,
            params_buf,
            textures,
            cur: 0,
            compute: None,
            render: None,
            globals,
        };
        sim.reset();
        sim
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
                &self.globals_buf,
                &self.params_buf,
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
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.textures.tex[0],
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

    pub fn set_params(&mut self, data: ParamsData) {
        self.queue.write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&data));
    }

    pub fn set_time(&mut self, seconds: f32) {
        self.globals.time = seconds;
    }

    /// Records a copy of the current `Globals` into the uniform buffer *inside* the encoder, so
    /// that several steps recorded into one submission each see their own globals.
    fn upload_globals_in_encoder(&self, encoder: &mut wgpu::CommandEncoder) {
        let staging = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ca globals staging"),
            contents: bytemuck::bytes_of(&self.globals),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(
            &staging,
            0,
            &self.globals_buf,
            0,
            std::mem::size_of::<Globals>() as u64,
        );
    }

    fn create_module(
        &self,
        file: ShaderFile,
        assembled: &Assembled,
    ) -> Result<wgpu::ShaderModule, Vec<ShaderError>> {
        validate(file, assembled)?;
        Ok(self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(file.label()),
            source: wgpu::ShaderSource::Wgsl(assembled.source.as_str().into()),
        }))
    }

    fn backend_error(file: ShaderFile, err: Option<wgpu::Error>) -> Result<(), Vec<ShaderError>> {
        match err {
            None => Ok(()),
            Some(e) => Err(vec![ShaderError {
                file,
                line: 1,
                column: 1,
                message: format!("GPU backend rejected shader: {e}"),
            }]),
        }
    }

    pub fn set_rule(
        &mut self,
        file: ShaderFile,
        assembled: &Assembled,
    ) -> Result<(), Vec<ShaderError>> {
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
        Self::backend_error(file, pollster::block_on(scope.pop()))?;
        self.compute = Some(pipeline);
        Ok(())
    }

    pub fn set_render(
        &mut self,
        file: ShaderFile,
        assembled: &Assembled,
    ) -> Result<(), Vec<ShaderError>> {
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
        Self::backend_error(file, pollster::block_on(scope.pop()))?;
        self.render = Some(pipeline);
        Ok(())
    }

    /// Records `n` simulation steps into `encoder`.
    pub fn step(&mut self, encoder: &mut wgpu::CommandEncoder, n: u32) {
        let Some(pipeline) = &self.compute else { return };
        let (w, h) = (self.config.width, self.config.height);
        for _ in 0..n {
            let src = self.cur;
            let dst = 1 - self.cur;
            match self.config.mode {
                Mode::TwoD => {
                    self.upload_globals_in_encoder(encoder);
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("ca step"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &self.textures.compute_bind_groups[src], &[]);
                    pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1);
                }
                Mode::OneD => {
                    let plan = plan_row(self.globals.row, h);
                    // Carry the unchanged rows from src to dst so dst holds the full diagram.
                    if plan.scroll {
                        copy_rows(encoder, &self.textures.tex[src], 1, &self.textures.tex[dst], 0, w, h - 1);
                    } else {
                        copy_rows(encoder, &self.textures.tex[src], 0, &self.textures.tex[dst], 0, w, plan.write_row);
                    }
                    self.globals.row = plan.write_row;
                    self.globals.prev_row = plan.read_row;
                    self.upload_globals_in_encoder(encoder);
                    {
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("ca step 1d"),
                            timestamp_writes: None,
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &self.textures.compute_bind_groups[src], &[]);
                        pass.dispatch_workgroups(w.div_ceil(WORKGROUP), 1, 1);
                    }
                    self.globals.row = plan.next_row;
                }
            }
            self.cur = dst;
            self.globals.frame = self.globals.frame.wrapping_add(1);
        }
    }

    /// Draws the current state with the render pipeline into the active render pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some(pipeline) = &self.render else { return };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.textures.render_bind_groups[self.cur], &[]);
        pass.draw(0..3, 0..1);
    }
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

fn create_textures(
    device: &wgpu::Device,
    compute_layout: &wgpu::BindGroupLayout,
    render_layout: &wgpu::BindGroupLayout,
    globals_buf: &wgpu::Buffer,
    params_buf: &wgpu::Buffer,
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
    let compute_bind_groups = [compute_bg(0), compute_bg(1)];
    let render_bind_groups = [render_bg(0), render_bg(1)];
    Textures { tex, compute_bind_groups, render_bind_groups }
}

/// GPU tests: need a real adapter, so they are `#[ignore]`d for CI. Run with
/// `cargo test gpu_tests -- --ignored`.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::preset::builtin::{load_builtin, BUILTINS};
    use crate::shader::assemble::{assemble_render, assemble_rule};
    use crate::shader::params::{merge_params, params_wgsl, parse_params};

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
            sim.set_rule(ShaderFile::Rule, &rule).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
            sim.set_render(ShaderFile::Render, &render).unwrap_or_else(|e| panic!("{}: {:?}", b.id, e));
            assert!(sim.has_pipelines());
            assert!(step_and_check(&mut sim, 40).is_none(), "{}: validation error while stepping", b.id);
            assert_eq!(sim.frame(), 40);
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn bad_rule_is_rejected_and_old_pipeline_stays() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[2]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        let Some(mut sim) = sim_with(config(Mode::TwoD, 32, 32)) else { return };
        sim.set_rule(ShaderFile::Rule, &rule).unwrap();
        sim.set_render(ShaderFile::Render, &render).unwrap();
        let (bad, _) = assembled_for("fn rule(pos: vec2<u32>) -> vec4<f32> { return bogus(; }", &p.render);
        let errs = sim.set_rule(ShaderFile::Rule, &bad).unwrap_err();
        assert_eq!(errs[0].line, 1);
        assert!(sim.has_pipelines(), "a failed build must not remove the live pipeline");
        assert!(step_and_check(&mut sim, 3).is_none());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn one_d_steps_past_the_bottom_and_with_height_one_without_validation_errors() {
        let _gpu = gpu_lock();
        let p = load_builtin(&BUILTINS[0]);
        let (rule, render) = assembled_for(&p.rule, &p.render);
        for h in [1u32, 2, 5] {
            let Some(mut sim) = sim_with(config(Mode::OneD, 16, h)) else { return };
            sim.set_rule(ShaderFile::Rule, &rule).unwrap();
            sim.set_render(ShaderFile::Render, &render).unwrap();
            assert!(step_and_check(&mut sim, h + 7).is_none(), "height {h}: validation error");
        }
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
}

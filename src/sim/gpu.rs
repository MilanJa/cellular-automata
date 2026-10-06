//! GPU objects that every simulation on a device shares: bind group and pipeline layouts, the
//! fixed paint / statistics / blit pipelines, the scene sampler, the empty "no layer B" texture
//! and the device-lost flag. Created once per device; `Simulation`s hold it by `Arc`.

use std::sync::{Arc, Mutex};

use eframe::wgpu;

use crate::shader::assemble::BLIT_WGSL;
use crate::sim::paint::PAINT_WGSL;
use crate::sim::stats::STATS_WGSL;
use crate::util::lock;

/// Intermediate picture format: filterable, high range for bloom and feedback.
pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
    pub(crate) compute_layout: wgpu::BindGroupLayout,
    pub(crate) render_layout: wgpu::BindGroupLayout,
    pub(crate) paint_layout: wgpu::BindGroupLayout,
    pub(crate) stats_layout: wgpu::BindGroupLayout,
    pub(crate) post_layout: wgpu::BindGroupLayout,
    pub(crate) blit_layout: wgpu::BindGroupLayout,
    pub(crate) compute_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) render_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) post_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) paint_pipeline: wgpu::ComputePipeline,
    pub(crate) stats_pipeline: wgpu::ComputePipeline,
    pub(crate) blit_pipeline: wgpu::RenderPipeline,
    pub(crate) sampler: wgpu::Sampler,
    /// What `other()` reads when there is no layer B: a 1x1 zero texture.
    pub(crate) empty_other_view: wgpu::TextureView,
    /// Set by wgpu's device-lost callback; drained by the app once per frame.
    device_lost: Arc<Mutex<Option<String>>>,
}

impl GpuContext {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat) -> Arc<Self> {
        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let sampled = |binding: u32, visibility: wgpu::ShaderStages, filterable: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let storage_tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba32Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let filtering_sampler = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
        };
        let pipeline_layout = |label: &str, bgl: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(bgl)],
                immediate_size: 0,
            })
        };
        let compute_module = |label: &str, source: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let compute_pipeline = |label: &str, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        let compute_layout = layout(
            "ca compute layout",
            &[
                sampled(0, wgpu::ShaderStages::COMPUTE, false),
                storage_tex(1),
                uniform(2),
                uniform(3),
                sampled(4, wgpu::ShaderStages::COMPUTE, false),
            ],
        );
        let render_layout = layout(
            "ca render layout",
            &[
                sampled(0, wgpu::ShaderStages::FRAGMENT, false),
                uniform(2),
                uniform(3),
                sampled(4, wgpu::ShaderStages::FRAGMENT, false),
            ],
        );
        let paint_layout = layout("ca paint layout", &[storage_tex(0), uniform(1)]);
        let stats_layout = layout(
            "ca stats layout",
            &[
                sampled(0, wgpu::ShaderStages::COMPUTE, false),
                sampled(1, wgpu::ShaderStages::COMPUTE, false),
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
        );
        let post_layout = layout(
            "ca post layout",
            &[
                sampled(0, wgpu::ShaderStages::FRAGMENT, true),
                filtering_sampler(1),
                uniform(2),
                uniform(3),
                sampled(4, wgpu::ShaderStages::FRAGMENT, true),
            ],
        );
        let blit_layout =
            layout("ca blit layout", &[sampled(0, wgpu::ShaderStages::FRAGMENT, true), filtering_sampler(1)]);

        let compute_pipeline_layout = pipeline_layout("ca compute pipeline layout", &compute_layout);
        let render_pipeline_layout = pipeline_layout("ca render pipeline layout", &render_layout);
        let post_pipeline_layout = pipeline_layout("ca post pipeline layout", &post_layout);
        let paint_pipeline_layout = pipeline_layout("ca paint pipeline layout", &paint_layout);
        let stats_pipeline_layout = pipeline_layout("ca stats pipeline layout", &stats_layout);
        let blit_pipeline_layout = pipeline_layout("ca blit pipeline layout", &blit_layout);

        let paint_pipeline =
            compute_pipeline("ca paint pipeline", &paint_pipeline_layout, &compute_module("ca paint", PAINT_WGSL));
        let stats_pipeline =
            compute_pipeline("ca stats pipeline", &stats_pipeline_layout, &compute_module("ca stats", STATS_WGSL));
        let blit_module = compute_module("ca blit", BLIT_WGSL);
        let blit_pipeline = fullscreen_pipeline(&device, "ca blit", &blit_pipeline_layout, &blit_module, target_format);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ca scene sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let empty_other = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ca empty other"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &empty_other,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::bytes_of(&[0.0f32; 4]),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(16), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let empty_other_view = empty_other.create_view(&Default::default());

        // Anything the error scopes miss is logged rather than aborting the process.
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
            log::error!("uncaptured wgpu error: {e}");
        }));
        // One callback per device: installing it here (not per simulation) means the flag the
        // app polls is the one that gets set, however many layers or thumbnails exist.
        let device_lost: Arc<Mutex<Option<String>>> = Default::default();
        {
            let flag = device_lost.clone();
            device.set_device_lost_callback(move |reason, message| {
                let text = format!("GPU device lost ({reason:?}): {message}");
                log::error!("{text}");
                *lock(&flag) = Some(text);
            });
        }

        Arc::new(GpuContext {
            device,
            queue,
            target_format,
            compute_layout,
            render_layout,
            paint_layout,
            stats_layout,
            post_layout,
            blit_layout,
            compute_pipeline_layout,
            render_pipeline_layout,
            post_pipeline_layout,
            paint_pipeline,
            stats_pipeline,
            blit_pipeline,
            sampler,
            empty_other_view,
            device_lost,
        })
    }

    /// Returns the device-lost message once, if the device was lost since the last call.
    pub fn take_device_lost(&self) -> Option<String> {
        lock(&self.device_lost).take()
    }
}

/// A fullscreen-triangle pipeline (`vs_main` / `fs_main`) writing opaque colour to `format`.
pub(crate) fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

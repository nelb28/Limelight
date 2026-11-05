use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};

pub const WAVEFORM_POINTS: u32 = 2048;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct WaveformParams {
    num_points: u32,
    amplitude_scale: f32,
    _pad0: u32,
    _pad1: u32,
}

pub struct WaveformPipeline {
    pipeline: wgpu::RenderPipeline,
    params_buf: wgpu::Buffer,
    samples_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl WaveformPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("waveform shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("waveform.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("waveform bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("waveform pipeline layout"),
            bind_group_layouts: &[&bgl],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("waveform pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("waveform params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let samples_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("waveform samples"),
            size: (WAVEFORM_POINTS as u64 * 4).max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("waveform bind group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: samples_buf.as_entire_binding(),
                },
            ],
        });

        Self {
            pipeline,
            params_buf,
            samples_buf,
            bind_group,
        }
    }

    /// Upload a window of audio samples (already downsampled to WAVEFORM_POINTS).
    pub fn update(&self, queue: &wgpu::Queue, samples: &[f32]) {
        let params = WaveformParams {
            num_points: WAVEFORM_POINTS,
            amplitude_scale: 0.30, // use 30% of NDC height
            _pad0: 0,
            _pad1: 0,
        };
        queue.write_buffer(&self.params_buf, 0, bytes_of(&params));
        queue.write_buffer(&self.samples_buf, 0, cast_slice(samples));
    }

    pub fn draw<'rp>(&'rp self, rpass: &mut wgpu::RenderPass<'rp>) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.draw(0..WAVEFORM_POINTS, 0..1);
    }
}

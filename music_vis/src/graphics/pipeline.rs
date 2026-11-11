use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Params {
    num_bars: u32,
    global_max: f32,
    max_height: f32,      // NDC height of the bar area (bars start at y = -1)
    reference_frac: f32,  // fraction of max_height reached at the reference level
    pitch_offset: f32,    // semitones above C of bin 0 (A4 = 440 Hz)
    bins_per_octave: f32,
    color_mode: u32,      // 0 = by position (blue -> red), 1 = by note
    _pad: u32,
}

pub struct BarPipeline {
    pipeline: wgpu::RenderPipeline,
    params_buf: wgpu::Buffer,
    magnitude_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    num_bars: u32,
    pitch_offset: f32,
    bins_per_octave: f32,
    color_mode: u32,
}

impl BarPipeline {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        num_bars: u32,
        (pitch_offset, bins_per_octave): (f32, f32),
        colors: super::ColorMode,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bars shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bars.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bars bgl"),
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
            label: Some("bars pipeline layout"),
            bind_group_layouts: &[&bgl],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bars pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bars params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let magnitude_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bars magnitudes"),
            size: (num_bars as u64 * 4).max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bars bind group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: magnitude_buf.as_entire_binding(),
                },
            ],
        });

        Self {
            pipeline,
            params_buf,
            magnitude_buf,
            bind_group,
            num_bars,
            pitch_offset,
            bins_per_octave,
            color_mode: colors as u32,
        }
    }

    pub fn update(&self, queue: &wgpu::Queue, magnitudes: &[f32], global_max: f32, layout: &super::Layout) {
        let params = Params {
            num_bars: self.num_bars,
            global_max,
            max_height: layout.bar_height(),
            reference_frac: super::layout::BAR_REFERENCE_FRACTION,
            pitch_offset: self.pitch_offset,
            bins_per_octave: self.bins_per_octave,
            color_mode: self.color_mode,
            _pad: 0,
        };
        queue.write_buffer(&self.params_buf, 0, bytes_of(&params));
        queue.write_buffer(&self.magnitude_buf, 0, cast_slice(magnitudes));
    }

    pub fn draw<'rp>(&'rp self, rpass: &mut wgpu::RenderPass<'rp>) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        // One quad per segment between neighboring bins (a continuous filled area).
        rpass.draw(0..6, 0..self.num_bars.saturating_sub(1));
    }
}

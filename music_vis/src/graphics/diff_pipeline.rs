use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};

/// Uniforms for `diff.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Params {
    num_bins: u32,
    max_height: f32,
    _pad0: u32,
    _pad1: u32,
}

/// Draws, on a comparison panel's bar area, where its bar graph differs from the
/// reference (leftmost) panel's: white where it is higher, black where it is lower.
pub struct DiffPipeline {
    pipeline: wgpu::RenderPipeline,
    params_buf: wgpu::Buffer,
    heights_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    num_bins: u32,
}

impl DiffPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, num_bins: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diff shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("diff.wgsl").into()),
        });

        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("diff bgl"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Uniform),
                entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("diff pipeline layout"),
            bind_group_layouts: &[&bgl],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("diff pipeline"),
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
            label: Some("diff params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let heights_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("diff heights"),
            size: (num_bins as u64 * 8).max(8),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diff bind group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: heights_buf.as_entire_binding() },
            ],
        });

        Self { pipeline, params_buf, heights_buf, bind_group, num_bins }
    }

    /// `heights` = (reference, this panel) per bin, as fractions of the bar area
    /// (see `layout::bar_fraction`).
    pub fn update(&self, queue: &wgpu::Queue, heights: &[[f32; 2]], layout: &super::Layout) {
        let params = Params {
            num_bins: self.num_bins,
            max_height: layout.bar_height(),
            _pad0: 0,
            _pad1: 0,
        };
        queue.write_buffer(&self.params_buf, 0, bytes_of(&params));
        queue.write_buffer(&self.heights_buf, 0, cast_slice(heights));
    }

    pub fn draw<'rp>(&'rp self, rpass: &mut wgpu::RenderPass<'rp>) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.draw(0..6, 0..self.num_bins.saturating_sub(1));
    }
}

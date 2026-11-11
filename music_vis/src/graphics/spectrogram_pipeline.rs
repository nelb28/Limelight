use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::cwt_file::CwtFile;

/// Seconds of history and of lookahead shown on each side of the playhead.
pub const HALF_WINDOW_SECS: f32 = 3.0;

/// Screen layout in NDC y (bottom, top). Bars fill from y = -1 up to `BARS_TOP`.
pub const PHASE_REGION: (f32, f32) = (0.45, 1.0);
pub const MAGNITUDE_REGION: (f32, f32) = (-0.15, 0.40);
pub const BARS_TOP: f32 = -0.20;

/// Magnitudes this many dB below the file's max are drawn black.
const DB_RANGE: f32 = 50.0;
/// Pitch deviation (cents) that maps to fully blue / red. 50 cents = a quarter tone.
const DEV_RANGE: f32 = 50.0;

const MODE_MAGNITUDE: u32 = 0;
const MODE_PHASE: u32 = 1;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Params {
    center_frame: f32,
    half_window_frames: f32,
    num_frames: u32,
    num_scales: u32,
    global_max: f32,
    y_bottom: f32,
    y_top: f32,
    screen_width: f32,
    db_range: f32,
    dev_range: f32,
    mode: u32,
    has_phase: u32,
}

/// One spectrogram view: its own region and color mode, sharing the data buffers.
struct View {
    params: Params,
    params_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// Draws the magnitude spectrogram and, above it, the phase-deviation spectrogram.
/// The whole CWT is uploaded once; per-frame updates are just the playhead uniform.
pub struct SpectrogramPipeline {
    pipeline: wgpu::RenderPipeline,
    views: Vec<View>,
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn upload_storage(device: &wgpu::Device, label: &str, data: &[f32]) -> wgpu::Buffer {
    let max_binding = device.limits().max_storage_buffer_binding_size as usize;
    assert!(
        data.len() * 4 <= max_binding,
        "{label} ({} MiB) exceeds GPU storage buffer limit ({} MiB)",
        data.len() * 4 >> 20,
        max_binding >> 20
    );
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: if data.is_empty() { &[0u8; 4] } else { cast_slice(data) },
        usage: wgpu::BufferUsages::STORAGE,
    })
}

impl SpectrogramPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, cwt: &CwtFile) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spectrogram shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("spectrogram.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("spectrogram bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1),
                storage_entry(2),
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spectrogram pipeline layout"),
            bind_group_layouts: &[&bgl],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("spectrogram pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
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

        let has_phase = !cwt.phase_dev.is_empty();
        if !has_phase {
            eprintln!("note: CWT file has no phase data (CWT1); re-run cwt to enable the phase graph");
        }
        let mags_buf = upload_storage(device, "spectrogram magnitudes", &cwt.data);
        let phase_buf = upload_storage(device, "spectrogram phase deviation", &cwt.phase_dev);

        let global_max = cwt.global_max();
        let base = Params {
            center_frame: 0.0,
            half_window_frames: HALF_WINDOW_SECS * cwt.header.frame_rate as f32,
            num_frames: cwt.header.num_frames,
            num_scales: cwt.header.num_scales,
            global_max: if global_max > 0.0 { global_max } else { 1.0 },
            y_bottom: 0.0,
            y_top: 0.0,
            screen_width: 1.0,
            db_range: DB_RANGE,
            dev_range: DEV_RANGE,
            mode: MODE_MAGNITUDE,
            has_phase: has_phase as u32,
        };

        let views = [(MODE_MAGNITUDE, MAGNITUDE_REGION), (MODE_PHASE, PHASE_REGION)]
            .into_iter()
            .map(|(mode, (y_bottom, y_top))| {
                let params = Params { mode, y_bottom, y_top, ..base };
                let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("spectrogram params"),
                    contents: bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("spectrogram bind group"),
                    layout: &bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: mags_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: phase_buf.as_entire_binding() },
                    ],
                });
                View { params, params_buf, bind_group }
            })
            .collect();

        Self { pipeline, views }
    }

    /// Move both views to the fractional CWT frame at the playhead.
    pub fn update(&mut self, queue: &wgpu::Queue, center_frame: f32, screen_width: u32) {
        for view in &mut self.views {
            view.params.center_frame = center_frame;
            view.params.screen_width = screen_width as f32;
            queue.write_buffer(&view.params_buf, 0, bytes_of(&view.params));
        }
    }

    pub fn draw<'rp>(&'rp self, rpass: &mut wgpu::RenderPass<'rp>) {
        rpass.set_pipeline(&self.pipeline);
        for view in &self.views {
            rpass.set_bind_group(0, &view.bind_group, &[]);
            rpass.draw(0..6, 0..1);
        }
    }
}

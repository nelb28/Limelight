use bytemuck::{bytes_of, cast_slice, Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::Layout;
use crate::cwt_file::CwtFile;

/// Seconds of history and of lookahead shown on each side of the playhead.
pub const HALF_WINDOW_SECS: f32 = 3.0;


/// How magnitudes map to color intensity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleMode {
    Db = 0,
    Linear = 1,
    Sqrt = 2,
}

/// How the bars are colored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorMode {
    /// Blue (low) to red (high) by position.
    Classic = 0,
    /// Hue by musical note (pitch class): every C the same hue, every C# the next...
    Note = 1,
}

/// Display settings chosen on the command line.
#[derive(Debug, Clone)]
pub struct DisplayOptions {
    pub scale: ScaleMode,
    /// Magnitudes this many dB below the max are drawn black (Db scale only).
    pub db_range: f32,
    /// Pitch deviation (cents) that maps to fully blue / red. 50 cents = a quarter tone.
    pub dev_range: f32,
    /// Percentile of magnitudes mapped to full bar height / color (100 = max).
    pub peak_pct: f32,
    /// Bar colors (default: by note).
    pub bar_colors: ColorMode,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            scale: ScaleMode::Db,
            db_range: 50.0,
            dev_range: 50.0,
            peak_pct: 100.0,
            bar_colors: ColorMode::Note,
        }
    }
}

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
    center_px: f32,
    db_range: f32,
    dev_range: f32,
    mode: u32,
    has_phase: u32,
    scale_mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        cwt: &CwtFile,
        display: &DisplayOptions,
        layout: &Layout,
    ) -> Self {
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
        if layout.phase.is_some() && !has_phase {
            eprintln!("note: CWT file has no phase data (CWT1); re-run cwt to enable the phase graph");
        }
        let mags_buf = upload_storage(device, "spectrogram magnitudes", &cwt.data);
        // Phase data only goes to the GPU when the phase graph is shown.
        let phase_data: &[f32] = if layout.phase.is_some() { &cwt.phase_dev } else { &[] };
        let phase_buf = upload_storage(device, "spectrogram phase deviation", phase_data);

        let global_max = cwt.global_max();
        let base = Params {
            center_frame: 0.0,
            half_window_frames: HALF_WINDOW_SECS * cwt.header.frame_rate as f32,
            num_frames: cwt.header.num_frames,
            num_scales: cwt.header.num_scales,
            global_max: if global_max > 0.0 { global_max } else { 1.0 },
            y_bottom: 0.0,
            y_top: 0.0,
            center_px: 0.0,
            db_range: display.db_range,
            dev_range: display.dev_range,
            mode: MODE_MAGNITUDE,
            has_phase: has_phase as u32,
            scale_mode: display.scale as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };

        let mut regions = vec![(MODE_MAGNITUDE, layout.magnitude)];
        if let Some(phase) = layout.phase {
            regions.push((MODE_PHASE, phase));
        }
        let views = regions
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

    /// Move the views to the fractional CWT frame at the playhead. `center_px` is the
    /// x pixel (framebuffer coordinates) of the panel's center, for the playhead marker.
    pub fn update(&mut self, queue: &wgpu::Queue, center_frame: f32, center_px: f32) {
        for view in &mut self.views {
            view.params.center_frame = center_frame;
            view.params.center_px = center_px;
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

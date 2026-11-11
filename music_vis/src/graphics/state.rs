use std::sync::Arc;
use winit::window::Window;

use super::{BarPipeline, DiffPipeline, DisplayOptions, Layout, SpectrogramPipeline};
use crate::cwt_file::CwtFile;

/// Pixels of background between side-by-side panels.
const PANEL_GAP_PX: u32 = 8;

/// One side-by-side view of the song: its own bars and spectrogram(s).
struct Panel {
    bars: BarPipeline,
    spectrogram: SpectrogramPipeline,
    /// Difference vs. the leftmost panel's bars (every panel except the first).
    diff: Option<DiffPipeline>,
}

pub struct State {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    layout: Layout,
    panels: Vec<Panel>,
    /// Draw the difference overlays (toggled with D).
    pub show_diff: bool,
}

impl State {
    /// One panel per (data, display options) pair, laid out left to right.
    pub async fn new(window: Arc<Window>, panels: &[(&CwtFile, &DisplayOptions)], layout: Layout) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        ).await.unwrap();

        let (device, queue) = adapter.request_device(
            // Use the adapter's real limits: the whole CWT is uploaded as one storage
            // buffer, which can exceed the conservative 128 MiB default for long songs.
            &wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            },
        ).await.unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let panels = panels
            .iter()
            .enumerate()
            .map(|(i, (cwt, display))| Panel {
                diff: (i > 0).then(|| DiffPipeline::new(&device, format, cwt.header.num_scales)),
                bars: BarPipeline::new(
                    &device,
                    format,
                    cwt.header.num_scales,
                    cwt.pitch_mapping(),
                    display.bar_colors,
                ),
                spectrogram: SpectrogramPipeline::new(&device, format, cwt, display, &layout),
            })
            .collect();

        Self {
            surface,
            device,
            queue,
            config,
            size,
            layout,
            panels,
            show_diff: true,
        }
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.size = size;
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Pixel x offset and width of panel `i`.
    fn panel_rect(&self, i: usize) -> (u32, u32) {
        let n = self.panels.len() as u32;
        let gaps = PANEL_GAP_PX * (n - 1);
        let width = (self.config.width.saturating_sub(gaps) / n).max(1);
        (i as u32 * (width + PANEL_GAP_PX), width)
    }

    /// Upload panel `i`'s difference overlay: (reference, panel i) bar-area fractions per bin.
    pub fn update_diff(&mut self, i: usize, heights: &[[f32; 2]]) {
        if let Some(diff) = &self.panels[i].diff {
            diff.update(&self.queue, heights, &self.layout);
        }
    }

    /// Update panel `i` for the current playhead position.
    pub fn update_panel(&mut self, i: usize, magnitudes: &[f32], global_max: f32, center_frame: f32) {
        let (x, width) = self.panel_rect(i);
        let panel = &mut self.panels[i];
        panel.bars.update(&self.queue, magnitudes, global_max, &self.layout);
        panel.spectrogram.update(&self.queue, center_frame, x as f32 + width as f32 / 2.0);
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&Default::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") }
        );

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.04,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            let height = self.config.height as f32;
            for i in 0..self.panels.len() {
                let (x, width) = self.panel_rect(i);
                rpass.set_viewport(x as f32, 0.0, width as f32, height, 0.0, 1.0);
                self.panels[i].bars.draw(&mut rpass);
                if self.show_diff {
                    if let Some(diff) = &self.panels[i].diff {
                        diff.draw(&mut rpass);
                    }
                }
                self.panels[i].spectrogram.draw(&mut rpass);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
    }
}

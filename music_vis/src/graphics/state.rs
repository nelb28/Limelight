use std::sync::Arc;
use winit::window::Window;

use super::{BarPipeline, SpectrogramPipeline};
use crate::cwt_file::CwtFile;

pub struct State {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    bar_pipeline: BarPipeline,
    spectrogram_pipeline: SpectrogramPipeline,
}

impl State {
    pub async fn new(window: Arc<Window>, cwt: &CwtFile) -> Self {
        let num_bars = cwt.header.num_scales;
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

        let bar_pipeline = BarPipeline::new(&device, format, num_bars);
        let spectrogram_pipeline = SpectrogramPipeline::new(&device, format, cwt);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            bar_pipeline,
            spectrogram_pipeline,
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

    pub fn update_magnitudes(&self, magnitudes: &[f32], global_max: f32) {
        self.bar_pipeline.update(&self.queue, magnitudes, global_max);
    }

    pub fn update_spectrogram(&mut self, center_frame: f32) {
        self.spectrogram_pipeline
            .update(&self.queue, center_frame, self.config.width);
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

            self.bar_pipeline.draw(&mut rpass);
            self.spectrogram_pipeline.draw(&mut rpass);
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
    }
}

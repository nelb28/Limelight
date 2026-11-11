use std::path::PathBuf;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::audio::AudioPlayer;
use crate::cwt_file::CwtFile;
use crate::graphics::State;

pub struct App {
    audio_path: PathBuf,
    cwt: CwtFile,
    window: Option<Arc<Window>>,
    state: Option<State>,
    audio: Option<AudioPlayer>,
}

impl App {
    pub fn new(audio_path: PathBuf, cwt: CwtFile) -> Self {
        Self {
            audio_path,
            cwt,
            window: None,
            state: None,
            audio: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_title("Limelight"))
                    .unwrap(),
            );

            let num_bars = self.cwt.header.num_scales;
            let mut state = pollster::block_on(State::new(window.clone(), &self.cwt));

            // Prime GPU buffers with silence so shaders have valid data.
            let silence_bars = vec![0.0f32; num_bars as usize];
            state.update_magnitudes(&silence_bars, 1.0);
            state.update_spectrogram(0.0);

            let audio = AudioPlayer::new(&self.audio_path);

            self.window = Some(window);
            self.state = Some(state);
            self.audio = Some(audio);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(state) = &mut self.state {
                    state.resize(size);
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Space) => {
                    if let Some(audio) = &self.audio {
                        audio.toggle_pause();
                    }
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    if let Some(audio) = &self.audio {
                        audio.seek_relative(-5.0);
                    }
                }
                Key::Named(NamedKey::ArrowRight) => {
                    if let Some(audio) = &self.audio {
                        audio.seek_relative(5.0);
                    }
                }
                _ => {}
            },

            WindowEvent::RedrawRequested => {
                if let (Some(audio), Some(state)) = (&self.audio, &mut self.state) {
                    // Current fractional CWT frame drives both the bars and the spectrogram.
                    let t = audio.cwt_frame_position(self.cwt.header.frame_rate);
                    let magnitudes = self.cwt.interpolated_frame(t);
                    state.update_magnitudes(&magnitudes, self.cwt.global_max());
                    state.update_spectrogram(t as f32);
                }

                if let Some(state) = &mut self.state {
                    match state.render() {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(_) => {}
                    }
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}

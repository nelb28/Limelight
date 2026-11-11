use std::path::PathBuf;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::audio::AudioPlayer;
use crate::cwt_file::CwtFile;
use crate::graphics::{layout::bar_fraction, DisplayOptions, Layout, State};

/// One side-by-side view: the (filtered) data and how to display it.
pub struct PanelData {
    pub cwt: CwtFile,
    pub display: DisplayOptions,
}

pub struct App {
    audio_path: PathBuf,
    panels: Vec<PanelData>,
    layout: Layout,
    fullscreen: bool,
    window: Option<Arc<Window>>,
    state: Option<State>,
    audio: Option<AudioPlayer>,
}

impl App {
    pub fn new(audio_path: PathBuf, panels: Vec<PanelData>, layout: Layout, fullscreen: bool) -> Self {
        assert!(!panels.is_empty());
        Self {
            audio_path,
            panels,
            layout,
            fullscreen,
            window: None,
            state: None,
            audio: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let mut attrs = Window::default_attributes().with_title("Limelight");
            if self.fullscreen {
                attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
            }
            let window = Arc::new(event_loop.create_window(attrs).unwrap());

            let panel_refs: Vec<(&CwtFile, &DisplayOptions)> =
                self.panels.iter().map(|p| (&p.cwt, &p.display)).collect();
            let mut state = pollster::block_on(State::new(window.clone(), &panel_refs, self.layout));

            // Prime GPU buffers with silence so shaders have valid data.
            for (i, p) in self.panels.iter().enumerate() {
                let n = p.cwt.header.num_scales as usize;
                state.update_panel(i, &vec![0.0f32; n], 1.0, 0.0);
                state.update_diff(i, &vec![[0.0f32; 2]; n]);
            }

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
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Character(ref c) if c.as_str().eq_ignore_ascii_case("d") => {
                    if let Some(state) = &mut self.state {
                        if self.panels.len() > 1 {
                            state.show_diff = !state.show_diff;
                            println!("difference overlay {}", if state.show_diff { "on" } else { "off" });
                        }
                    }
                }
                Key::Named(NamedKey::F11) => {
                    if let Some(window) = &self.window {
                        let next = match window.fullscreen() {
                            Some(_) => None,
                            None => Some(Fullscreen::Borderless(None)),
                        };
                        window.set_fullscreen(next);
                    }
                }
                // Playback speed: 1 = normal, 3 = three quarters, 2 = half (pitch drops too).
                Key::Character(ref c) if matches!(c.as_str(), "1" | "2" | "3") => {
                    let speed = match c.as_str() {
                        "3" => 0.75,
                        "2" => 0.5,
                        _ => 1.0,
                    };
                    if let Some(audio) = &self.audio {
                        if audio.speed() != speed {
                            audio.set_speed(speed);
                            println!("speed {speed}x");
                        }
                    }
                    if let Some(window) = &self.window {
                        let title = if speed == 1.0 {
                            "Limelight".to_string()
                        } else {
                            format!("Limelight - {speed}x speed")
                        };
                        window.set_title(&title);
                    }
                }
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
                    // Current fractional CWT frame drives the bars and spectrograms of
                    // every panel (all panels are the same song, so the same frame rate).
                    let mut reference: Vec<f32> = Vec::new();
                    for (i, p) in self.panels.iter().enumerate() {
                        let t = audio.cwt_frame_position(p.cwt.header.frame_rate);
                        let magnitudes = p.cwt.interpolated_frame(t);
                        state.update_panel(i, &magnitudes, p.cwt.global_max(), t as f32);

                        // Compare bar heights as drawn (each panel has its own scale)
                        // against the leftmost panel.
                        let drawn: Vec<f32> =
                            magnitudes.iter().map(|&m| bar_fraction(m, p.cwt.global_max())).collect();
                        if i == 0 {
                            reference = drawn;
                        } else if state.show_diff {
                            let pairs: Vec<[f32; 2]> =
                                reference.iter().zip(&drawn).map(|(&r, &d)| [r, d]).collect();
                            state.update_diff(i, &pairs);
                        }
                    }
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

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use rodio::{Decoder, Source};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::audio::AudioPlayer;
use crate::cwt_file::CwtFile;
use crate::graphics::{State, WAVEFORM_POINTS};

/// Decode an audio file to a mono f32 sample buffer.
fn decode_to_mono(path: &std::path::Path) -> Vec<f32> {
    let file = File::open(path).expect("Failed to open audio for waveform");
    let decoder = Decoder::new(BufReader::new(file)).expect("Failed to decode audio for waveform");
    let channels = decoder.channels().get() as usize;
    let raw: Vec<f32> = decoder.collect();
    // Downmix interleaved stereo (or more) to mono by averaging channels.
    raw.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Extract a window of `window_samples` samples centered on `center`, then
/// downsample to exactly `n` evenly-spaced points via mean-bucketing.
fn extract_waveform_window(
    audio: &[f32],
    center: usize,
    window_samples: usize,
    n: usize,
) -> Vec<f32> {
    let half = window_samples / 2;
    let start = center.saturating_sub(half);
    let end = (start + window_samples).min(audio.len());
    let window = &audio[start..end];

    if window.is_empty() {
        return vec![0.0; n];
    }

    // Mean-bucket downsample from window.len() → n points.
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let lo = i * window.len() / n;
        let hi = ((i + 1) * window.len() / n).max(lo + 1).min(window.len());
        let mean = window[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
        out.push(mean);
    }
    out
}

pub struct App {
    audio_path: PathBuf,
    cwt: CwtFile,
    audio_mono: Vec<f32>, // full decoded mono buffer for waveform display
    window: Option<Arc<Window>>,
    state: Option<State>,
    audio: Option<AudioPlayer>,
}

impl App {
    pub fn new(audio_path: PathBuf, cwt: CwtFile) -> Self {
        println!("Decoding audio for waveform display…");
        let audio_mono = decode_to_mono(&audio_path);
        println!("Decoded {} mono samples.", audio_mono.len());
        Self {
            audio_path,
            cwt,
            audio_mono,
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
            let state = pollster::block_on(State::new(window.clone(), num_bars));

            // Prime GPU buffers with silence so shaders have valid data.
            let silence_bars = vec![0.0f32; num_bars as usize];
            state.update_magnitudes(&silence_bars, 1.0);
            let silence_wave = vec![0.0f32; WAVEFORM_POINTS as usize];
            state.update_waveform(&silence_wave);

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
                if let (Some(audio), Some(state)) = (&self.audio, &self.state) {
                    let pos_secs = audio.position_secs();
                    let sr = self.cwt.header.sample_rate as usize;

                    // Update CWT bar magnitudes.
                    let t = audio.cwt_frame_position(self.cwt.header.frame_rate);
                    let magnitudes = self.cwt.interpolated_frame(t);
                    state.update_magnitudes(&magnitudes, self.cwt.global_max());

                    // Update waveform: 2-second window centered on current position.
                    let center = (pos_secs * sr as f64) as usize;
                    let window_samples = sr * 2;
                    let waveform = extract_waveform_window(
                        &self.audio_mono,
                        center,
                        window_samples,
                        WAVEFORM_POINTS as usize,
                    );
                    state.update_waveform(&waveform);
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

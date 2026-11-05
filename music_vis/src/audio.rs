use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

pub struct AudioPlayer {
    player: Player,
    _sink: MixerDeviceSink,
}

impl AudioPlayer {
    pub fn new(path: &Path) -> Self {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .expect("Failed to open audio output");
        sink.log_on_drop(false);

        let mixer = sink.mixer().clone();
        let file = File::open(path).expect("Failed to open audio file");
        let reader = BufReader::new(file);
        let source = Decoder::new(reader).expect("Failed to decode audio");

        let player = Player::connect_new(&mixer);
        player.append(source);

        Self {
            player,
            _sink: sink,
        }
    }

    /// Current playback position in seconds.
    pub fn position_secs(&self) -> f64 {
        self.player.get_pos().as_secs_f64()
    }

    /// Current position as a fractional CWT frame index.
    pub fn cwt_frame_position(&self, frame_rate: u32) -> f64 {
        self.position_secs() * frame_rate as f64
    }

    pub fn toggle_pause(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    pub fn seek_relative(&self, delta_secs: f64) {
        let current = self.position_secs();
        let target = (current + delta_secs).max(0.0);
        let _ = self.player.try_seek(Duration::from_secs_f64(target));
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn is_finished(&self) -> bool {
        self.player.empty()
    }
}

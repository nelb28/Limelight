use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::fs::File;
use std::io::BufReader;
use std::cell::Cell;
use std::path::Path;
use std::time::Duration;

/// Plays the song and reports the position *in the song*, at any playback speed.
///
/// rodio's `Player::get_pos()` advances at wall-clock rate whatever the speed (measured:
/// at 0.5x the song's last 2 s took 4 s while get_pos advanced 4 s), but a seek resets it
/// to the target in song time. So the song position is tracked from an anchor taken at
/// every speed change or seek: song = anchor_song + (get_pos - anchor_raw) * speed.
pub struct AudioPlayer {
    player: Player,
    _sink: MixerDeviceSink,
    speed: Cell<f32>,
    /// (get_pos at the anchor, song position at the anchor), in seconds.
    anchor: Cell<(f64, f64)>,
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
            speed: Cell::new(1.0),
            anchor: Cell::new((0.0, 0.0)),
        }
    }

    fn raw_pos(&self) -> f64 {
        self.player.get_pos().as_secs_f64()
    }

    /// Current position in the song, in seconds (independent of playback speed).
    pub fn position_secs(&self) -> f64 {
        let (raw, song) = self.anchor.get();
        song + (self.raw_pos() - raw) * self.speed.get() as f64
    }

    /// Current playback speed (1.0 = normal).
    pub fn speed(&self) -> f32 {
        self.speed.get()
    }

    /// Change playback speed. Like a tape slowed down, this also lowers the pitch
    /// (0.75x = about 5 semitones down, 0.5x = one octave down).
    pub fn set_speed(&self, speed: f32) {
        self.anchor.set((self.raw_pos(), self.position_secs()));
        self.speed.set(speed);
        self.player.set_speed(speed);
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

    /// Seek by `delta_secs` of song time (not wall time, at any speed).
    pub fn seek_relative(&self, delta_secs: f64) {
        let target = (self.position_secs() + delta_secs).max(0.0);
        if self.player.try_seek(Duration::from_secs_f64(target)).is_ok() {
            // A seek resets get_pos to the target in song time.
            self.anchor.set((self.raw_pos(), target));
        }
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn is_finished(&self) -> bool {
        self.player.empty()
    }
}

/// Needs an audio device and a song:
/// `AUDIO_PATH=... cargo test --release speed_tracking -- --ignored --nocapture`
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    #[ignore]
    fn speed_tracking() {
        let path = std::env::var("AUDIO_PATH").expect("set AUDIO_PATH");
        let audio = AudioPlayer::new(Path::new(&path));
        audio.player.set_volume(0.0);
        let mut expected = 0.0;
        let check = |label: &str, speed: f32, wall: f64, expected: &mut f64| {
            sleep(Duration::from_secs_f64(wall));
            *expected += wall * speed as f64;
            let got = audio.position_secs();
            println!("{label:28} expected {:6.2}s  got {:6.2}s", expected, got);
            assert!((got - *expected).abs() < 0.15, "{label}: {got} vs {expected}");
        };
        check("1x for 1s", 1.0, 1.0, &mut expected);
        audio.set_speed(0.5);
        check("0.5x for 2s", 0.5, 2.0, &mut expected);
        audio.seek_relative(5.0);
        expected += 5.0;
        check("+5s seek, 0.5x for 1s", 0.5, 1.0, &mut expected);
        audio.set_speed(0.75);
        check("0.75x for 2s", 0.75, 2.0, &mut expected);
        audio.set_speed(1.0);
        check("back to 1x for 1s", 1.0, 1.0, &mut expected);
    }
}

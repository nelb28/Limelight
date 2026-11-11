mod app;
mod audio;
mod cwt_file;
mod filters;
mod graphics;

use std::path::PathBuf;
use winit::event_loop::EventLoop;
use app::{App, PanelData};
use cwt_file::CwtFile;
use filters::{Filter, FILTER_HELP};
use graphics::{ColorMode, DisplayOptions, Layout, ScaleMode};

const USAGE: &str = "\
Usage: music_vis <audio> <audio.cwt> [panel options] [--vs [panel options]]... [global options]

Panel options (apply to the current panel; --vs starts a new side-by-side panel
with default settings, so the same song can be compared under different filters):
  -f, --filter SPEC          add a filter (see below; repeat to chain)
  --scale, --db-range, --dev-range, --peak-pct   display options (see below)
  --bar-colors note|index    bar colors: by musical note (default) or by position

Global options:
  --phase                    also show the phase-deviation spectrogram
  --windowed                 start in a window instead of fullscreen
  -h, --help, --list-filters show this help

With more than one panel, each extra panel's bars show the difference from the
leftmost panel: white = higher than the left, black (grey edge) = lower (D toggles).

Keys: Space pause/resume, Left/Right seek 5 s, 1/3/2 speed 1x/0.75x/0.5x
      (pitch drops with speed), D toggle difference overlay, F11 toggle fullscreen,
      Esc quit.

Example (side by side):
  music_vis song.flac song.cwt -f tilt -f contrast --peak-pct 99.5 --vs -f whiten -f contrast
";

fn usage() -> ! {
    eprintln!("{USAGE}");
    eprintln!("{FILTER_HELP}");
    std::process::exit(1);
}

/// Filters and display options for one side-by-side panel.
#[derive(Default)]
struct PanelSpec {
    filters: Vec<Filter>,
    display: DisplayOptions,
}

const MAX_PANELS: usize = 4;

fn main() {
    env_logger::init();

    let mut positional: Vec<String> = Vec::new();
    let mut specs: Vec<PanelSpec> = vec![PanelSpec::default()];
    let mut show_phase = false;
    let mut fullscreen = true;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("{flag} requires a value");
                std::process::exit(1)
            })
        };
        let number = |flag: &str, v: String| -> f32 {
            match v.parse::<f32>() {
                Ok(n) if n > 0.0 => n,
                _ => {
                    eprintln!("{flag}: expected a positive number, got '{v}'");
                    std::process::exit(1)
                }
            }
        };
        let spec = specs.last_mut().unwrap();
        let display = &mut spec.display;
        match arg.as_str() {
            "-f" | "--filter" => {
                let text = value(&arg);
                match Filter::parse(&text) {
                    Ok(f) => spec.filters.push(f),
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1)
                    }
                }
            }
            "--vs" => {
                if specs.len() == MAX_PANELS {
                    eprintln!("--vs: at most {MAX_PANELS} panels");
                    std::process::exit(1)
                }
                specs.push(PanelSpec::default());
            }
            "--phase" => show_phase = true,
            "--windowed" => fullscreen = false,
            "--scale" => {
                display.scale = match value(&arg).as_str() {
                    "db" => ScaleMode::Db,
                    "linear" => ScaleMode::Linear,
                    "sqrt" => ScaleMode::Sqrt,
                    other => {
                        eprintln!("--scale: expected db, linear or sqrt, got '{other}'");
                        std::process::exit(1)
                    }
                }
            }
            "--bar-colors" => {
                display.bar_colors = match value(&arg).as_str() {
                    "note" => ColorMode::Note,
                    "index" => ColorMode::Classic,
                    other => {
                        eprintln!("--bar-colors: expected note or index, got '{other}'");
                        std::process::exit(1)
                    }
                }
            }
            "--db-range" => display.db_range = number(&arg, value(&arg)),
            "--dev-range" => display.dev_range = number(&arg, value(&arg)),
            "--peak-pct" => {
                display.peak_pct = number(&arg, value(&arg));
                if display.peak_pct > 100.0 {
                    eprintln!("--peak-pct: must be in (0, 100], got {}", display.peak_pct);
                    std::process::exit(1)
                }
            }
            "--list-filters" | "-h" | "--help" => {
                println!("{USAGE}");
                println!("{FILTER_HELP}");
                return;
            }
            a if a.starts_with('-') => {
                eprintln!("unknown option '{a}'");
                usage()
            }
            _ => positional.push(arg),
        }
    }
    if positional.len() != 2 {
        usage();
    }
    let audio_path = PathBuf::from(&positional[0]);
    let cwt_path = PathBuf::from(&positional[1]);

    let source = CwtFile::read(&cwt_path).expect("Failed to read CWT file");
    let count = specs.len();
    let mut panels = Vec::with_capacity(count);
    for (i, spec) in specs.into_iter().enumerate() {
        let label = match (count, i) {
            (1, _) => "filters".to_string(),
            (2, 0) => "left".to_string(),
            (2, _) => "right".to_string(),
            _ => format!("panel {} (from left)", i + 1),
        };
        let names: Vec<String> = spec.filters.iter().map(|f| format!("{f:?}")).collect();
        println!("{label}: {}", if names.is_empty() { "no filters".into() } else { names.join(" -> ") });
        if !show_phase && spec.filters.iter().any(Filter::is_phase) {
            eprintln!("  note: phase-* filters only affect the phase graph; add --phase to show it");
        }
        let mut cwt = source.clone();
        if !spec.filters.is_empty() {
            filters::apply(&mut cwt, &spec.filters);
        }
        cwt.set_reference(spec.display.peak_pct);
        panels.push(PanelData { cwt, display: spec.display });
    }
    drop(source);
    if panels.len() > 1 {
        println!("difference overlay on the right: white = higher than left, black = lower (D toggles)");
    }
    if panels.iter().any(|p| p.display.bar_colors == ColorMode::Note) {
        println!("note colors: {}", note_legend());
    }

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new(audio_path, panels, Layout::new(show_phase), fullscreen);

    event_loop.run_app(&mut app).unwrap();
}

/// Note names with their colors as 24-bit ANSI swatches, matching the shaders'
/// hue = pitch class / 12 (C = red), saturation 0.85.
fn note_legend() -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let (r, g, b) = hsv_to_rgb(i as f32 / 12.0, 0.85, 1.0);
            let c = |v: f32| (v * 255.0).round() as u8;
            format!("\x1b[38;2;{};{};{}m\u{2588}\u{2588}\x1b[0m {name}", c(r), c(g), c(b))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

#[cfg(test)]
mod tests {
    use crate::cwt_file::CwtFile;

    /// Bins of a CWT1-style file at 48 bins/octave starting at `f0`.
    fn cwt_with(f0: f32, n: usize) -> CwtFile {
        let freqs: Vec<f32> = (0..n).map(|i| f0 * 2f32.powf(i as f32 / 48.0)).collect();
        CwtFile::new(n as u32, 1, 44100, 60, freqs, vec![0.0; n], Vec::new())
    }

    /// Pitch class (0 = C .. 11 = B) of bin `i`, as the shaders compute it.
    fn pitch_class(cwt: &CwtFile, i: usize) -> f32 {
        let (offset, bpo) = cwt.pitch_mapping();
        (((offset + 12.0 * i as f32 / bpo) / 12.0).rem_euclid(1.0)) * 12.0
    }

    #[test]
    fn note_colors_line_up_with_notes() {
        // 19.45 Hz is D#0; 4 bins per semitone, so bin 24 is A0 (6 semitones up).
        let cwt = cwt_with(19.45, 430);
        assert!((pitch_class(&cwt, 0) - 3.0).abs() < 0.01, "D#0 -> {}", pitch_class(&cwt, 0));
        assert!((pitch_class(&cwt, 24) - 9.0).abs() < 0.01, "A0 -> {}", pitch_class(&cwt, 24));
        // Octaves share a pitch class (same color).
        assert!((pitch_class(&cwt, 24) - pitch_class(&cwt, 24 + 48 * 5)).abs() < 0.01);
        // A4 = 440 Hz exactly at bin 0 -> A.
        assert!((pitch_class(&cwt_with(440.0, 10), 0) - 9.0).abs() < 1e-4);
        // Middle C (261.63 Hz) -> C (0 or 12 at the wrap).
        let c = pitch_class(&cwt_with(261.63, 10), 0);
        assert!(c < 0.01 || c > 11.99, "C4 -> {c}");
    }
}

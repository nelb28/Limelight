mod wavelets;
mod scales;
mod kernel;
mod conv;
mod pcm;
mod batch;
mod frames;
mod cwt_file;

use wavelets::{Morlet, Wavelet};
use scales::log_scales;
use kernel::wavelet_kernel;
use batch::BatchCwt;
use frames::{reduce_phase_deviation, reduce_to_frames};
use cwt_file::CwtFile;

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Parse optional `--sample-rate <hz>` flag; remaining args are positional.
    let mut positional: Vec<String> = Vec::new();
    let mut rate_flag: Option<u32> = None;
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if a == "--sample-rate" || a == "-r" {
            let v = it.next().and_then(|v| v.parse::<u32>().ok());
            if v.is_none() {
                eprintln!("--sample-rate requires an integer value in Hz");
                std::process::exit(1);
            }
            rate_flag = v;
        } else {
            positional.push(a.clone());
        }
    }
    if positional.is_empty() || positional.len() > 2 {
        eprintln!("Usage: cwt <input.pcm> [output.cwt] [--sample-rate <hz>]");
        eprintln!("  Sample rate defaults to the <input>.sr sidecar file, else 96000.");
        std::process::exit(1);
    }
    let input_path = PathBuf::from(&positional[0]);
    let output_path = if positional.len() == 2 {
        PathBuf::from(&positional[1])
    } else {
        input_path.with_extension("cwt")
    };

    let sample_rate: u32 = rate_flag
        .or_else(|| {
            std::fs::read_to_string(input_path.with_extension("sr"))
                .ok()
                .and_then(|t| t.trim().parse().ok())
        })
        .unwrap_or(96000);
    println!("Sample rate: {} Hz", sample_rate);
    let dt = 1.0 / sample_rate as f64;
    let ts = 8.0;
    let frame_rate: u32 = 60;
    let hop_size = (sample_rate as f64 / frame_rate as f64).round() as usize; 
    let window_size = hop_size * 2; // 50% overlap
    let min_freq = 19.45;
    // Keep the top analysis frequency safely below Nyquist for low-rate (e.g. MP3) audio.
    let max_freq = f64::min(9550.0, sample_rate as f64 * 0.45);
    let voices_per_octave = 48;

    let morlet = Morlet::new(6.0);
    let scales = log_scales(min_freq, max_freq, voices_per_octave, morlet.center_freq());
    let num_scales = scales.len();

    let frequencies: Vec<f32> = scales
        .iter()
        .map(|&s| (morlet.center_freq() / s) as f32)
        .collect();

    println!("Reading PCM: {}", input_path.display());
    let start = Instant::now();
    let mono = pcm::read_pcm_stereo_to_mono(&input_path).expect("Failed to read PCM file");
    println!(
        "  {} samples ({:.1}s at {} Hz), read in {:.1}s",
        mono.len(),
        mono.len() as f64 / sample_rate as f64,
        sample_rate,
        start.elapsed().as_secs_f64(),
    );

    let max_kernel_len = scales
        .iter()
        .map(|&s| wavelet_kernel(&morlet, s, dt, ts).len())
        .max()
        .unwrap();
    println!("Max kernel length: {} samples", max_kernel_len);
    println!(
        "Number of scales: {} ({} voices/octave)",
        num_scales, voices_per_octave
    );

    println!("Preparing overlap-save processor (fft_size determined by max kernel)...");
    let mut batch = BatchCwt::new(&mono, max_kernel_len);

    let num_frames = (mono.len() + hop_size - 1) / hop_size;
    let mut all_frames: Vec<f32> = Vec::with_capacity(num_frames * num_scales);
    let mut all_phase: Vec<f32> = Vec::with_capacity(num_frames * num_scales);

    println!("Processing {} scales...", num_scales);
    let start = Instant::now();
    for (i, &scale) in scales.iter().enumerate() {
        let coeffs = batch.transform_scale(&morlet, scale, dt, ts);
        let mags: Vec<f64> = coeffs.iter().map(|c| c.norm()).collect();
        let frame_vals = reduce_to_frames(&mags, hop_size, window_size);
        all_frames.extend(frame_vals.iter().map(|&v| v as f32));
        drop(mags);
        let dev = reduce_phase_deviation(&coeffs, hop_size, window_size, frequencies[i] as f64, dt);
        all_phase.extend(dev.iter().map(|&v| v as f32));
        if (i + 1) % 50 == 0 || i + 1 == num_scales {
            println!(
                "  {}/{} scales ({:.0} Hz)",
                i + 1,
                num_scales,
                frequencies[i]
            );
        }
    }
    println!("  Done in {:.1}s", start.elapsed().as_secs_f64());

    // Rearrange from scale-major to frame-major layout
    let actual_num_frames = all_frames.len() / num_scales;
    let to_frame_major = |scale_major: &[f32]| {
        let mut out = vec![0.0f32; scale_major.len()];
        for s in 0..num_scales {
            for f in 0..actual_num_frames {
                out[f * num_scales + s] = scale_major[s * actual_num_frames + f];
            }
        }
        out
    };
    let frame_major = to_frame_major(&all_frames);
    let phase_major = to_frame_major(&all_phase);

    let cwt = CwtFile::new(
        num_scales as u32,
        actual_num_frames as u32,
        sample_rate,
        frame_rate,
        frequencies,
        frame_major,
        phase_major,
    );

    println!("Writing: {}", output_path.display());
    cwt.write(&output_path).expect("Failed to write CWT file");
    println!("Done. {} frames x {} scales", actual_num_frames, num_scales);
}

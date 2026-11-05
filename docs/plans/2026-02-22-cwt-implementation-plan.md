# CWT Processing & Visualization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a CLI tool that processes stereo PCM audio through a Continuous Wavelet Transform (48 voices per octave, ~430 frequency bins), and a GPU-accelerated visualizer that plays the audio with a synced bar/waveform display.

**Architecture:** Two separate Rust binaries (`cwt` and `music_vis`) connected by a `.cwt` binary file format. The CWT tool reads PCM, downmixes stereo to mono, runs ~430-scale Morlet CWT via FFT convolution (48 voices per octave across ~9 octaves), reduces to 60fps frames with 50% overlap, and writes the result. The visualizer loads the `.cwt` file, decodes the original FLAC for playback via rodio, and renders bars + waveform in wgpu. The number of bars is dynamic, read from the `.cwt` file header.

**Tech Stack:** Rust 2024 edition, rustfft, num-complex, wgpu 28.0, winit 0.30, rodio (symphonia-flac), bytemuck

**Design doc:** `docs/plans/2026-02-22-cwt-processing-and-visualization-design.md`

**Key parameters:**
- 48 voices per octave, 19.45 Hz to 9550 Hz (~9 octaves, ~430 total frequency bins)
- The existing `log_scales()` function in `cwt/src/scales.rs` already generates these
- 96 kHz sample rate, Morlet wavelet (omega0=6), time support=8
- 60 fps output, hop=1600 samples, window=3200 samples (50% overlap)

---

## Phase 1: CWT Processing Binary

### Task 1: Fix Morlet convenience methods and failing tests

The existing test `test_precompute_and_convolve` in `conv.rs:168` calls `Morlet::standard()` and `morlet.freq_to_scale()`, which don't exist yet.

**Files:**
- Modify: `cwt/src/wavelets/morlet.rs`
- Modify: `cwt/src/wavelets/mod.rs`
- Test: `cwt/src/conv.rs` (existing tests)

**Step 1: Add methods to Morlet**

In `cwt/src/wavelets/morlet.rs`, add to the `impl Morlet` block:

```rust
pub fn standard() -> Self {
    Self { omega0: 6.0 }
}
```

Add `freq_to_scale` to the `Wavelet` trait in `cwt/src/wavelets/mod.rs`:

```rust
pub trait Wavelet {
    fn eval(&self, t: f64) -> Complex64;
    fn center_freq(&self) -> f64;

    fn freq_to_scale(&self, freq: f64) -> f64 {
        self.center_freq() / freq
    }
}
```

**Step 2: Run tests to verify they pass**

Run: `cd cwt && cargo test`
Expected: All tests pass, including `test_precompute_and_convolve`.

**Step 3: Commit**

```bash
git add cwt/src/wavelets/morlet.rs cwt/src/wavelets/mod.rs
git commit -m "fix: add Morlet::standard() and Wavelet::freq_to_scale()"
```

---

### Task 2: PCM reader with stereo-to-mono downmix

Read raw 24-bit stereo interleaved PCM files and downmix to mono f64.

The PCM files produced by `pcm_converter.py` use `soundfile` with `format='RAW', subtype='PCM_24'`. This writes interleaved 24-bit little-endian signed integer samples: `[L0_b0, L0_b1, L0_b2, R0_b0, R0_b1, R0_b2, L1_b0, ...]`. Each frame is 6 bytes.

**Files:**
- Create: `cwt/src/pcm.rs`
- Modify: `cwt/src/main.rs` (add `mod pcm;`)

**Step 1: Write failing tests**

Create `cwt/src/pcm.rs`:

```rust
/// Read a 24-bit stereo interleaved PCM file and downmix to mono f64.
///
/// Input format: raw 24-bit little-endian signed integers, interleaved L/R.
/// Each frame is 6 bytes: [L_b0, L_b1, L_b2, R_b0, R_b1, R_b2].
/// Output: mono samples normalized to [-1.0, 1.0].
pub fn read_pcm_stereo_to_mono(path: &std::path::Path) -> std::io::Result<Vec<f64>> {
    todo!()
}

fn decode_i24_le(bytes: &[u8]) -> i32 {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_decode_i24_le_positive() {
        // 1000 as 24-bit LE: 0xE8, 0x03, 0x00
        let val = decode_i24_le(&[0xE8, 0x03, 0x00]);
        assert_eq!(val, 1000);
    }

    #[test]
    fn test_decode_i24_le_negative() {
        // -1 as 24-bit LE: 0xFF, 0xFF, 0xFF
        let val = decode_i24_le(&[0xFF, 0xFF, 0xFF]);
        assert_eq!(val, -1);
    }

    #[test]
    fn test_decode_i24_le_zero() {
        let val = decode_i24_le(&[0x00, 0x00, 0x00]);
        assert_eq!(val, 0);
    }

    #[test]
    fn test_read_pcm_stereo_to_mono() {
        // Write a tiny PCM file: 2 stereo frames
        // Frame 0: L=1000, R=3000 -> mono = 2000
        // Frame 1: L=-2000, R=2000 -> mono = 0
        let dir = std::env::temp_dir().join("cwt_test_pcm");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.pcm");

        let mut file = std::fs::File::create(&path).unwrap();
        // Helper to write a 24-bit LE value
        let write_i24 = |f: &mut std::fs::File, val: i32| {
            let bytes = val.to_le_bytes();
            f.write_all(&bytes[..3]).unwrap();
        };
        write_i24(&mut file, 1000);
        write_i24(&mut file, 3000);
        write_i24(&mut file, -2000);
        write_i24(&mut file, 2000);
        drop(file);

        let mono = read_pcm_stereo_to_mono(&path).unwrap();
        assert_eq!(mono.len(), 2);

        let norm = 8388608.0_f64; // 2^23
        let expected_0 = (1000.0 + 3000.0) / 2.0 / norm;
        let expected_1 = (-2000.0 + 2000.0) / 2.0 / norm;
        assert!((mono[0] - expected_0).abs() < 1e-10);
        assert!((mono[1] - expected_1).abs() < 1e-10);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cd cwt && cargo test pcm`
Expected: FAIL — `todo!()` panics.

**Step 3: Implement**

Replace the `todo!()` bodies in `cwt/src/pcm.rs`:

```rust
fn decode_i24_le(bytes: &[u8]) -> i32 {
    let b0 = bytes[0] as u32;
    let b1 = bytes[1] as u32;
    let b2 = bytes[2] as u32;
    let val = b0 | (b1 << 8) | (b2 << 16);
    // Sign-extend from 24 to 32 bits
    if val & 0x800000 != 0 {
        (val | 0xFF000000) as i32
    } else {
        val as i32
    }
}

pub fn read_pcm_stereo_to_mono(path: &std::path::Path) -> std::io::Result<Vec<f64>> {
    let data = std::fs::read(path)?;
    let frame_size = 6; // 3 bytes per sample * 2 channels
    if data.len() % frame_size != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("PCM file size {} is not a multiple of frame size {}", data.len(), frame_size),
        ));
    }
    let num_frames = data.len() / frame_size;
    let norm = 8388608.0; // 2^23
    let mut mono = Vec::with_capacity(num_frames);
    for i in 0..num_frames {
        let offset = i * frame_size;
        let left = decode_i24_le(&data[offset..offset + 3]) as f64 / norm;
        let right = decode_i24_le(&data[offset + 3..offset + 6]) as f64 / norm;
        mono.push((left + right) / 2.0);
    }
    Ok(mono)
}
```

Add `mod pcm;` to `cwt/src/main.rs`.

**Step 4: Run tests to verify they pass**

Run: `cd cwt && cargo test pcm`
Expected: All 4 tests pass.

**Step 5: Commit**

```bash
git add cwt/src/pcm.rs cwt/src/main.rs
git commit -m "feat: add PCM reader with stereo-to-mono downmix"
```

---

### Task 3: Batch CWT processor

The existing `convolve_fft` recomputes the signal FFT on every call. For processing ~430 scales, we should compute the signal FFT once and reuse it. Create a `BatchCwt` struct that orchestrates this.

**Files:**
- Create: `cwt/src/batch.rs`
- Modify: `cwt/src/conv.rs` (expose `next_power_of_two`)
- Modify: `cwt/src/main.rs` (add `mod batch;`)

**Step 1: Write failing test**

Create `cwt/src/batch.rs`:

```rust
use crate::conv;
use crate::kernel::wavelet_kernel;
use crate::wavelets::{Complex64, Wavelet};
use rustfft::{num_complex::Complex, FftPlanner};

pub struct BatchCwt {
    fft_size: usize,
    signal_len: usize,
    signal_fft: Vec<Complex<f64>>,
    planner: FftPlanner<f64>,
}

impl BatchCwt {
    /// Prepare for batch CWT by pre-computing the signal FFT.
    /// `max_kernel_len` is the length of the longest wavelet kernel across all scales.
    pub fn new(signal: &[f64], max_kernel_len: usize) -> Self {
        todo!()
    }

    /// Compute CWT for one scale, returning per-sample magnitudes.
    pub fn transform_scale<W: Wavelet>(
        &mut self,
        wavelet: &W,
        scale: f64,
        dt: f64,
        ts: f64,
    ) -> Vec<f64> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavelets::Morlet;
    use std::f64::consts::PI;

    #[test]
    fn test_batch_cwt_sine_detection() {
        let morlet = Morlet::standard();
        let sample_rate = 1000.0;
        let dt = 1.0 / sample_rate;
        let ts = 8.0;
        let target_freq = 100.0;

        // Generate a 100 Hz sine wave, 1 second
        let signal: Vec<f64> = (0..1000)
            .map(|i| (2.0 * PI * target_freq * i as f64 * dt).sin())
            .collect();

        let scale_on = morlet.freq_to_scale(target_freq);
        let scale_off = morlet.freq_to_scale(400.0);

        let kernel_on = wavelet_kernel(&morlet, scale_on, dt, ts);
        let kernel_off = wavelet_kernel(&morlet, scale_off, dt, ts);
        let max_kernel_len = kernel_on.len().max(kernel_off.len());

        let mut batch = BatchCwt::new(&signal, max_kernel_len);
        let mags_on = batch.transform_scale(&morlet, scale_on, dt, ts);
        let mags_off = batch.transform_scale(&morlet, scale_off, dt, ts);

        // Magnitude at the target frequency should be much larger
        let peak_on: f64 = mags_on.iter().cloned().fold(0.0, f64::max);
        let peak_off: f64 = mags_off.iter().cloned().fold(0.0, f64::max);
        assert!(peak_on > peak_off * 3.0, "on={peak_on}, off={peak_off}");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd cwt && cargo test batch`
Expected: FAIL — `todo!()` panics.

**Step 3: Implement**

Make `next_power_of_two` public in `cwt/src/conv.rs`:

```rust
pub fn next_power_of_two(n: usize) -> usize {
```

Implement `BatchCwt` in `cwt/src/batch.rs`:

```rust
impl BatchCwt {
    pub fn new(signal: &[f64], max_kernel_len: usize) -> Self {
        let fft_size = conv::next_power_of_two(signal.len() + max_kernel_len - 1);
        let mut planner = FftPlanner::new();

        // Zero-pad and FFT the signal once
        let mut signal_fft: Vec<Complex<f64>> = signal
            .iter()
            .map(|&x| Complex::new(x, 0.0))
            .collect();
        signal_fft.resize(fft_size, Complex::new(0.0, 0.0));
        let fft = planner.plan_fft_forward(fft_size);
        fft.process(&mut signal_fft);

        Self {
            fft_size,
            signal_len: signal.len(),
            signal_fft,
            planner,
        }
    }

    pub fn transform_scale<W: Wavelet>(
        &mut self,
        wavelet: &W,
        scale: f64,
        dt: f64,
        ts: f64,
    ) -> Vec<f64> {
        let kernel = wavelet_kernel(wavelet, scale, dt, ts);

        // Zero-pad kernel and FFT
        let mut kernel_fft: Vec<Complex<f64>> = kernel
            .iter()
            .map(|&c| Complex::new(c.re, c.im))
            .collect();
        kernel_fft.resize(self.fft_size, Complex::new(0.0, 0.0));
        let fft = self.planner.plan_fft_forward(self.fft_size);
        fft.process(&mut kernel_fft);

        // Multiply signal_fft * kernel_fft into scratch buffer
        let mut product: Vec<Complex<f64>> = self
            .signal_fft
            .iter()
            .zip(kernel_fft.iter())
            .map(|(&s, &k)| s * k)
            .collect();

        // Inverse FFT
        let ifft = self.planner.plan_fft_inverse(self.fft_size);
        ifft.process(&mut product);

        // Normalize and extract magnitudes
        let norm = 1.0 / self.fft_size as f64;
        product
            .iter()
            .take(self.signal_len)
            .map(|c| Complex64::new(c.re * norm, c.im * norm).norm())
            .collect()
    }
}
```

Add `mod batch;` to `cwt/src/main.rs` and `pub` to `next_power_of_two` in `conv.rs`.

**Step 4: Run tests to verify they pass**

Run: `cd cwt && cargo test batch`
Expected: PASS.

**Step 5: Commit**

```bash
git add cwt/src/batch.rs cwt/src/conv.rs cwt/src/main.rs
git commit -m "feat: add BatchCwt for efficient multi-scale processing"
```

---

### Task 4: Frame reduction with 50% overlap

Reduce per-sample magnitudes to per-frame values by averaging over overlapping windows.

**Files:**
- Create: `cwt/src/frames.rs`
- Modify: `cwt/src/main.rs` (add `mod frames;`)

**Step 1: Write failing test**

Create `cwt/src/frames.rs`:

```rust
/// Reduce per-sample magnitudes to per-frame values.
///
/// Each frame's value is the mean magnitude over a window of `window_size` samples
/// centered on the frame's time position. Frames are spaced `hop_size` samples apart.
///
/// With window_size = 2 * hop_size, this gives 50% overlap between adjacent frames.
pub fn reduce_to_frames(magnitudes: &[f64], hop_size: usize, window_size: usize) -> Vec<f64> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduce_basic() {
        // 10 samples, hop=2, window=4 (50% overlap)
        // Frame centers at: 0, 2, 4, 6, 8
        let mags = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let frames = reduce_to_frames(&mags, 2, 4);
        assert_eq!(frames.len(), 5);
    }

    #[test]
    fn test_reduce_constant_signal() {
        let mags = vec![5.0; 100];
        let frames = reduce_to_frames(&mags, 10, 20);
        for f in &frames {
            assert!((f - 5.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_reduce_overlap_smoothing() {
        // A step function should be smoothed by overlap
        let mut mags = vec![0.0; 50];
        mags.extend(vec![1.0; 50]);
        let frames = reduce_to_frames(&mags, 10, 20);
        // Frame at the boundary should be between 0 and 1
        let boundary_frame = frames.len() / 2;
        assert!(frames[boundary_frame] > 0.0);
        assert!(frames[boundary_frame] < 1.0);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd cwt && cargo test frames`
Expected: FAIL — `todo!()` panics.

**Step 3: Implement**

```rust
pub fn reduce_to_frames(magnitudes: &[f64], hop_size: usize, window_size: usize) -> Vec<f64> {
    let num_frames = (magnitudes.len() + hop_size - 1) / hop_size;
    let half_window = window_size / 2;

    (0..num_frames)
        .map(|frame_idx| {
            let center = frame_idx * hop_size;
            let start = center.saturating_sub(half_window);
            let end = (center + half_window).min(magnitudes.len());
            let window = &magnitudes[start..end];
            if window.is_empty() {
                0.0
            } else {
                window.iter().sum::<f64>() / window.len() as f64
            }
        })
        .collect()
}
```

Add `mod frames;` to `cwt/src/main.rs`.

**Step 4: Run tests to verify they pass**

Run: `cd cwt && cargo test frames`
Expected: All 3 tests pass.

**Step 5: Commit**

```bash
git add cwt/src/frames.rs cwt/src/main.rs
git commit -m "feat: add frame reduction with overlapping windows"
```

---

### Task 5: CWT file format writer and reader

Define the `.cwt` binary format and implement write + read (read is needed by `music_vis` later, but we test the round-trip here).

**Files:**
- Create: `cwt/src/cwt_file.rs`
- Modify: `cwt/src/main.rs` (add `mod cwt_file;`)

**Step 1: Write failing test**

Create `cwt/src/cwt_file.rs`:

```rust
use std::io::{self, Read, Write};
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"CWT1";

pub struct CwtHeader {
    pub num_scales: u32,
    pub num_frames: u32,
    pub sample_rate: u32,
    pub frame_rate: u32,
    pub frequencies: Vec<f32>, // Hz value for each scale
}

pub struct CwtFile {
    pub header: CwtHeader,
    /// Row-major: frames[frame_idx * num_scales + scale_idx]
    pub data: Vec<f32>,
}

impl CwtFile {
    pub fn new(
        num_scales: u32,
        num_frames: u32,
        sample_rate: u32,
        frame_rate: u32,
        frequencies: Vec<f32>,
        data: Vec<f32>,
    ) -> Self {
        todo!()
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        todo!()
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        let frequencies = vec![20.0, 100.0, 1000.0, 10000.0];
        let data = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let cwt = CwtFile::new(4, 2, 96000, 60, frequencies.clone(), data.clone());

        let dir = std::env::temp_dir().join("cwt_test_file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.cwt");
        cwt.write(&path).unwrap();

        let loaded = CwtFile::read(&path).unwrap();
        assert_eq!(loaded.header.num_scales, 4);
        assert_eq!(loaded.header.num_frames, 2);
        assert_eq!(loaded.header.sample_rate, 96000);
        assert_eq!(loaded.header.frame_rate, 60);
        assert_eq!(loaded.header.frequencies, frequencies);
        assert_eq!(loaded.data.len(), data.len());
        for (a, b) in loaded.data.iter().zip(data.iter()) {
            assert!((a - b).abs() < 1e-6);
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_bad_magic() {
        let dir = std::env::temp_dir().join("cwt_test_bad_magic");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.cwt");
        std::fs::write(&path, b"NOPE").unwrap();

        let result = CwtFile::read(&path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd cwt && cargo test cwt_file`
Expected: FAIL — `todo!()` panics.

**Step 3: Implement**

```rust
impl CwtFile {
    pub fn new(
        num_scales: u32,
        num_frames: u32,
        sample_rate: u32,
        frame_rate: u32,
        frequencies: Vec<f32>,
        data: Vec<f32>,
    ) -> Self {
        assert_eq!(frequencies.len(), num_scales as usize);
        assert_eq!(data.len(), (num_scales * num_frames) as usize);
        Self {
            header: CwtHeader {
                num_scales,
                num_frames,
                sample_rate,
                frame_rate,
                frequencies,
            },
            data,
        }
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let mut f = std::fs::File::create(path)?;
        f.write_all(MAGIC)?;
        f.write_all(&self.header.num_scales.to_le_bytes())?;
        f.write_all(&self.header.num_frames.to_le_bytes())?;
        f.write_all(&self.header.sample_rate.to_le_bytes())?;
        f.write_all(&self.header.frame_rate.to_le_bytes())?;
        for &freq in &self.header.frequencies {
            f.write_all(&freq.to_le_bytes())?;
        }
        for &val in &self.data {
            f.write_all(&val.to_le_bytes())?;
        }
        Ok(())
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let mut f = std::fs::File::open(path)?;

        let mut magic = [0u8; 4];
        f.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad magic: expected CWT1, got {:?}", magic),
            ));
        }

        let read_u32 = |f: &mut std::fs::File| -> io::Result<u32> {
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(u32::from_le_bytes(buf))
        };
        let read_f32 = |f: &mut std::fs::File| -> io::Result<f32> {
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(f32::from_le_bytes(buf))
        };

        let num_scales = read_u32(&mut f)?;
        let num_frames = read_u32(&mut f)?;
        let sample_rate = read_u32(&mut f)?;
        let frame_rate = read_u32(&mut f)?;

        let mut frequencies = Vec::with_capacity(num_scales as usize);
        for _ in 0..num_scales {
            frequencies.push(read_f32(&mut f)?);
        }

        let data_len = (num_scales * num_frames) as usize;
        let mut data = Vec::with_capacity(data_len);
        for _ in 0..data_len {
            data.push(read_f32(&mut f)?);
        }

        Ok(Self::new(num_scales, num_frames, sample_rate, frame_rate, frequencies, data))
    }
}
```

Add `mod cwt_file;` to `cwt/src/main.rs`.

**Step 4: Run tests to verify they pass**

Run: `cd cwt && cargo test cwt_file`
Expected: Both tests pass.

**Step 5: Commit**

```bash
git add cwt/src/cwt_file.rs cwt/src/main.rs
git commit -m "feat: add CWT binary file format writer and reader"
```

---

### Task 6: Wire up the CWT CLI

Connect all components into the `cwt` binary's `main()`: parse args, read PCM, run batch CWT, reduce frames, write output. Uses the existing `log_scales()` with 48 voices per octave to generate ~430 frequency bins.

**Files:**
- Modify: `cwt/src/main.rs`

**Step 1: Implement main**

Replace `cwt/src/main.rs` entirely:

```rust
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
use frames::reduce_to_frames;
use cwt_file::CwtFile;

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: cwt <input.pcm> [output.cwt]");
        std::process::exit(1);
    }
    let input_path = PathBuf::from(&args[1]);
    let output_path = if args.len() == 3 {
        PathBuf::from(&args[2])
    } else {
        input_path.with_extension("cwt")
    };

    // Parameters
    let sample_rate: u32 = 96000;
    let dt = 1.0 / sample_rate as f64;
    let ts = 8.0;
    let frame_rate: u32 = 60;
    let hop_size = sample_rate as usize / frame_rate as usize; // 1600
    let window_size = hop_size * 2; // 3200 (50% overlap)
    let min_freq = 19.45;
    let max_freq = 9550.0;
    let voices_per_octave = 48;

    let morlet = Morlet::new(6.0);

    // Generate scales: 48 voices per octave across ~9 octaves = ~430 scales
    let scales = log_scales(min_freq, max_freq, voices_per_octave, morlet.center_freq());
    let num_scales = scales.len();

    // Frequencies for the header (low to high, matching scale order)
    let frequencies: Vec<f32> = scales
        .iter()
        .map(|&s| (morlet.center_freq() / s) as f32)
        .collect();

    println!("Reading PCM: {}", input_path.display());
    let start = Instant::now();
    let mono = pcm::read_pcm_stereo_to_mono(&input_path).expect("Failed to read PCM file");
    println!("  {} samples ({:.1}s at {} Hz), read in {:.1}s",
        mono.len(),
        mono.len() as f64 / sample_rate as f64,
        sample_rate,
        start.elapsed().as_secs_f64(),
    );

    // Find max kernel length for FFT sizing
    let max_kernel_len = scales
        .iter()
        .map(|&s| wavelet_kernel(&morlet, s, dt, ts).len())
        .max()
        .unwrap();
    println!("Max kernel length: {} samples", max_kernel_len);
    println!("Number of scales: {} ({} voices/octave)", num_scales, voices_per_octave);

    println!("Computing signal FFT...");
    let start = Instant::now();
    let mut batch = BatchCwt::new(&mono, max_kernel_len);
    println!("  Done in {:.1}s", start.elapsed().as_secs_f64());

    let num_frames = (mono.len() + hop_size - 1) / hop_size;
    let mut all_frames: Vec<f32> = Vec::with_capacity(num_frames * num_scales);

    println!("Processing {} scales...", num_scales);
    let start = Instant::now();
    for (i, &scale) in scales.iter().enumerate() {
        let mags = batch.transform_scale(&morlet, scale, dt, ts);
        let frame_vals = reduce_to_frames(&mags, hop_size, window_size);
        all_frames.extend(frame_vals.iter().map(|&v| v as f32));
        if (i + 1) % 50 == 0 || i + 1 == num_scales {
            println!("  {}/{} scales ({:.0} Hz)", i + 1, num_scales, frequencies[i]);
        }
    }
    println!("  Done in {:.1}s", start.elapsed().as_secs_f64());

    // Rearrange: currently scale-major (all frames for scale 0, then scale 1, ...).
    // We need frame-major (all scales for frame 0, then frame 1, ...).
    let actual_num_frames = all_frames.len() / num_scales;
    let mut frame_major: Vec<f32> = vec![0.0; all_frames.len()];
    for s in 0..num_scales {
        for f in 0..actual_num_frames {
            frame_major[f * num_scales + s] = all_frames[s * actual_num_frames + f];
        }
    }

    let cwt = CwtFile::new(
        num_scales as u32,
        actual_num_frames as u32,
        sample_rate,
        frame_rate,
        frequencies,
        frame_major,
    );

    println!("Writing: {}", output_path.display());
    cwt.write(&output_path).expect("Failed to write CWT file");
    println!("Done. {} frames x {} scales", actual_num_frames, num_scales);
}
```

**Step 2: Build and verify**

Run: `cd cwt && cargo build`
Expected: Compiles with no errors.

**Step 3: Smoke test with a real PCM file**

Run: `cd cwt && cargo run -- "/home/nelbeus/Projects/Limelight/Audio Processing/PCM/<pick one file>.pcm"`
Expected: Processes and writes a `.cwt` file. Verify the output file exists and has reasonable size (~430 scales * num_frames * 4 bytes + header).

**Step 4: Run all tests**

Run: `cd cwt && cargo test`
Expected: All tests pass.

**Step 5: Commit**

```bash
git add cwt/src/main.rs
git commit -m "feat: wire up CWT CLI pipeline (48 voices/octave, ~430 scales)"
```

---

## Phase 2: Music Visualizer

### Task 7: Add CWT file reader to music_vis

Copy the file format reader into `music_vis` (or reference a shared crate — for now, duplicate since it's small).

**Files:**
- Create: `music_vis/src/cwt_file.rs`
- Modify: `music_vis/src/main.rs` (add `mod cwt_file;`)

**Step 1: Create the reader**

Copy the `CwtFile`, `CwtHeader`, and `CwtFile::read()` implementation from `cwt/src/cwt_file.rs` into `music_vis/src/cwt_file.rs`. Only include `read()` and `new()` — no need for `write()`.

**Step 2: Add convenience methods**

Add to `CwtFile`:

```rust
/// Get magnitudes for a specific frame, interpolated between two frames.
/// `t` is the fractional frame position (e.g., 2.5 means halfway between frame 2 and 3).
pub fn interpolated_frame(&self, t: f64) -> Vec<f32> {
    let n = self.header.num_scales as usize;
    let frame_a = (t.floor() as usize).min(self.header.num_frames as usize - 1);
    let frame_b = (frame_a + 1).min(self.header.num_frames as usize - 1);
    let frac = (t - t.floor()) as f32;
    let mut result = Vec::with_capacity(n);
    for s in 0..n {
        let a = self.data[frame_a * n + s];
        let b = self.data[frame_b * n + s];
        result.push(a + (b - a) * frac);
    }
    result
}

/// Find the global maximum magnitude across all frames and scales.
pub fn global_max(&self) -> f32 {
    self.data.iter().cloned().fold(0.0f32, f32::max)
}
```

**Step 3: Build**

Run: `cd music_vis && cargo build`
Expected: Compiles.

**Step 4: Commit**

```bash
git add music_vis/src/cwt_file.rs music_vis/src/main.rs
git commit -m "feat: add CWT file reader to music_vis"
```

---

### Task 8: Add audio playback with position tracking

Use `rodio` to play FLAC audio and track the playback position via a shared atomic counter.

**Files:**
- Modify: `music_vis/Cargo.toml` (add rodio dependency)
- Create: `music_vis/src/audio.rs`
- Modify: `music_vis/src/main.rs` (add `mod audio;`)

**Step 1: Add dependency**

Add to `music_vis/Cargo.toml`:

```toml
rodio = { version = "0.20", features = ["symphonia-flac"] }
```

Note: if `0.20` is not available, use the latest version. Check with `cargo add rodio --features symphonia-flac` if unsure.

**Step 2: Implement audio player**

Create `music_vis/src/audio.rs`:

```rust
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct AudioPlayer {
    sink: Sink,
    _stream: OutputStream,
    position_samples: Arc<AtomicU64>,
    sample_rate: u32,
}

/// A Source wrapper that tracks how many samples have been consumed.
struct TrackedSource<S: Source<Item = f32>> {
    inner: S,
    position: Arc<AtomicU64>,
    channels: u16,
}

impl<S: Source<Item = f32>> Iterator for TrackedSource<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        self.position.fetch_add(1, Ordering::Relaxed);
        Some(sample)
    }
}

impl<S: Source<Item = f32>> Source for TrackedSource<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

impl AudioPlayer {
    pub fn new(path: &Path) -> Self {
        let (stream, stream_handle) = OutputStream::try_default().expect("Failed to open audio output");
        let sink = Sink::try_new(&stream_handle).expect("Failed to create audio sink");

        let file = File::open(path).expect("Failed to open audio file");
        let reader = BufReader::new(file);
        let source = Decoder::new(reader).expect("Failed to decode audio");

        let sample_rate = source.sample_rate();
        let channels = source.channels();

        let position = Arc::new(AtomicU64::new(0));
        let tracked = TrackedSource {
            inner: source,
            position: position.clone(),
            channels,
        };

        sink.append(tracked);

        Self {
            sink,
            _stream: stream,
            position_samples: position,
            sample_rate,
        }
    }

    /// Current playback position in seconds.
    pub fn position_secs(&self) -> f64 {
        let raw_samples = self.position_samples.load(Ordering::Relaxed);
        // TrackedSource counts individual samples across all channels
        // Divide by 2 for stereo to get frame count
        let frames = raw_samples / 2;
        frames as f64 / self.sample_rate as f64
    }

    /// Current position as a fractional CWT frame index.
    pub fn cwt_frame_position(&self, frame_rate: u32) -> f64 {
        self.position_secs() * frame_rate as f64
    }

    pub fn toggle_pause(&self) {
        if self.sink.is_paused() {
            self.sink.play();
        } else {
            self.sink.pause();
        }
    }

    pub fn seek_relative(&self, delta_secs: f64) {
        let current = self.position_secs();
        let target = (current + delta_secs).max(0.0);
        let _ = self.sink.try_seek(Duration::from_secs_f64(target));
    }

    pub fn is_finished(&self) -> bool {
        self.sink.empty()
    }
}
```

Add `mod audio;` to `music_vis/src/main.rs`.

**Step 3: Build**

Run: `cd music_vis && cargo build`
Expected: Compiles. If `rodio` version needs adjustment, fix `Cargo.toml`.

**Step 4: Commit**

```bash
git add music_vis/Cargo.toml music_vis/src/audio.rs music_vis/src/main.rs
git commit -m "feat: add audio playback with position tracking"
```

---

### Task 9: Bar visualization shader and render pipeline

Create the WGSL shader for the frequency bars and set up the wgpu render pipeline. The number of bars is dynamic — read from the CWT file header (`num_scales`, typically ~430).

**Files:**
- Create: `music_vis/src/graphics/bars.wgsl`
- Create: `music_vis/src/graphics/pipeline.rs`
- Modify: `music_vis/src/graphics/mod.rs`
- Modify: `music_vis/src/graphics/state.rs`

**Step 1: Write the shader**

Create `music_vis/src/graphics/bars.wgsl`:

```wgsl
struct Params {
    num_bars: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> magnitudes: array<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let c = v * s;
    let hp = h * 6.0;
    let x = c * (1.0 - abs(hp % 2.0 - 1.0));
    let m = v - c;
    var rgb: vec3<f32>;
    if hp < 1.0 {
        rgb = vec3(c, x, 0.0);
    } else if hp < 2.0 {
        rgb = vec3(x, c, 0.0);
    } else if hp < 3.0 {
        rgb = vec3(0.0, c, x);
    } else if hp < 4.0 {
        rgb = vec3(0.0, x, c);
    } else if hp < 5.0 {
        rgb = vec3(x, 0.0, c);
    } else {
        rgb = vec3(c, 0.0, x);
    }
    return rgb + vec3(m, m, m);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) ii: u32,
) -> VertexOutput {
    let n = f32(params.num_bars);
    let bar_width = 2.0 / n;
    let gap = bar_width * 0.05;
    let mag = clamp(magnitudes[ii], 0.0, 1.0);

    let x0 = -1.0 + f32(ii) * bar_width + gap;
    let x1 = x0 + bar_width - 2.0 * gap;
    let y0 = -1.0;
    let y1 = -1.0 + mag * 2.0;

    // 6 vertices: two triangles forming a quad
    var pos: vec2<f32>;
    switch vi {
        case 0u: { pos = vec2(x0, y0); }
        case 1u: { pos = vec2(x1, y0); }
        case 2u: { pos = vec2(x0, y1); }
        case 3u: { pos = vec2(x0, y1); }
        case 4u: { pos = vec2(x1, y0); }
        default: { pos = vec2(x1, y1); }
    }

    // Color: hue from warm (low freq) to cool (high freq)
    let hue = 0.0 + 0.7 * (f32(ii) / max(n - 1.0, 1.0));
    let color = hsv_to_rgb(hue, 0.85, 0.9);

    var out: VertexOutput;
    out.position = vec4(pos, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(in.color, 1.0);
}
```

**Step 2: Create the pipeline setup**

Create `music_vis/src/graphics/pipeline.rs`:

```rust
use wgpu::util::DeviceExt;

pub struct BarPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub params_buffer: wgpu::Buffer,
    pub magnitudes_buffer: wgpu::Buffer,
    pub num_bars: u32,
}

impl BarPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, num_bars: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bar Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bars.wgsl").into()),
        });

        // Params uniform buffer (num_bars as u32, padded to 16 bytes for alignment)
        let params_data: [u32; 4] = [num_bars, 0, 0, 0];
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Params Buffer"),
            contents: bytemuck::cast_slice(&params_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Magnitudes storage buffer (dynamic size based on num_bars)
        let magnitudes = vec![0.0f32; num_bars as usize];
        let magnitudes_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Magnitudes Buffer"),
            contents: bytemuck::cast_slice(&magnitudes),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bar Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bar Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: magnitudes_buffer.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bar Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Bar Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group,
            params_buffer,
            magnitudes_buffer,
            num_bars,
        }
    }

    pub fn update_magnitudes(&self, queue: &wgpu::Queue, magnitudes: &[f32]) {
        queue.write_buffer(&self.magnitudes_buffer, 0, bytemuck::cast_slice(magnitudes));
    }
}
```

**Step 3: Update graphics/mod.rs and state.rs**

Update `music_vis/src/graphics/mod.rs`:

```rust
pub mod state;
pub mod pipeline;

pub use state::State;
pub use pipeline::BarPipeline;
```

Update `State` in `music_vis/src/graphics/state.rs` to accept `num_bars: u32` in `new()`, hold a `BarPipeline`, and use it in `render()`:

- Add `bar_pipeline: BarPipeline` field
- Initialize it in `new()` with `BarPipeline::new(&device, format, num_bars)`
- In `render()`, after beginning the render pass, draw the bars:

```rust
rpass.set_pipeline(&self.bar_pipeline.pipeline);
rpass.set_bind_group(0, &self.bar_pipeline.bind_group, &[]);
rpass.draw(0..6, 0..self.bar_pipeline.num_bars);
```

**Step 4: Add bytemuck dependency**

Add to `music_vis/Cargo.toml`:

```toml
bytemuck = { version = "1", features = ["derive"] }
```

**Step 5: Build and verify**

Run: `cd music_vis && cargo build`
Expected: Compiles.

**Step 6: Commit**

```bash
git add music_vis/src/graphics/ music_vis/Cargo.toml
git commit -m "feat: add bar visualization shader and render pipeline"
```

---

### Task 10: Wire up the full visualizer

Connect CLI args, CWT loading, audio playback, and rendering into the main event loop. The number of bars comes from the CWT file's `num_scales` field.

**Files:**
- Modify: `music_vis/src/main.rs`
- Modify: `music_vis/src/app.rs`
- Modify: `music_vis/src/graphics/state.rs`

**Step 1: Update main.rs for CLI args**

```rust
mod app;
mod graphics;
mod cwt_file;
mod audio;

use winit::event_loop::EventLoop;
use app::App;
use std::path::PathBuf;

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: music_vis <song.cwt> <song.flac>");
        std::process::exit(1);
    }
    let cwt_path = PathBuf::from(&args[1]);
    let audio_path = PathBuf::from(&args[2]);

    let cwt_data = cwt_file::CwtFile::read(&cwt_path).expect("Failed to read CWT file");
    println!(
        "Loaded CWT: {} scales x {} frames @ {}fps",
        cwt_data.header.num_scales, cwt_data.header.num_frames, cwt_data.header.frame_rate,
    );

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new(cwt_data, audio_path);
    event_loop.run_app(&mut app).unwrap();
}
```

**Step 2: Update App to hold audio + CWT state and handle keyboard**

Modify `music_vis/src/app.rs` to:

- Accept `CwtFile` and audio path in `App::new()`
- Store `cwt_data: CwtFile`, `audio_path: PathBuf`, `global_max: f32`
- Create `AudioPlayer` in `resumed()` after window/state creation
- Pass `cwt_data.header.num_scales` to `State::new()` so it creates the correctly-sized `BarPipeline`
- Handle `KeyboardInput` events for spacebar (pause), left/right arrows (seek), escape (exit)
- In `RedrawRequested`: get current audio position, compute interpolated CWT frame, normalize by `global_max`, update magnitudes buffer, then render

Key additions to the event handler:

```rust
WindowEvent::KeyboardInput { event, .. } => {
    if event.state == winit::event::ElementState::Pressed {
        use winit::keyboard::{Key, NamedKey};
        match event.logical_key {
            Key::Named(NamedKey::Space) => {
                if let Some(audio) = &self.audio {
                    audio.toggle_pause();
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(audio) = &self.audio {
                    audio.seek_relative(5.0);
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if let Some(audio) = &self.audio {
                    audio.seek_relative(-5.0);
                }
            }
            Key::Named(NamedKey::Escape) => event_loop.exit(),
            _ => {}
        }
    }
}
```

In `RedrawRequested`, before rendering:

```rust
if let (Some(audio), Some(state)) = (&self.audio, &self.state) {
    let frame_pos = audio.cwt_frame_position(self.cwt_data.header.frame_rate);
    let mut magnitudes = self.cwt_data.interpolated_frame(frame_pos);
    // Normalize so the loudest moment in the file reaches 1.0
    if self.global_max > 0.0 {
        for m in &mut magnitudes {
            *m /= self.global_max;
        }
    }
    state.bar_pipeline.update_magnitudes(&state.queue, &magnitudes);
}
```

**Step 3: Build and test**

Run: `cd music_vis && cargo build`
Expected: Compiles.

Run with a real CWT + FLAC file to verify visually:
```bash
cd music_vis && cargo run -- /path/to/song.cwt /path/to/song.flac
```
Expected: Window opens, audio plays, ~430 bars animate in sync with a warm-to-cool color gradient.

**Step 4: Commit**

```bash
git add music_vis/src/
git commit -m "feat: wire up full visualizer with audio sync and keyboard controls"
```

---

### Task 11: Waveform overlay

Add a waveform display behind the bars showing a rolling window of the audio signal.

**Files:**
- Create: `music_vis/src/graphics/waveform.wgsl`
- Create: `music_vis/src/graphics/waveform_pipeline.rs`
- Modify: `music_vis/src/graphics/mod.rs`
- Modify: `music_vis/src/graphics/state.rs`
- Modify: `music_vis/src/app.rs`
- Modify: `music_vis/src/audio.rs` (add decoded samples buffer)

**Step 1: Decode audio samples for waveform display**

In `audio.rs`, add a separate decode pass that loads the full audio as mono f32 samples into a `Vec<f32>`. Store this alongside the `AudioPlayer`. Add a method:

```rust
pub fn waveform_window(&self, center_sample: usize, window_size: usize) -> Vec<f32> {
    // Return a window of mono samples centered on center_sample
}
```

**Step 2: Write waveform shader**

Create `music_vis/src/graphics/waveform.wgsl`. This shader draws a line from a buffer of sample values. Use a storage buffer of `f32` waveform samples and draw them as a thin quad strip.

The vertex shader positions each vertex at:
- x: `(sample_index / num_samples) * 2.0 - 1.0`
- y: `sample_value * amplitude_scale`

Two vertices per sample (above/below the waveform line by a small thickness) to form a quad strip.

**Step 3: Create waveform pipeline**

Create `music_vis/src/graphics/waveform_pipeline.rs` similar to `pipeline.rs` but for the waveform. The storage buffer holds waveform sample values, updated each frame.

**Step 4: Integrate into render pass**

In `state.rs`, draw the waveform first (behind), then bars on top. Use alpha blending for the waveform with a low opacity (e.g., 0.3) so bars are clearly visible.

**Step 5: Build and verify visually**

Run with a real file and confirm the waveform renders behind the bars.

**Step 6: Commit**

```bash
git add music_vis/src/graphics/ music_vis/src/audio.rs music_vis/src/app.rs
git commit -m "feat: add waveform overlay behind bar visualization"
```

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 1     | 1-6   | CWT processing binary: fix tests, PCM reader, batch CWT, frame reduction, file format, CLI |
| 2     | 7-11  | Music visualizer: CWT reader, audio playback, bar shader, wiring, waveform overlay |

**Dependencies:** Phase 2 (Task 7+) depends on having a `.cwt` file produced by Phase 1 (Task 6). All tasks within a phase are sequential.

**Scale count:** The number of frequency bins is determined by `log_scales(19.45, 9550.0, 48, center_freq)` — 48 voices per octave across ~9 octaves yields ~430 bins. This count flows through the `.cwt` file header and is used dynamically by the visualizer's shader and pipeline. No hardcoded bar counts anywhere.

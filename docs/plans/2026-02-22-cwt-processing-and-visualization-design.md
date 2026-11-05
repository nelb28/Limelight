# CWT Audio Processing & Visualization Design

## Overview

Process stereo audio files using the Continuous Wavelet Transform and visualize
the output as a bar/waveform hybrid in a music player. Processing is offline
(not real-time). The pre-computed CWT data is played back in sync with audio.

## Architecture

Two separate Rust binaries connected by a `.cwt` file format:

- **`cwt`** — CLI tool that reads a stereo PCM file, downmixes to mono, runs
  the full CWT, and writes a `.cwt` binary file.
- **`music_vis`** — Loads a `.cwt` file and the original audio, plays audio via
  rodio, and renders a bar/waveform visualization driven by CWT magnitudes.

## CWT Processing Pipeline (`cwt` binary)

### Input

Raw stereo PCM file: 96 kHz, 24-bit, interleaved L/R samples (produced by the
existing `pcm_converter.py`).

### Processing Steps

1. **Read PCM** — Parse interleaved 24-bit stereo samples into f32 L and R
   channels.
2. **Downmix to mono** — `mono[i] = (left[i] + right[i]) / 2.0`
3. **CWT convolution** — For each logarithmically-spaced scale (48 voices per
   octave, 19.45 Hz to 9550 Hz, yielding ~430 total frequency bins), convolve
   the full mono signal with the corresponding Morlet wavelet kernel (omega0=6)
   using FFT-based convolution.
4. **Magnitude extraction** — Take `abs()` of each complex CWT coefficient to
   get the magnitude envelope per scale.
5. **Frame reduction with overlap** — Reduce per-sample magnitudes to per-frame
   values at 60 fps:
   - Hop size: 1,600 samples (96000 / 60)
   - Averaging window: 3,200 samples (50% overlap)
   - Each frame value = mean magnitude over its 3,200-sample window centered on
     the frame's time position
6. **Write output** — Save to `.cwt` binary file.

### CLI Interface

```
cwt <input.pcm> <output.cwt>
```

If output path is omitted, writes to `<input>.cwt` alongside the input file.

## CWT File Format

```
Header:
  magic:       [u8; 4]           = b"CWT1"
  num_scales:  u32               = ~430 (48 voices/octave across ~9 octaves)
  num_frames:  u32               = (computed from signal length and frame rate)
  sample_rate: u32               = 96000
  frame_rate:  u32               = 60
  scales:      [f32; num_scales] = (frequency in Hz for each scale)

Body:
  frames: [f32; num_scales * num_frames]
  Row-major layout: frame0_scale0 .. frame0_scaleN, frame1_scale0 .. etc.
```

## Visualization & Playback (`music_vis` binary)

### Input

A `.cwt` file and the original audio file (FLAC format).

### CLI Interface

```
music_vis <song.cwt> <song.flac>
```

### Audio Playback

- Uses `rodio` (with `symphonia` backend) to decode and play FLAC audio.
- Tracks playback position in samples/time to derive the current CWT frame
  index.

### Visualization

**Bar display:**
- ~430 vertical bars (48 voices per octave), one per CWT scale, arranged
  left-to-right from low frequency to high frequency. At 1920px wide, each bar
  is ~4.5px — a dense, detailed spectrogram-like appearance.
- Bar height driven by CWT magnitude at the current frame for that scale.
- Color mapped by frequency band (warm colors for lows, cool for highs).
- Linear interpolation between current and next frame magnitudes based on exact
  audio position for fluid motion.

**Waveform overlay:**
- Rolling window of the raw audio signal near the current playback position.
- Rendered behind or above the bars as a secondary visual layer.

### Rendering

- wgpu-based GPU rendering (infrastructure already in place).
- Bars: instanced quads, one draw call. Height and color set per-instance via
  vertex buffer updated each frame.
- Waveform: line strip or thin quad strip.
- Simple vertex + fragment shaders.

### Controls

- Spacebar: play/pause
- Left/right arrows: seek
- Escape / window close: exit

## Parameters

| Parameter        | Value                |
|------------------|----------------------|
| Sample rate      | 96,000 Hz            |
| Bit depth        | 24-bit               |
| Channel mix      | (L+R)/2 mono         |
| Wavelet          | Morlet (omega0=6)    |
| Voices per octave| 48                   |
| Total scales     | ~430 (dynamic)       |
| Frequency range  | 19.45 Hz - 9,550 Hz |
| Frame rate       | 60 fps               |
| Hop size         | 1,600 samples        |
| Window size      | 3,200 samples        |
| Overlap          | 50%                  |

//! Peak re-synthesis: the last processing step, after all filters.
//!
//! Each frame of the filtered data is reduced to its N most prominent peaks, and the
//! frame is redrawn from scratch as the CWT response those peaks would have if each were
//! a pure sine tone. Everything else (noise, floor, smeared shoulders) is dropped, so
//! only what the filter chain picked out is shown.

use crate::cwt_file::CwtFile;
use crate::filters::find_peaks;

/// Morlet center frequency used by the `cwt` tool (`Morlet::new(6.0)`).
const OMEGA0: f32 = 6.0;
/// Peaks are looked for among at most this many per frame before merging.
const CANDIDATES: usize = 48;
/// Normalization never divides by less than this fraction of the song's loudest peak
/// (-40 dB), so near-silent frames aren't blown up to full height.
const NORM_FLOOR: f32 = 0.01;

pub const PEAKS_HELP: &str = "\
Peak re-synthesis (after all filters):
  --peaks N[,secs,merge,width]   redraw each frame as its N strongest peaks only [-,0.5,0,1]
      N      peaks kept per frame (1-16)
      secs   window for normalizing heights: each peak is divided by the loudest peak
             within +/- secs/2, so quiet passages reach the same height (0 = off)
      merge  peaks closer than this many octaves are combined into one at their
             amplitude-weighted position (0 = off; e.g. 0.1 ~ a whole tone)
      width  lobe width relative to a real sine tone's CWT response (<1 = sharper)";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resynth {
    pub count: usize,
    pub secs: f32,
    pub merge: f32,
    pub width: f32,
}

impl Resynth {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
        let num = |i: usize, default: f32, name: &str| -> Result<f32, String> {
            match parts.get(i) {
                None | Some(&"") => Ok(default),
                Some(v) => v.parse::<f32>().map_err(|_| format!("--peaks: bad {name} '{v}'")),
            }
        };
        let count = parts[0]
            .parse::<usize>()
            .map_err(|_| format!("--peaks: expected a peak count, got '{}'", parts[0]))?;
        let r = Resynth { count, secs: num(1, 0.5, "secs")?, merge: num(2, 0.0, "merge")?, width: num(3, 1.0, "width")? };
        if parts.len() > 4
            || !(1..=16).contains(&r.count)
            || !(r.secs >= 0.0 && r.secs.is_finite())
            || !(r.merge >= 0.0 && r.merge <= 2.0)
            || !(r.width > 0.0 && r.width <= 4.0)
        {
            return Err(format!("--peaks {spec}: parameter out of range (see --help)"));
        }
        Ok(r)
    }
}

/// A peak to draw: fractional bin position and amplitude.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tone {
    pos: f32,
    amp: f32,
}

/// The `count` strongest peaks of one frame, after merging peaks within `merge_bins`.
/// Greedy: the strongest remaining peak absorbs every remaining peak within reach,
/// at the amplitude-weighted mean position and the combined (energy-sum) amplitude.
fn select(row: &[f32], count: usize, merge_bins: f32) -> Vec<Tone> {
    let mut peaks: Vec<Tone> = find_peaks(row, CANDIDATES).iter().map(|p| Tone { pos: p.pos, amp: p.amp }).collect();
    peaks.sort_by(|a, b| b.amp.total_cmp(&a.amp));
    let mut out = Vec::with_capacity(count);
    let mut used = vec![false; peaks.len()];
    for i in 0..peaks.len() {
        if out.len() == count {
            break;
        }
        if used[i] {
            continue;
        }
        used[i] = true;
        let (mut wsum, mut psum, mut e) = (peaks[i].amp, peaks[i].amp * peaks[i].pos, peaks[i].amp * peaks[i].amp);
        if merge_bins > 0.0 {
            for j in i + 1..peaks.len() {
                if !used[j] && (peaks[j].pos - peaks[i].pos).abs() <= merge_bins {
                    used[j] = true;
                    wsum += peaks[j].amp;
                    psum += peaks[j].amp * peaks[j].pos;
                    e += peaks[j].amp * peaks[j].amp;
                }
            }
        }
        out.push(Tone { pos: psum / wsum, amp: e.sqrt() });
    }
    out
}

/// Replace the CWT data with the re-synthesized peaks (and matching phase deviation:
/// a sine at f reads as f at every bin it reaches, i.e. 1200*log2(f / bin center) cents).
pub fn apply(cwt: &mut CwtFile, r: &Resynth) {
    let ns = cwt.header.num_scales as usize;
    let nf = cwt.header.num_frames as usize;
    let freqs = cwt.header.frequencies.clone();
    let bpo = if ns > 1 && freqs[ns - 1] > freqs[0] { (ns - 1) as f32 / (freqs[ns - 1] / freqs[0]).log2() } else { 48.0 };

    let tones: Vec<Vec<Tone>> =
        (0..nf).map(|f| select(&cwt.data[f * ns..(f + 1) * ns], r.count, r.merge * bpo)).collect();

    // Normalization: divide by the loudest peak in a short window around each frame.
    let loudest: Vec<f32> = tones.iter().map(|t| t.iter().map(|p| p.amp).fold(0.0, f32::max)).collect();
    let floor = loudest.iter().cloned().fold(0.0, f32::max) * NORM_FLOOR;
    let half = (r.secs / 2.0 * cwt.header.frame_rate as f32).round() as usize;
    let norm: Vec<f32> = (0..nf)
        .map(|f| {
            if r.secs == 0.0 {
                return 1.0;
            }
            let (lo, hi) = (f.saturating_sub(half), (f + half + 1).min(nf));
            loudest[lo..hi].iter().cloned().fold(floor, f32::max).max(f32::MIN_POSITIVE)
        })
        .collect();

    let k = OMEGA0 * OMEGA0 / (2.0 * r.width * r.width);
    let has_phase = !cwt.phase_dev.is_empty();
    for f in 0..nf {
        let mags = &mut cwt.data[f * ns..(f + 1) * ns];
        mags.fill(0.0);
        let mut devs = vec![0.0f32; ns];
        for t in &tones[f] {
            let amp = t.amp / norm[f];
            let freq = freqs[0] * 2f32.powf(t.pos / bpo);
            for s in 0..ns {
                let ratio = freq / freqs[s];
                let e = k * (ratio - 1.0) * (ratio - 1.0);
                if e > 12.0 {
                    continue;
                }
                // Morlet response to a sine, relative to its value at the matching bin.
                let v = amp * ratio.sqrt() * (-e).exp();
                // Overlapping lobes: the stronger one wins, so each peak keeps its height.
                if v > mags[s] {
                    mags[s] = v;
                    devs[s] = 1200.0 * ratio.log2();
                }
            }
        }
        if has_phase {
            cwt.phase_dev[f * ns..(f + 1) * ns].copy_from_slice(&devs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwt_from(rows: &[Vec<f32>]) -> CwtFile {
        let ns = rows[0].len();
        let freqs: Vec<f32> = (0..ns).map(|i| 20.0 * 2f32.powf(i as f32 / 48.0)).collect();
        let data: Vec<f32> = rows.concat();
        let phase = vec![0.0; data.len()];
        CwtFile::new(ns as u32, rows.len() as u32, 48000, 60, freqs, data, phase)
    }

    fn lobe(row: &mut [f32], center: f32, amp: f32) {
        for (s, v) in row.iter_mut().enumerate() {
            let d = (s as f32 - center) / 4.0;
            *v += amp * (-0.5 * d * d).exp();
        }
    }

    fn local_maxima(r: &[f32]) -> Vec<usize> {
        (1..r.len() - 1).filter(|&s| r[s] > r[s - 1] && r[s] >= r[s + 1] && r[s] > 0.01).collect()
    }

    #[test]
    fn parse_spec() {
        assert_eq!(Resynth::parse("6").unwrap(), Resynth { count: 6, secs: 0.5, merge: 0.0, width: 1.0 });
        assert_eq!(Resynth::parse("4,0,0.1,0.5").unwrap(), Resynth { count: 4, secs: 0.0, merge: 0.1, width: 0.5 });
        assert!(Resynth::parse("0").is_err());
        assert!(Resynth::parse("17").is_err());
        assert!(Resynth::parse("4,-1").is_err());
        assert!(Resynth::parse("x").is_err());
    }

    #[test]
    fn keeps_n_strongest_drops_noise() {
        let mut row = vec![0.0f32; 300];
        // Noise floor with small ripples, plus three clear tones.
        for (s, v) in row.iter_mut().enumerate() {
            *v = 0.05 + 0.02 * ((s as f32) * 0.9).sin();
        }
        lobe(&mut row, 60.0, 1.0);
        lobe(&mut row, 150.0, 0.7);
        lobe(&mut row, 240.0, 0.3);
        let mut cwt = cwt_from(&[row]);
        apply(&mut cwt, &Resynth { count: 2, secs: 0.0, merge: 0.0, width: 1.0 });
        let peaks = local_maxima(&cwt.data);
        assert_eq!(peaks.len(), 2, "{peaks:?}");
        assert!((peaks[0] as i32 - 60).abs() <= 1 && (peaks[1] as i32 - 150).abs() <= 1, "{peaks:?}");
        assert!((cwt.data[60] - 1.0).abs() < 0.1, "height {}", cwt.data[60]);
        assert!(cwt.data[240] < 1e-3, "third tone not dropped: {}", cwt.data[240]);
        // Phase: on pitch at the peak, sharp below it (bin center lower than the tone).
        assert!(cwt.phase_dev[60].abs() < 30.0 && cwt.phase_dev[55] > 0.0);
    }

    #[test]
    fn merges_close_peaks() {
        let mut row = vec![0.0f32; 300];
        lobe(&mut row, 100.0, 1.0);
        lobe(&mut row, 112.0, 1.0); // a quarter octave apart
        let r = |merge| Resynth { count: 4, secs: 0.0, merge, width: 0.5 };
        let mut apart = cwt_from(&[row.clone()]);
        apply(&mut apart, &r(0.0));
        assert_eq!(local_maxima(&apart.data).len(), 2);
        let mut merged = cwt_from(&[row]);
        apply(&mut merged, &r(0.3));
        let peaks = local_maxima(&merged.data);
        assert_eq!(peaks.len(), 1, "{peaks:?}");
        assert!((peaks[0] as i32 - 106).abs() <= 1, "merged at {}", peaks[0]);
    }

    #[test]
    fn normalizes_over_window() {
        // A loud frame then a quiet one, 1 s apart; a 0.5 s window treats them separately.
        let mut loud = vec![0.0f32; 200];
        lobe(&mut loud, 80.0, 1.0);
        let mut quiet = vec![0.0f32; 200];
        lobe(&mut quiet, 80.0, 0.1);
        let mut rows = vec![loud; 60];
        rows.extend(vec![quiet; 60]);
        let mut cwt = cwt_from(&rows);
        apply(&mut cwt, &Resynth { count: 1, secs: 0.5, merge: 0.0, width: 1.0 });
        let at = |f: usize| cwt.data[f * 200 + 80];
        assert!((at(10) - 1.0).abs() < 0.05 && (at(110) - 1.0).abs() < 0.05, "{} {}", at(10), at(110));
        // Just after the change the loud frames are still in the window.
        assert!(at(61) < 0.2, "{}", at(61));
    }
}

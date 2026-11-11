//! Experimental filters applied to the CWT data at load time, before GPU upload.
//!
//! Filters run in the order given on the command line, each one transforming the
//! output of the previous. Magnitude filters change `data` (which drives the bars,
//! the magnitude spectrogram and the brightness of the phase spectrogram). Filters
//! named `phase-*` change `phase_dev` (cents), weighting by the current magnitudes.
//!
//! Spec syntax: `name` or `name:param` or `name:param1,param2`.

use crate::cwt_file::CwtFile;

const EPS: f32 = 1e-12;

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    /// Divide each frame by the song-mean spectrum smoothed over ±octaves (envelope whitening).
    Tilt { octaves: f32 },
    /// Fade out frames whose original (unfiltered) peak level is below `db` (relative
    /// to the 99th-percentile frame level), with a linear ramp of `knee` dB.
    Gate { db: f32, knee: f32 },
    /// Divide each bin by its mean (whole song if secs == 0, else rolling window).
    Whiten { secs: f32 },
    /// Centered moving average along time.
    SmoothTime { secs: f32 },
    /// Centered moving average across neighboring frequency bins.
    SmoothFreq { bins: usize },
    /// Divide each frame by the rolling max loudness (max over bins) in a window.
    Agc { secs: f32 },
    /// Positive log-magnitude increase vs. `secs` earlier (onsets / spectral flux).
    Flux { secs: f32 },
    /// Keep only what exceeds the local (rolling-mean) background: x - mean, >= 0.
    Novelty { secs: f32 },
    /// HPSS soft mask keeping horizontal (sustained, tonal) structure.
    Harmonic { secs: f32, bins: usize },
    /// HPSS soft mask keeping vertical (transient, percussive) structure.
    Percussive { secs: f32, bins: usize },
    /// Comb average of x(t + k*period), k = -count..=count: emphasizes repetition at `period`.
    Periodic { period: f32, count: usize },
    /// Per frame, subtract k * (median across bins) and clamp at 0: removes the floor.
    Contrast { k: f32 },
    /// Find prominent peaks per frame, match them by frequency ratio, and move `amount`
    /// of each harmonic peak (2..=count) into its fundamental. Peak/match decisions are
    /// made on data smoothed over `secs` so they don't flicker.
    Fundamental { amount: f32, count: usize, secs: f32 },
    /// Raise magnitudes to a power (< 1 compresses, > 1 expands contrast).
    Power { exp: f32 },
    /// Magnitude-weighted moving average of phase deviation along time.
    PhaseSmooth { secs: f32 },
    /// Magnitude-weighted rolling standard deviation of phase deviation (pitch wobble).
    PhaseVar { secs: f32 },
    /// Phase deviation minus its magnitude-weighted rolling mean (bends vs. steady pitch).
    PhaseDetrend { secs: f32 },
}

pub const FILTER_HELP: &str = "\
Filters (repeat --filter/-f to chain; applied in order):
  Magnitude filters (affect bars, magnitude graph, and phase-graph brightness)
    tilt[:octaves]           divide by the song's average spectrum smoothed   [2]
                             over +-octaves: evens out the bass-heavy slope but
                             keeps steady content like a bass line
    whiten[:secs]            divide each bin by its own mean (0 = whole song)  [0]
                             (cancels steady content and boosts rarely-used
                             bins in quiet passages; tilt is gentler)
    gate[:db,knee]           fade out frames quieter than db in the original  [-15,6]
                             audio (dB vs. loud frames, linear knee)
    smooth-time[:secs]       moving average along time                        [0.1]
    smooth-freq[:bins]       moving average across bins (half-width)          [2]
    agc[:secs]               normalize by rolling max loudness                [3]
    flux[:secs]              positive log-magnitude rise vs. secs earlier     [0.05]
    novelty[:secs]           excess over rolling-mean background              [1]
    harmonic[:secs,bins]     HPSS: keep sustained/tonal horizontal lines      [0.25,8]
    percussive[:secs,bins]   HPSS: keep transient vertical lines              [0.25,8]
    periodic:period[,count]  comb average at a repetition period (seconds)    [-,2]
    contrast[:k]             per frame, subtract k * median across bins       [1]
                             (removes the raised floor left by whiten)
    fundamental[:amount,n,secs]  find prominent peaks, match them by frequency [0.8,6,0.15]
                             ratio, move `amount` (0-1) of harmonic peaks 2..n into
                             their fundamental peak; decisions use data smoothed
                             over secs so they don't flicker (0 = off).
                             Works anywhere in the chain (all measures are relative).
    power[:exp]              magnitude ^ exp; > 1 sharpens peaks              [0.5]
  Phase-deviation filters (cents; weighted by current magnitudes)
    phase-smooth[:secs]      moving average along time                        [0.25]
    phase-var[:secs]         rolling std dev (wobble); red = unstable         [0.5]
    phase-detrend[:secs]     subtract rolling mean (bends vs. steady pitch)   [1]

Display options:
  --scale db|linear|sqrt     magnitude color scale                            [db]
  --db-range N               dB below max shown as black (db scale)           [50]
  --dev-range N              cents mapped to full blue/red                    [50]
  --peak-pct P               scale to the P-th percentile instead of the max  [100]
                             (louder bars/colors; values above P clamp to full)

Examples:
  music_vis song.flac song.cwt -f whiten -f harmonic:0.3 -f smooth-time:0.1 --scale sqrt
  music_vis song.flac song.cwt -f tilt -f contrast -f gate --peak-pct 99.5";

fn parse_params(spec: &str) -> (&str, Vec<&str>) {
    match spec.split_once(':') {
        Some((name, rest)) => (name, rest.split(',').map(str::trim).collect()),
        None => (spec, Vec::new()),
    }
}

fn param<T: std::str::FromStr>(params: &[&str], i: usize, default: Option<T>, name: &str) -> Result<T, String> {
    match params.get(i) {
        Some(p) if !p.is_empty() => p.parse().map_err(|_| format!("{name}: bad parameter '{p}'")),
        _ => default.ok_or_else(|| format!("{name}: missing required parameter #{}", i + 1)),
    }
}

impl Filter {
    pub fn parse(spec: &str) -> Result<Filter, String> {
        let (name, p) = parse_params(spec.trim());
        let max_params = match name {
            "harmonic" | "percussive" | "periodic" | "gate" => 2,
            "fundamental" => 3,
            _ => 1,
        };
        if p.len() > max_params {
            return Err(format!("{name}: too many parameters"));
        }
        let f = match name {
            "tilt" => Filter::Tilt { octaves: param(&p, 0, Some(2.0), name)? },
            "gate" => Filter::Gate {
                db: param(&p, 0, Some(-15.0), name)?,
                knee: param(&p, 1, Some(6.0), name)?,
            },
            "whiten" => Filter::Whiten { secs: param(&p, 0, Some(0.0), name)? },
            "smooth-time" => Filter::SmoothTime { secs: param(&p, 0, Some(0.1), name)? },
            "smooth-freq" => Filter::SmoothFreq { bins: param(&p, 0, Some(2), name)? },
            "agc" => Filter::Agc { secs: param(&p, 0, Some(3.0), name)? },
            "flux" => Filter::Flux { secs: param(&p, 0, Some(0.05), name)? },
            "novelty" => Filter::Novelty { secs: param(&p, 0, Some(1.0), name)? },
            "harmonic" => Filter::Harmonic {
                secs: param(&p, 0, Some(0.25), name)?,
                bins: param(&p, 1, Some(8), name)?,
            },
            "percussive" => Filter::Percussive {
                secs: param(&p, 0, Some(0.25), name)?,
                bins: param(&p, 1, Some(8), name)?,
            },
            "periodic" => Filter::Periodic {
                period: param(&p, 0, None, name)?,
                count: param(&p, 1, Some(2), name)?,
            },
            "contrast" => Filter::Contrast { k: param(&p, 0, Some(1.0), name)? },
            "fundamental" => Filter::Fundamental {
                amount: param(&p, 0, Some(0.8), name)?,
                count: param(&p, 1, Some(6), name)?,
                secs: param(&p, 2, Some(0.15), name)?,
            },
            "power" => Filter::Power { exp: param(&p, 0, Some(0.5), name)? },
            "phase-smooth" => Filter::PhaseSmooth { secs: param(&p, 0, Some(0.25), name)? },
            "phase-var" => Filter::PhaseVar { secs: param(&p, 0, Some(0.5), name)? },
            "phase-detrend" => Filter::PhaseDetrend { secs: param(&p, 0, Some(1.0), name)? },
            _ => return Err(format!("unknown filter '{name}' (see --list-filters)")),
        };
        f.validate()?;
        Ok(f)
    }

    fn validate(&self) -> Result<(), String> {
        let neg = |v: f32| v < 0.0 || !v.is_finite();
        let bad = match *self {
            Filter::Whiten { secs } => neg(secs),
            Filter::SmoothTime { secs }
            | Filter::Agc { secs }
            | Filter::Flux { secs }
            | Filter::Novelty { secs }
            | Filter::PhaseSmooth { secs }
            | Filter::PhaseVar { secs }
            | Filter::PhaseDetrend { secs } => neg(secs) || secs == 0.0,
            Filter::Harmonic { secs, bins } | Filter::Percussive { secs, bins } => {
                neg(secs) || secs == 0.0 || bins == 0
            }
            Filter::Periodic { period, count } => neg(period) || period == 0.0 || count == 0,
            Filter::Power { exp } => neg(exp) || exp == 0.0,
            Filter::Contrast { k } => neg(k) || k == 0.0,
            Filter::SmoothFreq { .. } => false,
            Filter::Fundamental { amount, count, secs } => {
                neg(amount) || amount == 0.0 || amount > 1.0 || count < 2 || neg(secs)
            }
            Filter::Tilt { octaves } => neg(octaves) || octaves == 0.0,
            Filter::Gate { db, knee } => !db.is_finite() || neg(knee) || knee == 0.0,
        };
        if bad { Err(format!("{self:?}: parameter out of range (see --list-filters)")) } else { Ok(()) }
    }

    pub fn is_phase(&self) -> bool {
        matches!(self, Filter::PhaseSmooth { .. } | Filter::PhaseVar { .. } | Filter::PhaseDetrend { .. })
    }
}

/// Frame-major grid view: data[f * ns + s].
struct Grid {
    nf: usize,
    ns: usize,
    frame_rate: f32,
    bins_per_octave: f32,
}

impl Grid {
    fn frames(&self, secs: f32) -> usize {
        ((secs * self.frame_rate).round() as usize).max(1)
    }
}

/// Centered moving average along time for every bin, using prefix sums.
/// `half` frames on each side; edges use the frames available.
fn moving_avg_time(g: &Grid, x: &[f32], half: usize) -> Vec<f32> {
    let mut out = vec![0.0; x.len()];
    let mut prefix = vec![0.0f64; g.nf + 1];
    for s in 0..g.ns {
        for f in 0..g.nf {
            prefix[f + 1] = prefix[f] + x[f * g.ns + s] as f64;
        }
        for f in 0..g.nf {
            let lo = f.saturating_sub(half);
            let hi = (f + half + 1).min(g.nf);
            out[f * g.ns + s] = ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32;
        }
    }
    out
}

/// Centered median along time (per bin) or frequency (per frame).
fn median_filter(g: &Grid, x: &[f32], half: usize, along_time: bool) -> Vec<f32> {
    let mut out = vec![0.0; x.len()];
    let mut buf = Vec::with_capacity(2 * half + 1);
    let (outer, inner) = if along_time { (g.ns, g.nf) } else { (g.nf, g.ns) };
    let idx = |o: usize, i: usize| if along_time { i * g.ns + o } else { o * g.ns + i };
    for o in 0..outer {
        for i in 0..inner {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(inner);
            buf.clear();
            buf.extend((lo..hi).map(|j| x[idx(o, j)]));
            let mid = buf.len() / 2;
            let (_, m, _) = buf.select_nth_unstable_by(mid, f32::total_cmp);
            out[idx(o, i)] = *m;
        }
    }
    out
}

/// `original` is the unfiltered magnitude data (used by `gate`).
fn apply_magnitude(g: &Grid, filter: &Filter, x: &mut Vec<f32>, original: &[f32]) {
    match *filter {
        Filter::Tilt { octaves } => {
            // Geometric moving average of the song-mean spectrum across frequency.
            let mut log_mean = vec![0.0f64; g.ns];
            for f in 0..g.nf {
                for s in 0..g.ns {
                    log_mean[s] += x[f * g.ns + s] as f64;
                }
            }
            for v in &mut log_mean {
                *v = (*v / g.nf.max(1) as f64 + EPS as f64).ln();
            }
            let half = (octaves * g.bins_per_octave).round() as usize;
            let env: Vec<f32> = (0..g.ns)
                .map(|s| {
                    let lo = s.saturating_sub(half);
                    let hi = (s + half + 1).min(g.ns);
                    (log_mean[lo..hi].iter().sum::<f64>() / (hi - lo) as f64).exp() as f32
                })
                .collect();
            for (i, v) in x.iter_mut().enumerate() {
                *v /= env[i % g.ns].max(EPS);
            }
        }
        Filter::Gate { db, knee } => {
            let levels: Vec<f32> = (0..g.nf)
                .map(|f| original[f * g.ns..(f + 1) * g.ns].iter().cloned().fold(0.0, f32::max))
                .collect();
            let mut sorted = levels.clone();
            let rank = ((0.99 * sorted.len() as f32).ceil() as usize).clamp(1, sorted.len().max(1)) - 1;
            let reference = if sorted.is_empty() {
                1.0
            } else {
                sorted.select_nth_unstable_by(rank, f32::total_cmp).1.max(EPS)
            };
            for f in 0..g.nf {
                let level_db = 20.0 * (levels[f] / reference).max(1e-12).log10();
                let w = ((level_db - (db - knee)) / knee).clamp(0.0, 1.0);
                for v in &mut x[f * g.ns..(f + 1) * g.ns] {
                    *v *= w;
                }
            }
        }
        Filter::Whiten { secs } => {
            let mean = if secs == 0.0 {
                let mut m = vec![0.0f64; g.ns];
                for f in 0..g.nf {
                    for s in 0..g.ns {
                        m[s] += x[f * g.ns + s] as f64;
                    }
                }
                let m: Vec<f32> = m.iter().map(|v| (v / g.nf.max(1) as f64) as f32).collect();
                (0..x.len()).map(|i| m[i % g.ns]).collect()
            } else {
                moving_avg_time(g, x, g.frames(secs / 2.0))
            };
            for (v, m) in x.iter_mut().zip(mean) {
                *v /= m.max(EPS);
            }
        }
        Filter::SmoothTime { secs } => *x = moving_avg_time(g, x, g.frames(secs / 2.0)),
        Filter::SmoothFreq { bins } => {
            let mut out = vec![0.0; x.len()];
            for f in 0..g.nf {
                let row = &x[f * g.ns..(f + 1) * g.ns];
                for s in 0..g.ns {
                    let lo = s.saturating_sub(bins);
                    let hi = (s + bins + 1).min(g.ns);
                    out[f * g.ns + s] = row[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
                }
            }
            *x = out;
        }
        Filter::Agc { secs } => {
            let loud: Vec<f32> = (0..g.nf)
                .map(|f| x[f * g.ns..(f + 1) * g.ns].iter().cloned().fold(0.0, f32::max))
                .collect();
            let half = g.frames(secs / 2.0);
            for f in 0..g.nf {
                let lo = f.saturating_sub(half);
                let hi = (f + half + 1).min(g.nf);
                let peak = loud[lo..hi].iter().cloned().fold(0.0, f32::max).max(EPS);
                for v in &mut x[f * g.ns..(f + 1) * g.ns] {
                    *v /= peak;
                }
            }
        }
        Filter::Flux { secs } => {
            let lag = g.frames(secs);
            let floor = x.iter().cloned().fold(0.0, f32::max) * 1e-4 + EPS;
            let log: Vec<f32> = x.iter().map(|v| (v + floor).ln()).collect();
            for f in 0..g.nf {
                for s in 0..g.ns {
                    let i = f * g.ns + s;
                    x[i] = if f >= lag { (log[i] - log[i - lag * g.ns]).max(0.0) } else { 0.0 };
                }
            }
        }
        Filter::Novelty { secs } => {
            let bg = moving_avg_time(g, x, g.frames(secs / 2.0));
            for (v, b) in x.iter_mut().zip(bg) {
                *v = (*v - b).max(0.0);
            }
        }
        Filter::Harmonic { secs, bins } | Filter::Percussive { secs, bins } => {
            // Fitzgerald HPSS: median along time keeps horizontal lines (harmonic),
            // median along frequency keeps vertical lines (percussive); Wiener soft mask.
            let h = median_filter(g, x, g.frames(secs / 2.0), true);
            let p = median_filter(g, x, bins, false);
            let harmonic = matches!(filter, Filter::Harmonic { .. });
            for i in 0..x.len() {
                let (h2, p2) = (h[i] * h[i], p[i] * p[i]);
                let keep = if harmonic { h2 } else { p2 };
                x[i] *= keep / (h2 + p2 + EPS);
            }
        }
        Filter::Periodic { period, count } => {
            let lag = g.frames(period);
            let mut out = vec![0.0; x.len()];
            for f in 0..g.nf {
                let mut n = 0;
                for k in -(count as i64)..=(count as i64) {
                    let t = f as i64 + k * lag as i64;
                    if t < 0 || t >= g.nf as i64 {
                        continue;
                    }
                    n += 1;
                    let t = t as usize;
                    for s in 0..g.ns {
                        out[f * g.ns + s] += x[t * g.ns + s];
                    }
                }
                for v in &mut out[f * g.ns..(f + 1) * g.ns] {
                    *v /= n as f32;
                }
            }
            *x = out;
        }
        Filter::Contrast { k } => {
            let mut row = Vec::with_capacity(g.ns);
            for f in 0..g.nf {
                let frame = &mut x[f * g.ns..(f + 1) * g.ns];
                row.clear();
                row.extend_from_slice(frame);
                let mid = row.len() / 2;
                let floor = *row.select_nth_unstable_by(mid, f32::total_cmp).1 * k;
                for v in frame.iter_mut() {
                    *v = (*v - floor).max(0.0);
                }
            }
        }
        Filter::Fundamental { amount, count, secs } => {
            // Decide peaks and harmonic matches on a time-smoothed copy so decisions don't
            // flicker frame to frame; the gains themselves stay sharp.
            let decide = if secs > 0.0 { moving_avg_time(g, x, g.frames(secs / 2.0)) } else { x.clone() };
            let mut gains = Vec::with_capacity(x.len());
            for f in 0..g.nf {
                gains.extend(fold_gains(&decide[f * g.ns..(f + 1) * g.ns], g.bins_per_octave, amount, count));
            }
            // Light smoothing of the gains as well, so the remaining on/off switches fade.
            if secs > 0.0 {
                gains = moving_avg_time(g, &gains, g.frames(secs / 6.0));
            }
            for (v, gain) in x.iter_mut().zip(gains) {
                *v *= gain;
            }
        }
        Filter::Power { exp } => x.iter_mut().for_each(|v| *v = v.max(0.0).powf(exp)),
        _ => unreachable!("phase filter passed to apply_magnitude"),
    }
}

/// Peaks must stand this many dB above the higher of their two flanking dips.
const PEAK_PROMINENCE_DB: f32 = 3.0;
/// Peaks this far below the frame's loudest peak are ignored.
const PEAK_FLOOR_DB: f32 = 30.0;
/// At most this many (strongest) peaks per frame take part.
const MAX_PEAKS: usize = 16;
/// A peak counts as harmonic k if its ratio to the root is within this of k.
const HARMONIC_TOLERANCE_CENTS: f32 = 35.0;
/// A root must be at least this fraction of its harmonic's amplitude (-20 dB);
/// stops a faint bin below a loud note from absorbing it.
const MIN_ROOT_RATIO: f32 = 0.1;

#[derive(Debug, Clone)]
pub(crate) struct Peak {
    /// Fractional bin position (parabolic interpolation on log amplitude).
    pub pos: f32,
    pub amp: f32,
    /// Inclusive bin range scaled when the peak gives or receives amplitude.
    lo: usize,
    hi: usize,
}

/// Prominent spectral peaks of one frame, sorted by frequency, with non-overlapping regions.
pub(crate) fn find_peaks(row: &[f32], max_peaks: usize) -> Vec<Peak> {
    let ns = row.len();
    let frame_max = row.iter().cloned().fold(0.0, f32::max);
    if ns < 3 || frame_max <= EPS {
        return Vec::new();
    }
    let floor = frame_max * 10f32.powf(-PEAK_FLOOR_DB / 20.0);
    let min_ratio = 10f32.powf(PEAK_PROMINENCE_DB / 20.0);

    let mut peaks: Vec<(usize, f32)> = Vec::new(); // (bin, prominence base)
    for s in 0..ns {
        let v = row[s];
        let left_ok = s == 0 || v > row[s - 1];
        let right_ok = s == ns - 1 || v >= row[s + 1];
        if !(left_ok && right_ok) || v < floor {
            continue;
        }
        // Standard prominence: lowest point on each side before reaching higher ground.
        let mut left_min = v;
        let mut i = s;
        while i > 0 && row[i - 1] <= v {
            i -= 1;
            left_min = left_min.min(row[i]);
        }
        let mut right_min = v;
        let mut j = s;
        while j + 1 < ns && row[j + 1] <= v {
            j += 1;
            right_min = right_min.min(row[j]);
        }
        // A peak on a spectrum edge has no dip on that side: use the other side only.
        let base = if s == 0 {
            right_min
        } else if s == ns - 1 {
            left_min
        } else {
            left_min.max(right_min)
        };
        if v >= base.max(EPS) * min_ratio || base <= EPS {
            peaks.push((s, base));
        }
    }

    // Keep the strongest, then restore frequency order.
    peaks.sort_by(|a, b| row[b.0].total_cmp(&row[a.0]));
    peaks.truncate(max_peaks);
    peaks.sort_by_key(|p| p.0);

    let mut out: Vec<Peak> = Vec::with_capacity(peaks.len());
    for (k, &(s, base)) in peaks.iter().enumerate() {
        // Inner boundaries: deepest dip between adjacent kept peaks (assigned to this side
        // or the next by position). Outer boundaries: walk down while above the base level.
        let lo = if k > 0 {
            let prev = peaks[k - 1].0;
            (prev + 1..=s).min_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap()
        } else {
            let mut i = s;
            while i > 0 && row[i - 1] > base && row[i - 1] <= row[i] {
                i -= 1;
            }
            i
        };
        let hi = if k + 1 < peaks.len() {
            let next = peaks[k + 1].0;
            (s..next).min_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap()
        } else {
            let mut j = s;
            while j + 1 < ns && row[j + 1] > base && row[j + 1] <= row[j] {
                j += 1;
            }
            j
        };
        // The shared dip belongs to the lower peak's region only.
        let lo = if k > 0 && lo == out[k - 1].hi { lo + 1 } else { lo };
        let pos = if s > 0 && s + 1 < ns && row[s - 1] > 0.0 && row[s + 1] > 0.0 {
            let (a, b, c) = (row[s - 1].ln(), row[s].ln(), row[s + 1].ln());
            let denom = a - 2.0 * b + c;
            if denom < 0.0 { s as f32 + (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { s as f32 }
        } else {
            s as f32
        };
        out.push(Peak { pos, amp: row[s], lo: lo.min(s), hi: hi.max(s) });
    }
    out
}

/// Per-bin gains for one frame that move `amount` of each harmonic peak into its root.
///
/// Peaks are visited low to high. A peak is a harmonic of an earlier *root* peak when
/// their frequency ratio is within `HARMONIC_TOLERANCE_CENTS` of an integer k in
/// 2..=count; among candidate roots the best `root amplitude / k` wins. Only roots
/// receive amplitude, so there are no chains (an octave of an octave counts as k = 4).
fn fold_gains(row: &[f32], bins_per_octave: f32, amount: f32, count: usize) -> Vec<f32> {
    let mut gain = vec![1.0f32; row.len()];
    let peaks = find_peaks(row, MAX_PEAKS);
    let cents_per_bin = 1200.0 / bins_per_octave;
    let mut roots: Vec<usize> = Vec::new();
    let mut received = vec![0.0f32; peaks.len()];

    for (j, pj) in peaks.iter().enumerate() {
        let mut best: Option<(usize, f32)> = None;
        for &r in &roots {
            let pr = &peaks[r];
            if pr.amp < MIN_ROOT_RATIO * pj.amp {
                continue;
            }
            let cents = (pj.pos - pr.pos) * cents_per_bin;
            let k = 2f32.powf(cents / 1200.0).round();
            if k < 2.0 || k > count as f32 {
                continue;
            }
            if (cents - 1200.0 * k.log2()).abs() > HARMONIC_TOLERANCE_CENTS {
                continue;
            }
            let score = pr.amp / k;
            if best.map_or(true, |(_, b)| score > b) {
                best = Some((r, score));
            }
        }
        match best {
            Some((r, _)) => {
                // Lower the peak toward the straight line between its region's edge dips:
                // new = line + (1 - amount) * (row - line). The result is a blend of the
                // original curve and a straight line, so it keeps the curve's shape (no
                // crater, no new peaks) and meets the neighbours exactly at the edges
                // (no step, as a uniform cut of the whole region would leave).
                let (ex, top) = excess(row, pj);
                received[r] += amount * top;
                for (s, e) in (pj.lo..=pj.hi).zip(ex) {
                    if row[s] > EPS {
                        gain[s] = 1.0 - amount * e / row[s];
                    }
                }
            }
            None => roots.push(j),
        }
    }
    for (r, &add) in received.iter().enumerate() {
        if add <= 0.0 {
            continue;
        }
        let p = &peaks[r];
        // Add the received amplitude on top of the root's own peak, shaped like the part
        // above its edge line, so the boost is zero at the edges and `add` at the top.
        let (ex, top) = excess(row, p);
        for (s, e) in (p.lo..=p.hi).zip(ex) {
            if row[s] > EPS {
                let shape = if top > 0.01 * p.amp {
                    e / top
                } else {
                    // Flat-topped region (peak on a region edge): fall back to tapering by height.
                    (row[s] / p.amp).min(1.0)
                };
                gain[s] = 1.0 + add * shape / row[s];
            }
        }
    }
    gain
}

/// Part of a peak's region that rises above the straight line between the region's edge
/// values, per bin from `lo` to `hi`, and that excess at the peak's top bin.
/// An edge at the peak itself (peak on the spectrum edge) uses the other edge's level.
fn excess(row: &[f32], p: &Peak) -> (Vec<f32>, f32) {
    let top_bin = (p.pos.round() as usize).clamp(p.lo, p.hi);
    let left = if p.lo < top_bin { row[p.lo] } else { row[p.hi] };
    let right = if p.hi > top_bin { row[p.hi] } else { row[p.lo] };
    let span = (p.hi - p.lo).max(1) as f32;
    let line = |s: usize| left + (right - left) * (s - p.lo) as f32 / span;
    let ex: Vec<f32> = (p.lo..=p.hi).map(|s| (row[s] - line(s)).max(0.0)).collect();
    let top = ex[top_bin - p.lo];
    (ex, top)
}

/// Magnitude-weighted rolling mean and mean-of-squares of `dev` along time.
fn weighted_moments(g: &Grid, dev: &[f32], w: &[f32], half: usize) -> (Vec<f32>, Vec<f32>) {
    let wd: Vec<f32> = dev.iter().zip(w).map(|(d, w)| d * w).collect();
    let wd2: Vec<f32> = dev.iter().zip(w).map(|(d, w)| d * d * w).collect();
    let sw = moving_avg_time(g, w, half);
    let swd = moving_avg_time(g, &wd, half);
    let swd2 = moving_avg_time(g, &wd2, half);
    let mean = swd.iter().zip(&sw).map(|(a, b)| a / b.max(EPS)).collect();
    let sq = swd2.iter().zip(&sw).map(|(a, b)| a / b.max(EPS)).collect();
    (mean, sq)
}

fn apply_phase(g: &Grid, filter: &Filter, dev: &mut Vec<f32>, mags: &[f32]) {
    match *filter {
        Filter::PhaseSmooth { secs } => {
            *dev = weighted_moments(g, dev, mags, g.frames(secs / 2.0)).0;
        }
        Filter::PhaseVar { secs } => {
            let (mean, sq) = weighted_moments(g, dev, mags, g.frames(secs / 2.0));
            *dev = mean.iter().zip(sq).map(|(m, s)| (s - m * m).max(0.0).sqrt()).collect();
        }
        Filter::PhaseDetrend { secs } => {
            let mean = weighted_moments(g, dev, mags, g.frames(secs / 2.0)).0;
            for (d, m) in dev.iter_mut().zip(mean) {
                *d -= m;
            }
        }
        _ => unreachable!("magnitude filter passed to apply_phase"),
    }
}

/// Apply `filters` in order to the CWT data and refresh its global max.
pub fn apply(cwt: &mut CwtFile, filters: &[Filter]) {
    let freqs = &cwt.header.frequencies;
    let ns = cwt.header.num_scales as usize;
    // Bins are log-spaced; derive the density from the stored center frequencies.
    let bins_per_octave = if ns > 1 && freqs[ns - 1] > freqs[0] {
        (ns - 1) as f32 / (freqs[ns - 1] / freqs[0]).log2()
    } else {
        48.0
    };
    let g = Grid {
        nf: cwt.header.num_frames as usize,
        ns,
        frame_rate: cwt.header.frame_rate as f32,
        bins_per_octave,
    };
    let original = if filters.iter().any(|f| matches!(f, Filter::Gate { .. })) {
        cwt.data.clone()
    } else {
        Vec::new()
    };
    for filter in filters {
        let start = std::time::Instant::now();
        if filter.is_phase() {
            if cwt.phase_dev.is_empty() {
                eprintln!("skipping {filter:?}: CWT file has no phase data");
                continue;
            }
            apply_phase(&g, filter, &mut cwt.phase_dev, &cwt.data);
        } else {
            apply_magnitude(&g, filter, &mut cwt.data, &original);
        }
        println!("  applied {filter:?} in {:.2}s", start.elapsed().as_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(nf: usize, ns: usize) -> Grid {
        Grid { nf, ns, frame_rate: 60.0, bins_per_octave: 48.0 }
    }

    /// Apply a magnitude filter with `x` itself as the original data.
    fn run(g: &Grid, f: &Filter, x: &mut Vec<f32>) {
        let orig = x.clone();
        apply_magnitude(g, f, x, &orig);
    }

    #[test]
    fn parse_specs() {
        assert_eq!(Filter::parse("whiten").unwrap(), Filter::Whiten { secs: 0.0 });
        assert_eq!(Filter::parse("smooth-time:0.5").unwrap(), Filter::SmoothTime { secs: 0.5 });
        assert_eq!(
            Filter::parse("harmonic:0.3,12").unwrap(),
            Filter::Harmonic { secs: 0.3, bins: 12 }
        );
        assert_eq!(
            Filter::parse("periodic:2").unwrap(),
            Filter::Periodic { period: 2.0, count: 2 }
        );
        assert!(Filter::parse("periodic").is_err()); // period is required
        assert!(Filter::parse("bogus").is_err());
        assert!(Filter::parse("smooth-time:abc").is_err());
        assert!(Filter::parse("smooth-time:-1").is_err());
        assert!(Filter::parse("power:1,2").is_err());
        assert_eq!(Filter::parse("contrast").unwrap(), Filter::Contrast { k: 1.0 });
        assert_eq!(Filter::parse("contrast:1.5").unwrap(), Filter::Contrast { k: 1.5 });
        assert!(Filter::parse("contrast:-1").is_err());
        assert!(Filter::parse("contrast:0").is_err());
        assert_eq!(Filter::parse("tilt").unwrap(), Filter::Tilt { octaves: 2.0 });
        assert_eq!(Filter::parse("tilt:1.5").unwrap(), Filter::Tilt { octaves: 1.5 });
        assert_eq!(Filter::parse("gate").unwrap(), Filter::Gate { db: -15.0, knee: 6.0 });
        assert_eq!(Filter::parse("gate:-20,3").unwrap(), Filter::Gate { db: -20.0, knee: 3.0 });
        assert!(Filter::parse("tilt:0").is_err());
        assert!(Filter::parse("gate:-15,0").is_err());
        assert!(Filter::parse("gate:1,2,3").is_err());
        assert_eq!(
            Filter::parse("fundamental").unwrap(),
            Filter::Fundamental { amount: 0.8, count: 6, secs: 0.15 }
        );
        assert_eq!(
            Filter::parse("fundamental:0.5,8,0").unwrap(),
            Filter::Fundamental { amount: 0.5, count: 8, secs: 0.0 }
        );
        assert!(Filter::parse("fundamental:1.5").is_err());
        assert!(Filter::parse("fundamental:0.5,1").is_err());
    }

    #[test]
    fn smoothing_preserves_constant() {
        let g = grid(50, 4);
        for filter in [
            Filter::SmoothTime { secs: 0.2 },
            Filter::SmoothFreq { bins: 2 },
            Filter::Periodic { period: 0.1, count: 2 },
        ] {
            let mut x = vec![3.0; 200];
            run(&g, &filter, &mut x);
            assert!(x.iter().all(|v| (v - 3.0).abs() < 1e-5), "{filter:?}");
        }
    }

    #[test]
    fn contrast_removes_floor_keeps_peak() {
        let g = grid(2, 5);
        // Frame 0 constant (all floor); frame 1 has one peak of 10 over a floor of 2.
        let mut x = vec![3.0, 3.0, 3.0, 3.0, 3.0, 2.0, 2.0, 10.0, 2.0, 2.0];
        run(&g, &Filter::Contrast { k: 1.0 }, &mut x);
        assert_eq!(x, vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 8.0, 0.0, 0.0]);
    }

    #[test]
    fn tilt_flattens_slope_and_keeps_steady_peak() {
        // 4 octaves of bins, -6 dB/octave slope, steady peak (x10) at bin 96.
        let (nf, ns) = (10, 193);
        let g = grid(nf, ns);
        let slope = |s: usize| 2f32.powf(-(s as f32) / 48.0);
        let mut x: Vec<f32> = (0..nf * ns).map(|i| slope(i % ns)).collect();
        let mut flat = x.clone();
        run(&g, &Filter::Tilt { octaves: 1.0 }, &mut flat);
        // Interior bins (full smoothing window) become ~constant.
        for s in 48..145 {
            assert!((flat[s] / flat[96] - 1.0).abs() < 1e-3, "bin {s}: {}", flat[s]);
        }
        for f in 0..nf {
            x[f * ns + 96] *= 10.0;
        }
        run(&g, &Filter::Tilt { octaves: 1.0 }, &mut x);
        assert!(x[96] > 5.0 * x[90] && x[96] > 5.0 * x[102], "peak {} vs {} {}", x[96], x[90], x[102]);
    }

    /// One frame of 4.5 octaves with narrow (single-bin) partials at the given bins.
    fn spectrum(partials: &[(usize, f32)]) -> Vec<f32> {
        let mut row = vec![0.0f32; 217];
        for &(bin, amp) in partials {
            row[bin] = amp;
        }
        row
    }

    #[test]
    fn fundamental_folds_harmonic_series() {
        // f0 at bin 20, harmonics 2..5 at +48, +76, +96, +111 bins.
        let g = grid(1, 217);
        let mut x = spectrum(&[(20, 1.0), (68, 0.5), (96, 0.4), (116, 0.3), (131, 0.2)]);
        let total_before: f32 = x.iter().sum();
        run(&g, &Filter::Fundamental { amount: 1.0, count: 5, secs: 0.0 }, &mut x);
        assert!((x[20] - 2.4).abs() < 1e-4, "fundamental {}", x[20]);
        for h in [68, 96, 116, 131] {
            assert!(x[h].abs() < 1e-5, "harmonic at {h} = {}", x[h]);
        }
        assert!((x.iter().sum::<f32>() - total_before).abs() < 1e-4, "amplitude not conserved");

        // Partial amount moves a proportional share.
        let mut y = spectrum(&[(20, 1.0), (68, 0.5)]);
        run(&g, &Filter::Fundamental { amount: 0.5, count: 2, secs: 0.0 }, &mut y);
        assert!((y[20] - 1.25).abs() < 1e-4 && (y[68] - 0.25).abs() < 1e-4, "{} {}", y[20], y[68]);
    }

    #[test]
    fn fundamental_guards_octave_errors_and_leaves_lone_tones() {
        let g = grid(1, 217);
        // Weak bin an octave below a loud tone may take at most its own size.
        let mut x = spectrum(&[(30, 0.05), (78, 1.0)]);
        run(&g, &Filter::Fundamental { amount: 1.0, count: 5, secs: 0.0 }, &mut x);
        assert!(x[78] >= 0.95 - 1e-5, "loud tone lost too much: {}", x[78]);
        // A tone with no harmonics present is unchanged.
        let mut lone = spectrum(&[(100, 0.7)]);
        run(&g, &Filter::Fundamental { amount: 0.8, count: 5, secs: 0.0 }, &mut lone);
        assert_eq!(lone[100], 0.7);
    }

    /// Gaussian lobe of the given width (bins) around a fractional center.
    fn add_lobe(row: &mut [f32], center: f32, amp: f32, width: f32) {
        for (s, v) in row.iter_mut().enumerate() {
            let d = (s as f32 - center) / width;
            *v += amp * (-0.5 * d * d).exp();
        }
    }

    #[test]
    fn fundamental_handles_smeared_lobes_and_detuning() {
        // CWT-like lobes (width 4 bins) on a small floor; harmonics 2 and 3 slightly
        // sharp (stretched partials, +10 cents ~ 0.4 bin).
        let g = grid(1, 217);
        let mut x = vec![0.001f32; 217];
        add_lobe(&mut x, 40.0, 1.0, 4.0);
        add_lobe(&mut x, 88.4, 0.6, 4.0);
        add_lobe(&mut x, 116.5, 0.4, 4.0);
        let before = x.clone();
        run(&g, &Filter::Fundamental { amount: 1.0, count: 6, secs: 0.0 }, &mut x);
        assert!(x[88] < 0.05 * before[88] && x[116] < 0.05 * before[116], "harmonics remain: {} {}", x[88], x[116]);
        assert!((x[40] - 2.0).abs() < 0.05, "fundamental {}", x[40]);
        // Tapered: the shoulder grows, but proportionally less than the peak top.
        let (peak_gain, shoulder_gain) = (x[40] / before[40], x[46] / before[46]);
        assert!(shoulder_gain > 1.0 && shoulder_gain < peak_gain, "{shoulder_gain} vs {peak_gain}");
        // Single peak in the output where there were three.
        let peaks = (1..216).filter(|&s| x[s] > x[s - 1] && x[s] >= x[s + 1] && x[s] > 0.01).count();
        assert_eq!(peaks, 1);
    }

    #[test]
    fn fundamental_leaves_no_steps_at_shallow_dips() {
        // Root at 40, its octave at 88, and an unrelated tone at 98 whose lobe merges
        // with the octave's (shallow dip between them), plus one beside the root at 50.
        let g = grid(1, 217);
        let mut x = vec![0.001f32; 217];
        add_lobe(&mut x, 40.0, 1.0, 4.0);
        add_lobe(&mut x, 50.0, 0.8, 4.0);
        add_lobe(&mut x, 88.0, 0.9, 4.0);
        add_lobe(&mut x, 98.0, 0.8, 4.0);
        let max_step = |r: &[f32]| r.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        let orig = x.clone();
        run(&g, &Filter::Fundamental { amount: 1.0, count: 6, secs: 0.0 }, &mut x);
        assert!(x[88] < 0.75 * orig[88], "octave not reduced: {}", x[88]);
        assert!(x[40] > 1.3, "root not boosted: {}", x[40]);
        // Relative to the tallest peak (as the bars show it), neighbouring bins differ no
        // more than in the input: a cut or boost edge would show as a much bigger jump.
        let peak = |r: &[f32]| r.iter().cloned().fold(0.0, f32::max);
        let (before, after) = (max_step(&orig) / peak(&orig), max_step(&x) / peak(&x));
        assert!(after < 1.2 * before, "step {after} vs input {before}");
    }

    #[test]
    fn fundamental_ignores_unrelated_peaks_and_is_scale_invariant() {
        let g = grid(1, 217);
        // Ratio 1.4 (a tritone-ish 583 cents): not harmonic, must be untouched.
        let mut x = vec![0.001f32; 217];
        add_lobe(&mut x, 60.0, 1.0, 4.0);
        add_lobe(&mut x, 60.0 + 48.0 * 1.4f32.log2(), 0.8, 4.0);
        let before = x.clone();
        run(&g, &Filter::Fundamental { amount: 1.0, count: 6, secs: 0.0 }, &mut x);
        assert_eq!(x, before);
        // Scaling the input (e.g. after another filter) scales the output identically.
        let mut a = spectrum(&[(20, 1.0), (68, 0.5), (96, 0.4)]);
        let mut b: Vec<f32> = a.iter().map(|v| v * 1e-3).collect();
        let f = Filter::Fundamental { amount: 0.7, count: 6, secs: 0.0 };
        run(&g, &f, &mut a);
        run(&g, &f, &mut b);
        for (va, vb) in a.iter().zip(&b) {
            assert!((va * 1e-3 - vb).abs() < 1e-8);
        }
    }

    #[test]
    fn gate_uses_original_levels() {
        // Frames at 0 dB, -30 dB, and -18 dB (mid-knee for gate:-15,6).
        let g = grid(3, 2);
        let db = |d: f32| 10f32.powf(d / 20.0);
        let original = vec![1.0, 0.5, db(-30.0), db(-30.0), db(-18.0), db(-18.0)];
        // Gate after another filter: the current data is flat, but levels come from the original.
        let mut x = vec![2.0f32; 6];
        apply_magnitude(&g, &Filter::Gate { db: -15.0, knee: 6.0 }, &mut x, &original);
        assert_eq!(&x[0..2], &[2.0, 2.0]);
        assert_eq!(&x[2..4], &[0.0, 0.0]);
        assert!((x[4] - 1.0).abs() < 1e-3 && (x[5] - 1.0).abs() < 1e-3, "{:?}", &x[4..]);
    }

    #[test]
    fn whiten_equalizes_bins() {
        let g = grid(10, 2);
        let mut x: Vec<f32> = (0..20).map(|i| if i % 2 == 0 { 100.0 } else { 1.0 }).collect();
        run(&g, &Filter::Whiten { secs: 0.0 }, &mut x);
        assert!(x.iter().all(|v| (v - 1.0).abs() < 1e-5));
    }

    #[test]
    fn flux_and_novelty_zero_on_constant_and_fire_on_onset() {
        let g = grid(40, 1);
        let mut x = vec![1.0f32; 40];
        x[20..].fill(5.0);
        let mut flux = x.clone();
        run(&g, &Filter::Flux { secs: 1.0 / 60.0 }, &mut flux);
        assert!(flux[20] > 1.0);
        assert!(flux.iter().enumerate().all(|(i, v)| i == 20 || *v == 0.0));
        let mut constant = vec![2.0f32; 40];
        run(&g, &Filter::Novelty { secs: 0.2 }, &mut constant);
        assert!(constant.iter().all(|v| v.abs() < 1e-5));
    }

    #[test]
    fn agc_normalizes_loudness() {
        let g = grid(100, 2);
        let mut x: Vec<f32> = (0..200).map(|i| if i < 100 { 0.01 } else { 10.0 }).collect();
        run(&g, &Filter::Agc { secs: 0.1 }, &mut x);
        assert!((x[10] - 1.0).abs() < 1e-5 && (x[190] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn hpss_separates_lines() {
        // Horizontal line at bin 10 (sustained tone), vertical line at frame 30 (hit).
        let (nf, ns) = (60, 21);
        let g = grid(nf, ns);
        let mut x = vec![0.01f32; nf * ns];
        for f in 0..nf {
            x[f * ns + 10] = 1.0;
        }
        for s in 0..ns {
            x[30 * ns + s] = 1.0;
        }
        let tone = 15 * ns + 10;
        let hit = 30 * ns + 3;
        let mut h = x.clone();
        run(&g, &Filter::Harmonic { secs: 0.2, bins: 4 }, &mut h);
        let mut p = x.clone();
        run(&g, &Filter::Percussive { secs: 0.2, bins: 4 }, &mut p);
        assert!(h[tone] > 0.9 && h[hit] < 0.1, "harmonic: tone {} hit {}", h[tone], h[hit]);
        assert!(p[hit] > 0.9 && p[tone] < 0.1, "percussive: tone {} hit {}", p[tone], p[hit]);
    }

    #[test]
    fn phase_filters() {
        let g = grid(120, 1);
        let mags = vec![1.0f32; 120];
        // Alternating +-20 cents wobble around a +10 cent offset.
        let dev: Vec<f32> = (0..120).map(|i| 10.0 + if i % 2 == 0 { 20.0 } else { -20.0 }).collect();

        let mut smooth = dev.clone();
        apply_phase(&g, &Filter::PhaseSmooth { secs: 0.5 }, &mut smooth, &mags);
        assert!((smooth[60] - 10.0).abs() < 1.0, "smooth {}", smooth[60]);

        let mut var = dev.clone();
        apply_phase(&g, &Filter::PhaseVar { secs: 0.5 }, &mut var, &mags);
        assert!((var[60] - 20.0).abs() < 1.0, "var {}", var[60]);

        let mut detrend = dev.clone();
        apply_phase(&g, &Filter::PhaseDetrend { secs: 0.5 }, &mut detrend, &mags);
        assert!((detrend[60].abs() - 20.0).abs() < 1.0, "detrend {}", detrend[60]);
    }
}

/// Diagnostics on a real file: `CWT_PATH=... cargo test --release roughness -- --ignored --nocapture`.
#[cfg(test)]
mod diagnostics {
    use super::*;

    /// Mean frame-to-frame absolute change relative to the mean value (temporal choppiness),
    /// and mean number of local maxima per frame (spectral clutter).
    fn metrics(cwt: &CwtFile) -> (f64, f64) {
        let ns = cwt.header.num_scales as usize;
        let x = &cwt.data;
        let mean = x.iter().map(|&v| v as f64).sum::<f64>() / x.len() as f64;
        let diff = x[ns..].iter().zip(x.iter()).map(|(a, b)| (a - b).abs() as f64).sum::<f64>()
            / (x.len() - ns) as f64;
        let frames = x.len() / ns;
        let max = x.iter().cloned().fold(0.0, f32::max);
        let peaks: usize = (0..frames)
            .map(|f| {
                let r = &x[f * ns..(f + 1) * ns];
                (1..ns - 1).filter(|&s| r[s] > r[s - 1] && r[s] >= r[s + 1] && r[s] > max * 1e-3).count()
            })
            .sum();
        (diff / mean, peaks as f64 / frames as f64)
    }

    #[test]
    #[ignore]
    fn roughness() {
        let path = std::env::var("CWT_PATH").expect("set CWT_PATH");
        // CHAINS="fundamental;tilt contrast fundamental" (chains separated by ';',
        // filters by spaces); defaults to a basic comparison set.
        let chains = std::env::var("CHAINS")
            .unwrap_or_else(|_| ";fundamental;tilt contrast;tilt contrast fundamental".into());
        for chain in chains.split(';') {
            let mut cwt = CwtFile::read(std::path::Path::new(&path)).unwrap();
            let filters: Vec<Filter> =
                chain.split_whitespace().map(|s| Filter::parse(s).unwrap()).collect();
            // Run all but the last filter, then measure how much the last one changes.
            let (head, last) = filters.split_at(filters.len().saturating_sub(1));
            apply(&mut cwt, head);
            let before = cwt.data.clone();
            apply(&mut cwt, last);
            let changed = cwt.data.iter().zip(&before).map(|(a, b)| (a - b).abs() as f64).sum::<f64>()
                / before.iter().map(|&v| v as f64).sum::<f64>().max(1e-30);
            let (rough, peaks) = metrics(&cwt);
            let name = if chain.trim().is_empty() { "(raw)" } else { chain.trim() };
            println!("METRIC {name:40} roughness={rough:.3} peaks/frame={peaks:.1} last-filter-changed={:.1}%", changed * 100.0);
        }
    }

    /// Sharp edges as drawn, after each prefix of a chain:
    /// `CWT_PATH=... CHAIN="tilt contrast" PCT=99.7 cargo test --release edges -- --ignored --nocapture`.
    /// freq-edges: neighbouring bins whose bar heights differ by > 10% of the bar area
    /// (near-vertical walls in the bar graph), per frame.
    /// time-edges: % of frames where most visible bins jump by > 6 dB at once
    /// (vertical lines in the spectrogram).
    #[test]
    #[ignore]
    fn edges() {
        let path = std::env::var("CWT_PATH").expect("set CWT_PATH");
        let chain = std::env::var("CHAIN").expect("set CHAIN");
        let pct: f32 = std::env::var("PCT").ok().and_then(|p| p.parse().ok()).unwrap_or(100.0);
        let filters: Vec<Filter> = chain.split_whitespace().map(|s| Filter::parse(s).unwrap()).collect();
        let base = CwtFile::read(std::path::Path::new(&path)).unwrap();
        let ns = base.header.num_scales as usize;
        for n in 0..=filters.len() {
            let mut cwt = base.clone();
            apply(&mut cwt, &filters[..n]);
            cwt.set_reference(pct);
            let gm = cwt.global_max();
            let h: Vec<f32> = cwt.data.iter().map(|&v| crate::graphics::layout::bar_fraction(v, gm)).collect();
            let frames = h.len() / ns;
            let (mut fedges, mut worst) = (0usize, 0.0f32);
            let mut tedges = 0usize;
            for f in 0..frames {
                let r = &h[f * ns..(f + 1) * ns];
                for s in 0..ns - 1 {
                    let d = (r[s + 1] - r[s]).abs();
                    worst = worst.max(d);
                    if d > 0.10 {
                        fedges += 1;
                    }
                }
                if f > 0 {
                    let p = &h[(f - 1) * ns..f * ns];
                    let mut jumps: Vec<f32> = (0..ns)
                        .filter(|&s| r[s].max(p[s]) > 0.05)
                        .map(|s| (20.0 * ((r[s] + 1e-3) / (p[s] + 1e-3)).log10()).abs())
                        .collect();
                    if jumps.len() > 10 {
                        let mid = jumps.len() / 2;
                        if *jumps.select_nth_unstable_by(mid, f32::total_cmp).1 > 6.0 {
                            tedges += 1;
                        }
                    }
                }
            }
            let name = if n == 0 { "(raw)".to_string() } else { format!("+ {:?}", filters[n - 1]) };
            println!(
                "EDGES {name:60} freq-edges/frame={:.2} steepest={:.2} time-edges={:.2}%",
                fedges as f64 / frames as f64,
                worst,
                tedges as f64 * 100.0 / frames as f64
            );
        }
    }
}

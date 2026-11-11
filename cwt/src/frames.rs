use crate::wavelets::Complex64;

/// Reduce per-sample magnitudes to per-frame values.
///
/// Each frame's value is the mean magnitude over a window of `window_size` samples
/// centered on the frame's time position. Frames are spaced `hop_size` samples apart.
///
/// With window_size = 2 * hop_size, this gives 50% overlap between adjacent frames.
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

/// Per-frame instantaneous-frequency deviation from a bin's center frequency, in cents.
///
/// For each frame window, sums z = c[n+1] * conj(c[n]) over the window. arg(z) is the
/// magnitude-weighted mean phase advance per sample, which gives the instantaneous
/// frequency. The result is 1200 * log2(f_inst / center_freq): positive = sharp,
/// negative = flat. Frames with no energy return 0.
pub fn reduce_phase_deviation(
    coeffs: &[Complex64],
    hop_size: usize,
    window_size: usize,
    center_freq: f64,
    dt: f64,
) -> Vec<f64> {
    let num_frames = (coeffs.len() + hop_size - 1) / hop_size;
    let half_window = window_size / 2;
    (0..num_frames)
        .map(|frame_idx| {
            let center = frame_idx * hop_size;
            let start = center.saturating_sub(half_window);
            let end = (center + half_window).min(coeffs.len().saturating_sub(1));
            let mut z = Complex64::new(0.0, 0.0);
            for n in start..end {
                z += coeffs[n + 1] * coeffs[n].conj();
            }
            if z.norm() <= f64::MIN_POSITIVE {
                return 0.0;
            }
            // arg(z) is in (-pi, pi]; valid while center_freq < Nyquist.
            let inst_freq = z.arg() / (2.0 * std::f64::consts::PI * dt);
            if inst_freq <= 0.0 {
                return -1200.0; // clamp: far flat / degenerate
            }
            (1200.0 * (inst_freq / center_freq).log2()).clamp(-1200.0, 1200.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduce_basic() {
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
        let mut mags = vec![0.0; 50];
        mags.extend(vec![1.0; 50]);
        let frames = reduce_to_frames(&mags, 10, 20);
        let boundary_frame = frames.len() / 2;
        assert!(frames[boundary_frame] > 0.0);
        assert!(frames[boundary_frame] < 1.0);
    }

    #[test]
    fn test_phase_deviation_detects_detuned_tone() {
        use std::f64::consts::PI;
        let dt = 1.0 / 8000.0;
        let center = 440.0;
        // Analytic tone a quarter tone (+50 cents) sharp of the bin center.
        let f = center * 2f64.powf(50.0 / 1200.0);
        let coeffs: Vec<Complex64> = (0..8000)
            .map(|n| Complex64::from_polar(1.0, 2.0 * PI * f * n as f64 * dt))
            .collect();
        let dev = reduce_phase_deviation(&coeffs, 133, 266, center, dt);
        for d in &dev[1..dev.len() - 1] {
            assert!((d - 50.0).abs() < 0.5, "got {d}");
        }
    }
}

use crate::conv::next_power_of_two;
use crate::kernel::wavelet_kernel;
use crate::wavelets::{Complex64, Wavelet};
use rustfft::{num_complex::Complex, FftPlanner};

// Target number of valid output samples per block. Larger = fewer FFT calls
// but more memory. At 524288, peak memory is ~48 MB for 1M-point FFTs.
const TARGET_BLOCK_SIZE: usize = 1 << 19; // 524288

pub struct BatchCwt {
    signal: Vec<f64>,
    fft_size: usize, // N = next_power_of_two(TARGET_BLOCK_SIZE + max_kernel_len - 1)
    planner: FftPlanner<f64>,
}

impl BatchCwt {
    /// Prepare for batch CWT. Does NOT pre-compute a signal FFT;
    /// the signal is stored and processed in blocks (overlap-save).
    pub fn new(signal: &[f64], max_kernel_len: usize) -> Self {
        let fft_size = next_power_of_two(TARGET_BLOCK_SIZE + max_kernel_len - 1);
        Self {
            signal: signal.to_vec(),
            fft_size,
            planner: FftPlanner::new(),
        }
    }

    /// Compute CWT for one scale via overlap-save, returning per-sample complex
    /// coefficients aligned so that output[n] is centered on input sample n.
    pub fn transform_scale<W: Wavelet>(
        &mut self,
        wavelet: &W,
        scale: f64,
        dt: f64,
        ts: f64,
    ) -> Vec<Complex64> {
        let kernel = wavelet_kernel(wavelet, scale, dt, ts);
        let kernel_len = kernel.len();
        let signal_len = self.signal.len();

        // The actual FFT size for this scale must fit kernel + at least one
        // output sample. If this kernel is longer than our pre-sized FFT
        // (shouldn't happen when max_kernel_len was correct), fall back.
        let fft_size = if kernel_len > self.fft_size {
            next_power_of_two(kernel_len + 1)
        } else {
            self.fft_size
        };

        // Valid output samples per block (the non-overlapping portion).
        let valid_size = fft_size - kernel_len + 1;

        // Compute kernel FFT once for this scale.
        let mut kernel_spectrum: Vec<Complex<f64>> = kernel
            .iter()
            .map(|&c| Complex::new(c.re, c.im))
            .collect();
        kernel_spectrum.resize(fft_size, Complex::new(0.0, 0.0));
        let fft = self.planner.plan_fft_forward(fft_size);
        fft.process(&mut kernel_spectrum);

        let ifft = self.planner.plan_fft_inverse(fft_size);
        let norm = 1.0 / fft_size as f64;

        // The kernel spans t in [-ts*scale, +ts*scale], so the causal convolution
        // output is delayed by half the kernel length. Compute `delay` extra causal
        // samples and drop the first `delay` so output[n] is centered on sample n.
        let delay = (kernel_len - 1) / 2;
        let causal_len = signal_len + delay;
        let mut output = vec![Complex64::new(0.0, 0.0); signal_len];

        // Overlap-save: step through causal output in strides of valid_size.
        let mut out_pos = 0usize;
        while out_pos < causal_len {
            // Input block starts (kernel_len - 1) samples before out_pos,
            // so that the circular-convolution's valid region aligns.
            let overlap = kernel_len - 1;
            let in_start = out_pos.saturating_sub(overlap);
            let leading_zeros = overlap.saturating_sub(out_pos);

            // Build the input block of exactly fft_size samples.
            let mut block = vec![Complex::new(0.0f64, 0.0); fft_size];
            let sig_start = in_start;
            let sig_end = (in_start + fft_size - leading_zeros).min(signal_len).max(sig_start);
            for (i, &s) in self.signal[sig_start..sig_end].iter().enumerate() {
                block[leading_zeros + i] = Complex::new(s, 0.0);
            }
            // trailing zeros already set by vec! initialiser

            // FFT → multiply → IFFT.
            fft.process(&mut block);
            for i in 0..fft_size {
                block[i] *= kernel_spectrum[i];
            }
            ifft.process(&mut block);

            // Copy valid output samples (indices overlap..fft_size-1).
            let valid_count = valid_size.min(causal_len - out_pos);
            for i in 0..valid_count {
                let causal_idx = out_pos + i;
                if causal_idx >= delay {
                    let c = block[overlap + i];
                    output[causal_idx - delay] = Complex64::new(c.re * norm, c.im * norm);
                }
            }

            out_pos += valid_size;
        }

        output
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

        let signal: Vec<f64> = (0..1000)
            .map(|i| (2.0 * PI * target_freq * i as f64 * dt).sin())
            .collect();

        let scale_on = morlet.freq_to_scale(target_freq);
        let scale_off = morlet.freq_to_scale(400.0);

        let kernel_on = wavelet_kernel(&morlet, scale_on, dt, ts);
        let kernel_off = wavelet_kernel(&morlet, scale_off, dt, ts);
        let max_kernel_len = kernel_on.len().max(kernel_off.len());

        let mut batch = BatchCwt::new(&signal, max_kernel_len);
        let mags_on: Vec<f64> = batch.transform_scale(&morlet, scale_on, dt, ts).iter().map(|c| c.norm()).collect();
        let mags_off: Vec<f64> = batch.transform_scale(&morlet, scale_off, dt, ts).iter().map(|c| c.norm()).collect();

        let peak_on: f64 = mags_on.iter().cloned().fold(0.0, f64::max);
        let peak_off: f64 = mags_off.iter().cloned().fold(0.0, f64::max);
        assert!(peak_on > peak_off * 3.0, "on={peak_on}, off={peak_off}");
    }

    #[test]
    fn test_output_centered_on_impulse() {
        // A low-frequency (long) kernel must not delay its response.
        let morlet = Morlet::standard();
        let dt = 1.0 / 1000.0;
        let mut signal = vec![0.0; 4000];
        signal[2000] = 1.0;
        let scale = morlet.freq_to_scale(20.0);
        let k = wavelet_kernel(&morlet, scale, dt, 8.0).len();
        let mut batch = BatchCwt::new(&signal, k);
        let out = batch.transform_scale(&morlet, scale, dt, 8.0);
        let peak = (0..out.len()).max_by(|&a, &b| out[a].norm().total_cmp(&out[b].norm())).unwrap();
        assert!((peak as i64 - 2000).abs() <= 1, "peak at {peak}, expected 2000");
    }

    #[test]
    fn test_phase_deviation_end_to_end() {
        // A real 445 Hz sine analysed in the 440 Hz bin should read ~+19.6 cents.
        let morlet = Morlet::standard();
        let rate = 44100.0;
        let dt = 1.0 / rate;
        let signal: Vec<f64> = (0..44100).map(|i| (2.0 * PI * 445.0 * i as f64 * dt).sin()).collect();
        let scale = morlet.freq_to_scale(440.0);
        let k = wavelet_kernel(&morlet, scale, dt, 8.0).len();
        let mut batch = BatchCwt::new(&signal, k);
        let coeffs = batch.transform_scale(&morlet, scale, dt, 8.0);
        let dev = crate::frames::reduce_phase_deviation(&coeffs, 735, 1470, 440.0, dt);
        let expected = 1200.0 * (445.0f64 / 440.0).log2();
        let mid = dev[dev.len() / 2];
        assert!((mid - expected).abs() < 1.0, "got {mid}, expected {expected}");
    }
}

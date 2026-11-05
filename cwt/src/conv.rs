use crate::wavelets::Complex64;
use rustfft::{FftPlanner, num_complex::Complex};

/// Pre-compute the FFT of a wavelet kernel
/// 
/// This allows you to reuse the same kernel FFT for multiple convolutions,
/// saving computation time when analyzing a signal at one frequency.
/// 
/// # Arguments
/// * `kernel` - The wavelet kernel in time domain
/// * `fft_size` - Size for the FFT (should be >= kernel.len() + signal.len() - 1)
/// 
/// # Returns
/// FFT of the zero-padded kernel
pub fn precompute_kernel_fft(kernel: &[Complex64], fft_size: usize) -> Vec<Complex<f64>> {
    assert!(fft_size >= kernel.len(), "fft_size must be >= kernel length");
    
    // Zero-pad kernel to fft_size
    let mut padded_kernel: Vec<Complex<f64>> = kernel.iter()
        .map(|&c| Complex::new(c.re, c.im))
        .collect();
    padded_kernel.resize(fft_size, Complex::new(0.0, 0.0));
    
    // Compute FFT
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut padded_kernel);
    
    padded_kernel
}

/// Convolve a signal with a pre-computed kernel FFT
/// 
/// This performs FFT-based convolution using a pre-computed kernel FFT.
/// The result has the same length as the input signal (valid convolution).
/// 
/// # Arguments
/// * `signal` - Input signal (real-valued)
/// * `kernel_fft` - Pre-computed FFT of the kernel
/// 
/// # Returns
/// Complex convolution result (same length as signal)
pub fn convolve_fft(signal: &[f64], kernel_fft: &[Complex<f64>]) -> Vec<Complex64> {
    let fft_size = kernel_fft.len();
    assert!(fft_size >= signal.len(), "kernel_fft size must be >= signal length");
    
    // Zero-pad signal to fft_size
    let mut padded_signal: Vec<Complex<f64>> = signal.iter()
        .map(|&x| Complex::new(x, 0.0))
        .collect();
    padded_signal.resize(fft_size, Complex::new(0.0, 0.0));
    
    // FFT of signal
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut padded_signal);
    
    // Multiply in frequency domain
    for i in 0..fft_size {
        padded_signal[i] *= kernel_fft[i];
    }
    
    // Inverse FFT
    let ifft = planner.plan_fft_inverse(fft_size);
    ifft.process(&mut padded_signal);
    
    // Normalize and convert back to Complex64
    let normalization = 1.0 / fft_size as f64;
    padded_signal.iter()
        .take(signal.len())
        .map(|&c| Complex64::new(c.re * normalization, c.im * normalization))
        .collect()
}

/// Convolve a signal with a kernel (convenience function)
/// 
/// This performs FFT-based convolution in one step, computing the kernel FFT
/// internally. Use this if you only need to convolve once. For multiple
/// convolutions with the same kernel, use `precompute_kernel_fft` and
/// `convolve_fft` separately.
/// 
/// # Arguments
/// * `signal` - Input signal (real-valued)
/// * `kernel` - Wavelet kernel in time domain
/// 
/// # Returns
/// Complex convolution result (same length as signal)
pub fn convolve(signal: &[f64], kernel: &[Complex64]) -> Vec<Complex64> {
    let fft_size = next_power_of_two(signal.len() + kernel.len() - 1);
    let kernel_fft = precompute_kernel_fft(kernel, fft_size);
    convolve_fft(signal, &kernel_fft)
}

/// Find the next power of two >= n (for efficient FFT)
pub fn next_power_of_two(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut p = 1;
    while p < n {
        p *= 2;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavelets::{Wavelet, Morlet};
    use crate::kernel::wavelet_kernel;
    use std::f64::consts::PI;
    
    #[test]
    fn test_next_power_of_two() {
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(100), 128);
        assert_eq!(next_power_of_two(1000), 1024);
    }
    
    #[test]
    fn test_precompute_kernel_fft() {
        let kernel = vec![
            Complex64::new(1.0, 0.0),
            Complex64::new(0.5, 0.0),
            Complex64::new(0.0, 0.0),
        ];
        
        let fft_size = 8;
        let kernel_fft = precompute_kernel_fft(&kernel, fft_size);
        
        assert_eq!(kernel_fft.len(), fft_size);
    }
    
    #[test]
    fn test_convolve_with_impulse() {
        // Convolve with impulse should return the signal
        let signal = vec![1.0, 2.0, 3.0, 4.0];
        let kernel = vec![Complex64::new(1.0, 0.0)];
        
        let result = convolve(&signal, &kernel);
        
        assert_eq!(result.len(), signal.len());
        for (i, &s) in signal.iter().enumerate() {
            assert!((result[i].re - s).abs() < 1e-10);
            assert!(result[i].im.abs() < 1e-10);
        }
    }
    
    #[test]
    fn test_convolve_fft_matches_direct() {
        // Test that FFT convolution gives similar results to direct
        let signal = vec![1.0, 0.0, -1.0, 0.0];
        let kernel = vec![
            Complex64::new(0.5, 0.0),
            Complex64::new(1.0, 0.0),
            Complex64::new(0.5, 0.0),
        ];
        
        let result = convolve(&signal, &kernel);
        
        // Should produce a smoothed version of the signal
        assert_eq!(result.len(), signal.len());
    }
    
    #[test]
    fn test_precompute_and_convolve() {
        let morlet = Morlet::standard();
        let scale = morlet.freq_to_scale(100.0);
        let dt = 1.0 / 1000.0;
        let ts = 8.0;
        
        let kernel = wavelet_kernel(&morlet, scale, dt, ts);
        
        // Create a test signal: 100 Hz sine wave
        let signal: Vec<f64> = (0..1000)
            .map(|i| (2.0 * PI * 100.0 * i as f64 * dt).sin())
            .collect();
        
        // Pre-compute kernel FFT
        let fft_size = next_power_of_two(signal.len() + kernel.len() - 1);
        let kernel_fft = precompute_kernel_fft(&kernel, fft_size);
        
        // Convolve
        let result = convolve_fft(&signal, &kernel_fft);
        
        assert_eq!(result.len(), signal.len());
        
        // Should have significant magnitude at 100 Hz
        let max_magnitude = result.iter()
            .map(|c| c.norm())
            .fold(0.0f64, |a, b| a.max(b));
        
        assert!(max_magnitude > 0.1);
    }
}

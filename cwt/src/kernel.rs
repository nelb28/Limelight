use crate::wavelets::{Wavelet, Complex64};

/// Generate a wavelet kernel for a specific scale
/// 
/// Creates a discretely-sampled wavelet kernel that can be used for convolution
/// with a signal in the continuous wavelet transform.
/// 
/// # Arguments
/// * `wavelet` - Reference to a wavelet implementing the Wavelet trait
/// * `scale` - Scale parameter (a > 0, larger means lower frequency)
/// * `dt` - Time step / sample period in seconds (1 / sample_rate)
/// * `ts` - Time support multiplier (controls kernel length, typically 5-10)
/// 
/// # Returns
/// Vector of complex wavelet values, sampled at dt intervals
/// ```
pub fn wavelet_kernel<W: Wavelet>(wavelet: &W, scale: f64, dt: f64, ts: f64) -> Vec<Complex64> {
    // Time range: [-ts*scale, +ts*scale]
    let t_max = ts * scale;
    let t_min = -t_max;
    
    // Number of samples
    let num_samples = ((t_max - t_min) / dt).ceil() as usize + 1;
    
    // Generate kernel samples
    let mut kernel = Vec::with_capacity(num_samples);
    
    for i in 0..num_samples {
        let t = t_min + i as f64 * dt;
        
        // Evaluate scaled wavelet: ψ(t/a) / sqrt(a)
        let t_normalized = t / scale;
        let psi = wavelet.eval(t_normalized);
        // The dt factor turns the discrete convolution sum into an integral approximation,
        // making output magnitudes independent of the sample rate.
        let normalization = dt / scale.sqrt();
        
        let kernel_val = Complex64::new(
            psi.re * normalization,
            psi.im * normalization,
        );
        
        kernel.push(kernel_val);
    }
    
    kernel
}

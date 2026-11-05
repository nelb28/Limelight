/// Generate logarithmically-spaced frequencies
/// 
/// # Arguments
/// * `min_freq` - Minimum frequency in Hz
/// * `max_freq` - Maximum frequency in Hz
/// * `voices_per_octave` - Number of subdivisions per octave (e.g., 12 for semitones)
/// 
/// # Returns
/// Vector of frequencies in ascending order
///
/// ```
pub fn log_scales(min_freq: f64, max_freq: f64, voices_per_octave: usize, center_freq: f64) -> Vec<f64> {
    // Calculate number of octaves
    let num_octaves = (max_freq / min_freq).log2();
    
    // Total number of frequencies
    let num_freqs = (num_octaves * voices_per_octave as f64).ceil() as usize + 1;
    
    // Generate logarithmically-spaced frequencies
    let mut scales = Vec::with_capacity(num_freqs);
    
    for i in 0..num_freqs {
        // Logarithmic frequency spacing: f = f_min * 2^(i / voices_per_octave)
        let freq = min_freq * 2_f64.powf(i as f64 / voices_per_octave as f64);
        
        // Stop if we exceed max frequency
        if freq > max_freq {
            break;
        }
        
        scales.push(center_freq / freq);
    }
    
    scales
}

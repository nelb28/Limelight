mod morlet;

pub use morlet::Morlet;
pub use num_complex::Complex64;

/// Wavelet trait for continuous wavelet transform
pub trait Wavelet {
    /// Evaluate the wavelet at time t
    /// Returns complex value
    fn eval(&self, t: f64) -> Complex64;

    /// Get the center frequency in Hz
    fn center_freq(&self) -> f64;

    fn freq_to_scale(&self, freq: f64) -> f64 {
        self.center_freq() / freq
    }
}

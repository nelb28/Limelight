use super::{Wavelet, Complex64};
use std::f64::consts::PI;

/// Morlet wavelet: complex sinusoid in a Gaussian envelope
/// ψ(t) = exp(iω₀t) * exp(-t²/2) / π^(1/4)
pub struct Morlet {
    omega0: f64,
}

impl Morlet {
    /// Create a new Morlet wavelet
    /// omega0 is the central frequency parameter (typically 6.0)
    pub fn new(omega0: f64) -> Self {
        Self { omega0 }
    }

    pub fn standard() -> Self {
        Self { omega0: 6.0 }
    }
}

impl Wavelet for Morlet {
    fn eval(&self, t: f64) -> Complex64 {
        let norm = 1.0 / PI.powf(0.25);
        let envelope = (-t * t / 2.0).exp();
        let amplitude = norm * envelope;
        let phase = self.omega0 * t;
        
        Complex64::from_polar(amplitude, phase)
    }
    
    fn center_freq(&self) -> f64 {
        self.omega0 / (2.0 * PI)
    }
}

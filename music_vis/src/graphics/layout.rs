//! Vertical layout of one panel in NDC y (-1 = bottom, 1 = top).
//!
//! From the bottom up: bars, then the magnitude spectrogram, then (optionally) the
//! phase-deviation spectrogram. Each panel is drawn into its own viewport, so these
//! coordinates are the same for every panel.

/// Fraction of the bar area reached by a bar at the reference level (the max, or the
/// `--peak-pct` percentile). Louder peaks use the headroom above it instead of being
/// cut off flat.
pub const BAR_REFERENCE_FRACTION: f32 = 0.85;

/// Height of a bar as a fraction of the bar area, exactly as `bars.wgsl` draws it.
pub fn bar_fraction(magnitude: f32, global_max: f32) -> f32 {
    let rel = (magnitude / global_max).max(0.0);
    (rel * BAR_REFERENCE_FRACTION).min(1.0)
}

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// Top edge of the bar area (bars grow up from y = -1).
    pub bars_top: f32,
    /// (bottom, top) of the magnitude spectrogram.
    pub magnitude: (f32, f32),
    /// (bottom, top) of the phase-deviation spectrogram, if shown.
    pub phase: Option<(f32, f32)>,
}

impl Layout {
    pub fn new(show_phase: bool) -> Self {
        if show_phase {
            Self { bars_top: -0.18, magnitude: (-0.10, 0.42), phase: Some((0.47, 1.0)) }
        } else {
            Self { bars_top: -0.12, magnitude: (-0.04, 1.0), phase: None }
        }
    }

    /// NDC height of the bar area.
    pub fn bar_height(&self) -> f32 {
        self.bars_top + 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_fraction_matches_shader_rules() {
        assert_eq!(bar_fraction(0.0, 2.0), 0.0);
        assert_eq!(bar_fraction(-1.0, 2.0), 0.0); // negative clamps to 0
        assert!((bar_fraction(2.0, 2.0) - BAR_REFERENCE_FRACTION).abs() < 1e-6); // reference level
        assert_eq!(bar_fraction(10.0, 2.0), 1.0); // above headroom clamps to the top
    }
}

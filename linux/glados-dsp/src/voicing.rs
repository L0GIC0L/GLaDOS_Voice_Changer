/// Maps the "Voicing sensitivity" setting to the YIN aperiodicity threshold.
pub struct Voicing;

impl Voicing {
    /// A frame counts as pitched when its aperiodicity is below this value, so a higher sensitivity
    /// means a higher threshold and more of the sound is treated as voice. Runs from a strict 0.08
    /// (only very clean vowels) to a permissive 0.40 (noisy, breathy or reverberant speech).
    pub fn threshold(sensitivity_percent: f32) -> f64 {
        0.08 + 0.32 * (sensitivity_percent as f64 / 100.0).clamp(0.0, 1.0)
    }
}

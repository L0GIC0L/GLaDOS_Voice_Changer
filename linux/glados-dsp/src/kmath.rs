//! Kotlin and Java numeric semantics that Rust spells differently, so the ported DSP rounds exactly as the Android
//! version does.

/// Kotlin `Double.roundToInt()` (Java `Math.round`): halves round up, towards positive infinity, and the result
/// saturates at the `Int` range. NaN gives 0, where Kotlin would throw.
pub(crate) fn round_f64(x: f64) -> i32 {
    let f = x.floor();
    // x - floor(x) is exact, so this decides ties without the error of floor(x + 0.5).
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    r as i32
}

/// Kotlin `Float.roundToInt()` (Java `Math.round(float)`): halves round up, saturating; NaN gives 0.
pub(crate) fn round_f32(x: f32) -> i32 {
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    r as i32
}

/// Kotlin `Float.pow(Float)`, `tanh(Float)` and friends widen to double, call `java.lang.Math` and narrow again.
pub(crate) fn powf(base: f32, e: f32) -> f32 {
    (base as f64).powf(e as f64) as f32
}

pub(crate) fn tanhf(x: f32) -> f32 {
    (x as f64).tanh() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_matches_java() {
        assert_eq!(1, round_f64(0.5));
        assert_eq!(0, round_f64(-0.5));
        assert_eq!(-1, round_f64(-0.5000000000000001));
        assert_eq!(0, round_f64(0.49999999999999994));
        assert_eq!(-2, round_f64(-2.5));
        assert_eq!(3, round_f64(2.5));
        assert_eq!(i32::MAX, round_f64(1e300));
        assert_eq!(i32::MIN, round_f64(-1e300));
        assert_eq!(0, round_f32(-0.5));
        assert_eq!(1, round_f32(0.5));
        assert_eq!(-3, round_f32(-3.2));
    }
}

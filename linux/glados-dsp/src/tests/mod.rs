//! Ports of the Kotlin `dsp` unit tests. They live inside the crate, as the Kotlin ones live in the same module,
//! so they can reach the internal FFT, the live engine's clock and `java.util.Random`.

mod denoiser_test;
mod fx_chain_test;
mod kotlin_reference_test;
mod live_pitch_engine_test;
mod model_test;
mod offline_test;
mod test_signals;
mod voicing_test;
mod yin_test;

/// JUnit's `assertEquals(expected, actual, delta)`.
#[track_caller]
pub(crate) fn assert_close(expected: f64, actual: f64, delta: f64, label: &str) {
    assert!(
        (expected - actual).abs() <= delta,
        "{label}: expected {expected} but was {actual} (allowed difference {delta})"
    );
}

/// The Kotlin tests' `{ false }` cancellation callback.
pub(crate) fn never() -> bool {
    false
}

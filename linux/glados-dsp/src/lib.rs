//! The GLaDOS voice changer's sound engine: pitch tracking, note editing, PSOLA pitch and formant shifting, effects
//! and noise reduction. Pure Rust with no dependencies, so it can be tested without any audio hardware.
//!
//! Ported from the Android app's Kotlin `dsp` module, one file per Kotlin file, keeping its numerics: the same inputs
//! give the same output as the Android version, give or take the last bit of the platform's maths library.

pub mod analysis;
pub mod denoiser;
pub mod fx_chain;
pub mod grain;
pub mod live_pitch_engine;
pub mod marks;
pub mod model;
pub mod notes;
pub mod offline_renderer;
pub mod pitch_bandpass;
pub mod scales;
pub mod speed_warp;
pub mod tuner;
pub mod voicing;
pub mod wav;
pub mod yin;

pub(crate) mod java_random;
mod kmath;

pub use analysis::{Analysis, Analyzer};
pub use denoiser::Denoiser;
pub use fx_chain::FxChain;
pub use grain::Grain;
pub use live_pitch_engine::LivePitchEngine;
pub use marks::Marks;
pub use notes::{Edit, Note, NoteOps, TimeMap};
pub use offline_renderer::{OfflineRenderer, RenderSettings};
pub use pitch_bandpass::PitchBandpass;
pub use speed_warp::SpeedWarp;
pub use tuner::Tuner;
pub use voicing::Voicing;
pub use wav::{Audio, Wav};
pub use yin::Yin;

#[cfg(test)]
mod tests;

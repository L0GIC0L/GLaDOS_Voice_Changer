//! The GLaDOS voice changer's sound engine: pitch tracking, note editing, PSOLA pitch and formant shifting, effects
//! and noise reduction. Pure Rust with no dependencies, so it can be tested without any audio hardware.

pub mod model;
pub mod scales;

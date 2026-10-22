//! Sound in and out: clip playback, recording, live voice, and reading audio files.

pub mod decode;
pub mod devices;
pub mod live;
pub mod player;
pub mod recorder;

use std::sync::atomic::{AtomicU32, Ordering};

/// An `f32` that can be shared between the audio callback and the UI.
#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Mixes interleaved frames down to mono.
pub fn downmix(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let scale = 1.0 / channels as f32;
    out.extend(interleaved.chunks_exact(channels).map(|f| f.iter().sum::<f32>() * scale));
}

/// Copies mono samples to every channel of an interleaved buffer.
pub fn upmix(mono: &[f32], channels: usize, out: &mut [f32]) {
    for (frame, v) in out.chunks_exact_mut(channels.max(1)).zip(mono) {
        frame.fill(*v);
    }
}

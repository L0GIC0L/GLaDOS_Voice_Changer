use crate::scales::{self, CHROMATIC_MASK};

const SETTLE_FRAMES: i32 = 4;

/// Chooses the target note for a detected pitch and smooths the correction over time.
///
/// With `retune_ms` = 0 the output sits exactly on the target note (hard steps). Larger values
/// smooth the *correction* rather than the pitch, so natural glides survive at slow speeds.
#[derive(Clone, Debug)]
pub struct Tuner {
    frame_seconds: f64,

    pub mask: i32,
    pub retune_ms: f64,
    pub strength: f64,
    pub hold_semitones: f64,
    pub monotone: bool,
    pub monotone_note: i32,

    current_target: i32,
    smoothed: f64,
    was_voiced: bool,
    voiced_frames: i32,
}

impl Tuner {
    pub fn new(frame_seconds: f64) -> Self {
        Tuner {
            frame_seconds,
            mask: CHROMATIC_MASK,
            retune_ms: 20.0,
            strength: 1.0,
            hold_semitones: 0.25,
            monotone: false,
            monotone_note: 57,
            current_target: i32::MIN,
            smoothed: 0.0,
            was_voiced: false,
            voiced_frames: 0,
        }
    }

    pub fn reset(&mut self) {
        self.current_target = i32::MIN;
        self.was_voiced = false;
        self.voiced_frames = 0;
    }

    /// Target note (MIDI) most recently chosen, or [`i32::MIN`] if none.
    pub fn target(&self) -> i32 {
        self.current_target
    }

    /// `midi_in` is the detected pitch as MIDI, after any transposition. Returns the output pitch as MIDI.
    pub fn update(&mut self, midi_in: f64, voiced: bool) -> f64 {
        if !voiced {
            self.was_voiced = false;
            self.voiced_frames = 0;
            return midi_in;
        }
        self.voiced_frames += 1;
        let nearest = if self.monotone { self.monotone_note } else { scales::nearest_allowed(midi_in, self.mask) };
        let mut tgt = nearest;
        if !self.monotone
            && self.was_voiced
            && self.voiced_frames > SETTLE_FRAMES
            && self.current_target != i32::MIN
            && scales::is_allowed(self.mask, self.current_target)
            && nearest != self.current_target
        {
            // Hysteresis: stay on the current note until the new one is clearly closer. Not applied for the
            // first few frames of a phrase, when the pitch estimate is still settling and a wrong first guess
            // must not be held.
            if (midi_in - self.current_target as f64).abs() < (midi_in - nearest as f64).abs() + self.hold_semitones {
                tgt = self.current_target;
            }
        }
        self.current_target = tgt;

        let correction = (tgt as f64 - midi_in) * self.strength;
        if !self.was_voiced || self.retune_ms <= 0.0 {
            self.smoothed = correction;
        } else {
            let alpha = 1.0 - (-self.frame_seconds * 1000.0 / self.retune_ms).exp();
            self.smoothed += alpha * (correction - self.smoothed);
        }
        self.was_voiced = true;
        midi_in + self.smoothed
    }
}

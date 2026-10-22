//! Notes, scales and pitch conversion.

pub mod pitch {
    pub fn hz_to_midi(hz: f64, a4: f64) -> f64 {
        69.0 + 12.0 * (hz / a4).log2()
    }

    pub fn midi_to_hz(midi: f64, a4: f64) -> f64 {
        a4 * 2f64.powf((midi - 69.0) / 12.0)
    }
}

pub const NOTE_NAMES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

pub struct ScaleDef {
    pub name: &'static str,
    pub intervals: &'static [i32],
}

pub static SCALES: [ScaleDef; 11] = [
    ScaleDef { name: "Chromatic", intervals: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] },
    ScaleDef { name: "Major", intervals: &[0, 2, 4, 5, 7, 9, 11] },
    ScaleDef { name: "Natural minor", intervals: &[0, 2, 3, 5, 7, 8, 10] },
    ScaleDef { name: "Harmonic minor", intervals: &[0, 2, 3, 5, 7, 8, 11] },
    ScaleDef { name: "Pentatonic major", intervals: &[0, 2, 4, 7, 9] },
    ScaleDef { name: "Pentatonic minor", intervals: &[0, 3, 5, 7, 10] },
    ScaleDef { name: "Blues", intervals: &[0, 3, 5, 6, 7, 10] },
    ScaleDef { name: "Whole tone", intervals: &[0, 2, 4, 6, 8, 10] },
    ScaleDef { name: "Dorian", intervals: &[0, 2, 3, 5, 7, 9, 10] },
    ScaleDef { name: "Mixolydian", intervals: &[0, 2, 4, 5, 7, 9, 10] },
    ScaleDef { name: "Custom", intervals: &[] },
];

pub const SCALE_NAMES: [&str; 11] = [
    "Chromatic", "Major", "Natural minor", "Harmonic minor", "Pentatonic major", "Pentatonic minor", "Blues",
    "Whole tone", "Dorian", "Mixolydian", "Custom",
];

pub const CUSTOM: i32 = 10;
pub const CHROMATIC_MASK: i32 = 0xFFF;

/// 12-bit mask of allowed pitch classes (bit 0 = C). For [`CUSTOM`] the mask is used as given, otherwise the scale is
/// transposed to `root`.
pub fn mask(scale: i32, root: i32, custom_mask: i32) -> i32 {
    if scale == CUSTOM {
        return custom_mask & CHROMATIC_MASK;
    }
    let def = &SCALES[scale.clamp(0, SCALES.len() as i32 - 1) as usize];
    def.intervals.iter().fold(0, |m, i| m | (1 << (i + root).rem_euclid(12)))
}

pub fn is_allowed(mask: i32, midi: i32) -> bool {
    (mask >> midi.rem_euclid(12)) & 1 == 1
}

/// The allowed note nearest `midi`; plain rounding if the mask allows nothing.
pub fn nearest_allowed(midi: f64, mask: i32) -> i32 {
    if mask & CHROMATIC_MASK == 0 {
        return midi.round() as i32;
    }
    let lo = midi.floor() as i32 - 6;
    let hi = midi.ceil() as i32 + 6;
    let mut best = midi.round() as i32;
    let mut best_d = f64::MAX;
    for n in lo..=hi {
        if is_allowed(mask, n) {
            let d = (n as f64 - midi).abs();
            if d < best_d {
                best_d = d;
                best = n;
            }
        }
    }
    best
}

pub fn note_name(midi: i32) -> String {
    format!("{}{}", NOTE_NAMES[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_and_nearest_notes() {
        let c_major = mask(1, 0, 0);
        assert_eq!(0b101010110101, c_major);
        assert_eq!(62, nearest_allowed(61.4, c_major)); // C♯ is out of key; D is 0.6 away, C is 1.4
        assert_eq!(60, nearest_allowed(60.9, c_major));
        assert_eq!(c_major, mask(2, 9, 0)); // A minor has the same notes
        assert_eq!(61, nearest_allowed(61.2, CHROMATIC_MASK));
        assert_eq!(60, nearest_allowed(60.4, 0));
        assert_eq!("A4", note_name(69));
        assert_eq!("C♯3", note_name(49));
        assert_eq!(0b000100000001, mask(CUSTOM, 5, 0b000100000001));
        assert!((pitch::hz_to_midi(440.0, 440.0) - 69.0).abs() < 1e-12);
        assert!((pitch::midi_to_hz(57.0, 440.0) - 220.0).abs() < 1e-9);
    }
}

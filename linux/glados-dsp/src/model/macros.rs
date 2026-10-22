use super::{VoiceParams, P};

/// How a Simple-mode slider maps onto the real settings.
enum Kind {
    /// Scales several zero-based amounts together so their balance is kept. The knob sits at the largest of them as
    /// a fraction of its range; from all-zero they come in at their weights.
    Blend(&'static [(P, f32)]),
    Autotune,
    Tone,
}

/// A Simple-mode slider: one 0 to 100 knob that moves several settings at once.
pub struct Macro {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    kind: Kind,
}

const WIDE_LOW: f32 = 70.0;
const WIDE_HIGH: f32 = 18000.0;
const NARROW_LOW: f32 = 500.0;
const NARROW_HIGH: f32 = 2500.0;

impl Macro {
    /// Where the current settings sit on the knob, 0 to 100.
    pub fn read(&self, p: &VoiceParams) -> f32 {
        match self.kind {
            Kind::Blend(parts) => level(parts, p) * 100.0,
            Kind::Autotune => {
                if p.get(P::Strength) < 99.5 {
                    p.get(P::Strength) / 2.0
                } else {
                    50.0 + 50.0 * (1.0 - (p.get(P::Retune) / 200.0).clamp(0.0, 1.0))
                }
            }
            Kind::Tone => ((WIDE_HIGH / p.get(P::Lpf)).ln() / (WIDE_HIGH / NARROW_HIGH).ln()).clamp(0.0, 1.0) * 100.0,
        }
    }

    /// The settings moved to knob position `v` (0 to 100).
    pub fn write(&self, p: &VoiceParams, v: f32) -> VoiceParams {
        let t = (v / 100.0).clamp(0.0, 1.0);
        match self.kind {
            Kind::Blend(parts) => {
                let cur = level(parts, p);
                parts.iter().fold(*p, |acc, (s, w)| {
                    let max = s.spec().max;
                    acc.with(*s, if cur > 0.005 { p.get(*s) * t / cur } else { max * w * t })
                })
            }
            Kind::Autotune => {
                if v <= 50.0 {
                    p.with(P::Strength, v * 2.0).with(P::Retune, 200.0)
                } else {
                    p.with(P::Strength, 100.0).with(P::Retune, 200.0 * (1.0 - (v - 50.0) / 50.0))
                }
            }
            Kind::Tone => p
                .with(P::Hpf, WIDE_LOW * (NARROW_LOW / WIDE_LOW).powf(t))
                .with(P::Lpf, WIDE_HIGH * (NARROW_HIGH / WIDE_HIGH).powf(t)),
        }
    }
}

fn level(parts: &[(P, f32)], p: &VoiceParams) -> f32 {
    parts.iter().map(|(s, _)| p.get(*s) / s.spec().max).fold(0.0, f32::max)
}

pub static MACROS: [Macro; 6] = [
    Macro {
        id: "autotune",
        label: "Autotune",
        help: "Left keeps your own melody. The middle pulls gently to the scale. Right snaps hard like a robot.",
        kind: Kind::Autotune,
    },
    Macro {
        id: "robotic",
        label: "Robotic",
        help: "Ring mod, metal comb and bit crush together.",
        kind: Kind::Blend(&[(P::RingMix, 0.5), (P::CombMix, 1.0), (P::CrushMix, 0.8)]),
    },
    Macro {
        id: "glitches",
        label: "Glitches",
        help: "Every kind of random malfunction together.",
        kind: Kind::Blend(&[
            (P::Glitch, 1.0), (P::GlitchReverse, 0.4), (P::GlitchTape, 0.25),
            (P::GlitchPitch, 0.4), (P::GlitchDrop, 0.3), (P::GlitchStatic, 0.3),
        ]),
    },
    Macro {
        id: "movement",
        label: "Movement",
        help: "Chorus, phaser, tremolo and tape warble together.",
        kind: Kind::Blend(&[(P::Chorus, 1.0), (P::Phaser, 0.6), (P::Tremolo, 0.3), (P::Warble, 0.5)]),
    },
    Macro { id: "tone", label: "Radio tone", help: "Left is full range. Right is a thin, tinny telephone.", kind: Kind::Tone },
    Macro {
        id: "room",
        label: "Room",
        help: "Reverb and echo together.",
        kind: Kind::Blend(&[(P::ReverbMix, 1.0), (P::EchoMix, 0.3)]),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Preset;

    #[test]
    fn simple_mode_sliders_land_where_they_are_set() {
        for start in [VoiceParams::defaults(), Preset::find("Natural").unwrap().apply()] {
            for m in MACROS.iter() {
                for v in [0.0, 40.0, 100.0] {
                    let got = m.read(&m.write(&start, v));
                    assert!((got - v).abs() <= 1.5, "{} -> {v} read back {got}", m.id);
                }
            }
        }
        // Scaling keeps the balance between the settings a slider moves (defaults: ring 10, comb 15, crush 15).
        let p = MACROS[1].write(&VoiceParams::defaults(), 30.0);
        assert!((p.get(P::RingMix) - 20.0).abs() <= 1.0);
        assert!((p.get(P::CombMix) - 30.0).abs() <= 1.0);
    }
}

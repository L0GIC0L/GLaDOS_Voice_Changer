//! The settings schema: every control in the app is generated from [`SPECS`].

mod library;
mod macros;
mod presets;
mod specs;

pub use macros::{Macro, MACROS};
pub use presets::{Preset, PresetCategory, PRESETS};
pub use specs::{P, COUNT, SPECS};

use crate::scales::note_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Tuning,
    Voice,
    Speed,
    Robot,
    Glitch,
    Motion,
    Space,
    Output,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::Tuning, Group::Voice, Group::Speed, Group::Robot, Group::Glitch, Group::Motion, Group::Space, Group::Output,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Tuning => "Tuning",
            Group::Voice => "Voice",
            Group::Speed => "Speed and harmony",
            Group::Robot => "Robot",
            Group::Glitch => "Glitches",
            Group::Motion => "Movement",
            Group::Space => "Space",
            Group::Output => "EQ and output",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Group::Tuning => "Which notes the voice is pulled to, and how hard.",
            Group::Voice => "Register, formants and pitch detection.",
            Group::Speed => "How fast the voice talks, random speed changes, and a second voice. Recordings and speech only.",
            Group::Robot => "The metallic, digital character.",
            Group::Glitch => "Random malfunctions. Apart from crackle, they only fire while there is voice.",
            Group::Motion => "Effects that sweep, pulse and waver.",
            Group::Space => "Echo and the test chamber.",
            Group::Output => "Final tone shaping and level.",
        }
    }
}

/// One user-adjustable setting.
pub struct ParamSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub group: Group,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub step: f32,
    pub unit: &'static str,
    pub help: &'static str,
    pub choices: Option<&'static [&'static str]>,
    pub toggle: bool,
    pub hidden: bool,
    /// Describes the recording rather than the voice (noise, pitch range), so presets leave it alone.
    pub input: bool,
    /// Only affects the live voice.
    pub live_only: bool,
    pub formatter: Option<fn(f32) -> String>,
}

const fn base(id: &'static str, label: &'static str, group: Group, min: f32, max: f32, default: f32, step: f32) -> ParamSpec {
    ParamSpec {
        id, label, group, min, max, default, step, unit: "", help: "", choices: None, toggle: false, hidden: false,
        input: false, live_only: false, formatter: None,
    }
}

#[allow(clippy::too_many_arguments)]
const fn slider(
    id: &'static str, label: &'static str, group: Group, min: f32, max: f32, default: f32, step: f32,
    unit: &'static str, help: &'static str,
) -> ParamSpec {
    ParamSpec { unit, help, ..base(id, label, group, min, max, default, step) }
}

const fn toggle(id: &'static str, label: &'static str, group: Group, default: bool, help: &'static str) -> ParamSpec {
    ParamSpec { help, toggle: true, ..base(id, label, group, 0.0, 1.0, if default { 1.0 } else { 0.0 }, 1.0) }
}

const fn choice(
    id: &'static str, label: &'static str, group: Group, options: &'static [&'static str], default: i32, help: &'static str,
) -> ParamSpec {
    ParamSpec { help, choices: Some(options), ..base(id, label, group, 0.0, (options.len() - 1) as f32, default as f32, 1.0) }
}

impl ParamSpec {
    const fn hidden(self) -> Self {
        ParamSpec { hidden: true, ..self }
    }
    const fn input(self) -> Self {
        ParamSpec { input: true, ..self }
    }
    const fn live_only(self) -> Self {
        ParamSpec { live_only: true, ..self }
    }
    const fn formatted(self, f: fn(f32) -> String) -> Self {
        ParamSpec { formatter: Some(f), ..self }
    }

    /// Clamps to the range and snaps to the step grid, trimming float error so values like 0.1 * 120 - 12 come back as 0.
    pub fn clamp(&self, v: f32) -> f32 {
        let c = if v.is_nan() { self.default } else { v.clamp(self.min, self.max) };
        if self.step <= 0.0 {
            return c;
        }
        let snapped = self.min as f64 + ((c - self.min) as f64 / self.step as f64).round() * self.step as f64;
        (((snapped * 10000.0).round() / 10000.0) as f32).clamp(self.min, self.max)
    }

    pub fn format(&self, v: f32) -> String {
        if let Some(f) = self.formatter {
            return f(v);
        }
        if let Some(c) = self.choices {
            return c[(v.round() as i32).clamp(0, c.len() as i32 - 1) as usize].to_string();
        }
        if self.toggle {
            return if v >= 0.5 { "On" } else { "Off" }.to_string();
        }
        let text = if self.step >= 1.0 || v.abs() >= 1000.0 {
            format!("{}", v.round() as i64)
        } else if self.step >= 0.1 {
            format!("{v:.1}")
        } else {
            format!("{v:.2}")
        };
        if self.unit.is_empty() { text } else { format!("{text} {}", self.unit) }
    }
}

fn note_name_fmt(v: f32) -> String {
    note_name(v.round() as i32)
}

impl P {
    pub fn spec(self) -> &'static ParamSpec {
        &SPECS[self as usize]
    }

    pub fn by_id(id: &str) -> Option<P> {
        P::ALL.iter().copied().find(|p| p.spec().id == id)
    }

    /// Visible settings of a group, in schema order.
    pub fn in_group(g: Group) -> impl Iterator<Item = P> {
        P::ALL.into_iter().filter(move |p| p.spec().group == g && !p.spec().hidden)
    }
}

/// An immutable snapshot of every setting. `Copy`, so it is cheap to hand to the audio thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams([f32; COUNT]);

impl Default for VoiceParams {
    fn default() -> Self {
        Self::defaults()
    }
}

impl VoiceParams {
    /// The defaults are the Portal 1 voice.
    pub fn defaults() -> Self {
        let mut v = [0f32; COUNT];
        for (i, s) in SPECS.iter().enumerate() {
            v[i] = s.default;
        }
        VoiceParams(v)
    }

    pub fn get(&self, p: P) -> f32 {
        self.0[p as usize]
    }

    pub fn bool(&self, p: P) -> bool {
        self.0[p as usize] >= 0.5
    }

    pub fn int(&self, p: P) -> i32 {
        self.0[p as usize].round() as i32
    }

    pub fn with(&self, p: P, value: f32) -> Self {
        let mut c = *self;
        c.0[p as usize] = p.spec().clamp(value);
        c
    }

    /// Applies `(id, value)` pairs; unknown ids are ignored, so settings saved by other versions still load.
    pub fn with_all<'a>(&self, values: impl IntoIterator<Item = (&'a str, f32)>) -> Self {
        let mut c = *self;
        for (id, value) in values {
            if let Some(p) = P::by_id(id) {
                c.0[p as usize] = p.spec().clamp(value);
            }
        }
        c
    }

    pub fn to_pairs(&self) -> Vec<(&'static str, f32)> {
        SPECS.iter().zip(self.0.iter()).map(|(s, v)| (s.id, *v)).collect()
    }

    pub fn from_pairs<'a>(values: impl IntoIterator<Item = (&'a str, f32)>) -> Self {
        Self::defaults().with_all(values)
    }

    /// These settings with the recording settings (noise reduction, pitch range, voicing, gate) taken from `other`.
    pub fn with_input_from(&self, other: &VoiceParams) -> Self {
        let mut c = *self;
        for (i, s) in SPECS.iter().enumerate() {
            if s.input {
                c.0[i] = other.0[i];
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scales::{NOTE_NAMES, SCALE_NAMES};

    #[test]
    fn spec_ids_are_unique_and_defaults_in_range() {
        let mut ids: Vec<_> = SPECS.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(COUNT, ids.len());
        for (i, s) in SPECS.iter().enumerate() {
            assert!(s.min < s.max, "{} min<max", s.id);
            assert!(s.default >= s.min && s.default <= s.max, "{} default in range", s.id);
            assert_eq!(s.default, s.clamp(s.default), "{} default survives clamp", s.id);
            assert_eq!(i, P::ALL[i] as usize);
        }
        assert_eq!(&NOTE_NAMES[..], P::Key.spec().choices.unwrap());
        assert_eq!(&SCALE_NAMES[..], P::Scale.spec().choices.unwrap());
    }

    #[test]
    fn params_round_trip_and_clamp() {
        let p = VoiceParams::defaults().with(P::Retune, 123.0).with(P::Key, 7.0);
        assert_eq!(p, VoiceParams::from_pairs(p.to_pairs()));
        let wild = VoiceParams::defaults().with(P::Retune, 99999.0).with(P::Shift, -99.0);
        assert_eq!(P::Retune.spec().max, wild.get(P::Retune));
        assert_eq!(P::Shift.spec().min, wild.get(P::Shift));
        assert_eq!(0.0, P::Shift.spec().clamp(0.1 * 120.0 - 12.0));
        assert_eq!(VoiceParams::defaults(), VoiceParams::from_pairs([("doesNotExist", 3.0)]));
    }

    #[test]
    fn formatting_is_readable() {
        assert_eq!("20 ms", P::Retune.spec().format(20.0));
        assert_eq!("0.0 st", P::Shift.spec().format(0.0));
        assert_eq!("A3", P::MonoNote.spec().format(57.0));
        assert_eq!("On", P::Monotone.spec().format(1.0));
        assert_eq!("D", P::Key.spec().format(2.0));
    }
}

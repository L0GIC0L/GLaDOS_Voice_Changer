use std::sync::LazyLock;

use super::library::LIBRARY;
use super::VoiceParams;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PresetCategory {
    Glados,
    Aperture,
    Machines,
    Characters,
    Utility,
}

impl PresetCategory {
    pub const ALL: [PresetCategory; 5] =
        [PresetCategory::Glados, PresetCategory::Aperture, PresetCategory::Machines, PresetCategory::Characters, PresetCategory::Utility];

    pub fn title(self) -> &'static str {
        match self {
            PresetCategory::Glados => "GLaDOS",
            PresetCategory::Aperture => "Aperture Science",
            PresetCategory::Machines => "Robots and radios",
            PresetCategory::Characters => "Characters",
            PresetCategory::Utility => "Utility",
        }
    }
}

/// A preset as written in the library: overrides on top of the defaults (the Portal 1 voice), or on top of
/// [`CLEAN`] when `clean` is set.
pub(super) struct PresetDef {
    pub name: &'static str,
    pub category: PresetCategory,
    pub blurb: &'static str,
    pub clean: bool,
    pub overrides: &'static [(&'static str, f32)],
}

/// Everything off: no tuning, no robot, no glitches, no room, flat EQ. Non-GLaDOS presets build on this.
const CLEAN: &[(&str, f32)] = &[
    ("autoReg", 0.0), ("strength", 0.0), ("formant", 0.0), ("formantFollow", 0.0),
    ("ringMix", 0.0), ("combMix", 0.0), ("crushMix", 0.0), ("glitch", 0.0),
    ("reverbMix", 0.0), ("hpf", 70.0), ("lpf", 18000.0), ("lowDb", 0.0), ("midDb", 0.0),
];

pub struct Preset {
    pub name: &'static str,
    pub category: PresetCategory,
    pub blurb: &'static str,
    params: VoiceParams,
}

impl Preset {
    /// The preset's settings, with the recording settings left at their defaults.
    pub fn apply(&self) -> VoiceParams {
        self.params
    }

    /// The preset's voice, keeping the recording settings (noise reduction, pitch range, voicing, gate) from `current`.
    pub fn apply_to(&self, current: &VoiceParams) -> VoiceParams {
        self.params.with_input_from(current)
    }

    pub fn find(name: &str) -> Option<&'static Preset> {
        PRESETS.iter().find(|p| p.name == name)
    }

    /// The built-in preset whose voice settings are exactly those in `p`, if any. Recording settings are ignored.
    pub fn matching(p: &VoiceParams) -> Option<&'static Preset> {
        PRESETS.iter().find(|preset| preset.apply_to(p) == *p)
    }

    pub fn in_category(c: PresetCategory) -> impl Iterator<Item = &'static Preset> {
        PRESETS.iter().filter(move |p| p.category == c)
    }
}

pub static PRESETS: LazyLock<Vec<Preset>> = LazyLock::new(|| {
    LIBRARY
        .iter()
        .map(|d| {
            let base = if d.clean { VoiceParams::defaults().with_all(CLEAN.iter().copied()) } else { VoiceParams::defaults() };
            Preset { name: d.name, category: d.category, blurb: d.blurb, params: base.with_all(d.overrides.iter().copied()) }
        })
        .collect()
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::P;

    #[test]
    fn presets_only_reference_known_voice_settings_in_range() {
        for d in LIBRARY.iter() {
            for (id, v) in d.overrides.iter().chain(if d.clean { CLEAN } else { &[] }) {
                let p = P::by_id(id).unwrap_or_else(|| panic!("preset {} references unknown setting {id}", d.name));
                let s = p.spec();
                assert!(!s.input, "preset {} sets the recording setting {id}", d.name);
                assert!(*v >= s.min && *v <= s.max, "{}.{id}={v} out of range", d.name);
            }
        }
        assert_eq!(VoiceParams::defaults(), PRESETS[0].apply());
        let mut names: Vec<_> = PRESETS.iter().map(|p| p.name.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(PRESETS.len(), names.len(), "preset names must be unique");
        assert_eq!(46, PRESETS.len());
        for p in PRESETS.iter() {
            assert_eq!(Some(p.name), Preset::matching(&p.apply()).map(|m| m.name));
        }
        // Recording settings belong to the clip, not the voice: presets keep them and still match.
        let noisy = VoiceParams::defaults().with(P::Denoise, 90.0).with(P::Fmin, 60.0);
        let applied = Preset::find("Announcer").unwrap().apply_to(&noisy);
        assert_eq!(90.0, applied.get(P::Denoise));
        assert_eq!(60.0, applied.get(P::Fmin));
        assert_eq!(Some("Announcer"), Preset::matching(&applied).map(|m| m.name));
    }

    #[test]
    fn glados_presets_keep_the_speakers_own_pitch() {
        for p in Preset::in_category(PresetCategory::Glados) {
            let v = p.apply();
            assert!(!v.bool(P::AutoReg), "{} moves the voice to a target register", p.name);
            assert!(v.get(P::Shift) <= 0.0, "{} pitches the voice up", p.name);
            if v.bool(P::Monotone) {
                assert!(v.bool(P::MonoAuto), "{} holds a fixed note", p.name);
            }
        }
    }
}

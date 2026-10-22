//! Everything the app remembers between runs, as JSON in ~/.config/glados-voice/settings.json.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glados_dsp::model::VoiceParams;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub params: BTreeMap<String, f32>,
    pub user_presets: BTreeMap<String, BTreeMap<String, f32>>,
    pub color_scheme: ColorScheme,
    /// Use the Aperture orange accent instead of the desktop's.
    pub aperture_accent: bool,
    pub simple_mode: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub tts_piper: bool,
    pub tts_voice: String,
    pub tts_female: bool,
    pub piper_model: String,
    pub tts_rate: f32,
    pub tts_pitch: f32,
    pub tts_tune: bool,
    pub tts_text: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            params: BTreeMap::new(),
            user_presets: BTreeMap::new(),
            color_scheme: ColorScheme::System,
            aperture_accent: true,
            simple_mode: true,
            input_device: None,
            output_device: None,
            tts_piper: false,
            tts_voice: "en-us".into(),
            tts_female: true,
            piper_model: String::new(),
            tts_rate: 1.0,
            tts_pitch: 1.0,
            tts_tune: true,
            tts_text: "Hello, and, again, welcome to the Aperture Science Computer-Aided Enrichment Center.".into(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("GLADOS_VOICE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| gtk4::glib::user_config_dir().join("glados-voice"));
        base.join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            // Write then rename, so a crash mid-write cannot leave a truncated file.
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    pub fn voice_params(&self) -> VoiceParams {
        VoiceParams::from_pairs(self.params.iter().map(|(k, v)| (k.as_str(), *v)))
    }

    pub fn set_voice_params(&mut self, p: &VoiceParams) {
        self.params = p.to_pairs().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    }
}

pub fn pairs_to_params(m: &BTreeMap<String, f32>) -> VoiceParams {
    VoiceParams::from_pairs(m.iter().map(|(k, v)| (k.as_str(), *v)))
}

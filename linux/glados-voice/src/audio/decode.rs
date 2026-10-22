//! Reads any common audio file (wav, mp3, flac, ogg, m4a and more) into mono samples.

use std::fs::File;
use std::path::Path;

use glados_dsp::Audio;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Longest stretch imported; longer files are cut, as on Android.
pub const MAX_SECONDS: u32 = 600;

pub struct Decoded {
    pub audio: Audio,
    pub truncated: bool,
}

pub fn decode(path: &Path) -> Result<Decoded, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("Unrecognised audio file ({e})"))?;
    let track = format.default_track(TrackType::Audio).ok_or("The file has no audio track")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|c| c.audio())
        .ok_or("The audio track cannot be decoded")?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("Unsupported audio format ({e})"))?;

    let mut rate = params.sample_rate.unwrap_or(0);
    let mut mono = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let mut truncated = false;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::ResetRequired) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            // A damaged packet: skip it, as players do.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let spec = buf.spec();
        if rate == 0 {
            rate = spec.rate();
        }
        let channels = spec.channels().count().max(1);
        buf.copy_to_vec_interleaved(&mut interleaved);
        let start = mono.len();
        mono.extend(interleaved.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
        let cap = (MAX_SECONDS * rate.max(1)) as usize;
        if mono.len() >= cap {
            mono.truncate(cap);
            truncated = true;
            break;
        }
        debug_assert!(mono.len() >= start);
    }
    if rate == 0 || mono.is_empty() {
        return Err("The file contains no audio".into());
    }
    Ok(Decoded { audio: Audio { samples: mono, sample_rate: rate as i32 }, truncated })
}

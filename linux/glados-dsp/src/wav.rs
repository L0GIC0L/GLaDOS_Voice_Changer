use std::io::Write;

use crate::kmath::round_f32;

/// Mono floating-point audio with its sample rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: i32,
}

impl Audio {
    pub fn new(samples: Vec<f32>, sample_rate: i32) -> Self {
        Audio { samples, sample_rate }
    }

    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

const TRUNCATED: &str = "Truncated or corrupt WAV file";

/// Little-endian reads that fail, rather than panic, past the end of the data (Java's ByteBuffer bounds checks).
struct Le<'a>(&'a [u8]);

impl Le<'_> {
    fn bytes<const N: usize>(&self, at: i32) -> Result<[u8; N], String> {
        if at < 0 {
            return Err(TRUNCATED.to_string());
        }
        let at = at as usize;
        self.0.get(at..at + N).map(|s| s.try_into().unwrap()).ok_or_else(|| TRUNCATED.to_string())
    }

    fn u8(&self, at: i32) -> Result<i32, String> {
        Ok(self.bytes::<1>(at)?[0] as i8 as i32)
    }

    fn i16(&self, at: i32) -> Result<i32, String> {
        Ok(i16::from_le_bytes(self.bytes(at)?) as i32)
    }

    fn i32(&self, at: i32) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.bytes(at)?))
    }

    fn f32(&self, at: i32) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.bytes(at)?))
    }

    fn tag(&self, at: i32) -> Result<[u8; 4], String> {
        self.bytes(at)
    }
}

/// Minimal RIFF/WAVE reader and writer: PCM 8/16/24/32-bit and float32 in, 16-bit mono out.
pub struct Wav;

impl Wav {
    /// Fails with a message if the data is not a readable WAV file (including truncated or corrupt ones).
    pub fn read(bytes: &[u8]) -> Result<Audio, String> {
        let b = Le(bytes);
        if !(bytes.len() >= 12 && &b.tag(0)? == b"RIFF" && &b.tag(8)? == b"WAVE") {
            return Err("Not a WAV file".to_string());
        }
        let size = bytes.len() as i64;
        let mut pos: i32 = 12;
        let mut format = 1;
        let mut channels = 1;
        let mut rate = 44100;
        let mut bits = 16;
        let mut data_start: i32 = -1;
        let mut data_len: i32 = 0;
        while pos as i64 + 8 <= size {
            let id = b.tag(pos)?;
            let declared = b.i32(pos + 4)? as u32 as i64;
            let body = pos + 8;
            let remaining = size - body as i64;
            // Streaming writers (some Android TTS engines) leave the length at 0 or -1, and a corrupt header can
            // claim more than the file holds. Clamp in 64 bits so the sum cannot overflow.
            let len = if declared > remaining || (&id == b"data" && declared == 0) { remaining as i32 } else { declared as i32 };
            match &id {
                b"fmt " => {
                    format = b.i16(body)? & 0xFFFF;
                    channels = b.i16(body + 2)?;
                    rate = b.i32(body + 4)?;
                    bits = b.i16(body + 14)?;
                    if format == 0xFFFE && len >= 26 {
                        format = b.i16(body + 24)? & 0xFFFF;
                    }
                }
                b"data" => {
                    data_start = body;
                    data_len = len;
                }
                _ => {}
            }
            if data_start >= 0 {
                break;
            }
            pos = body + len + (len & 1);
        }
        if data_start < 0 {
            return Err("WAV has no data chunk".to_string());
        }
        if !(format == 1 || format == 3) {
            return Err(format!("Unsupported WAV format {format}"));
        }
        if !(1..=32).contains(&channels) {
            return Err(format!("Unsupported channel count {channels}"));
        }
        if !(bits == 8 || bits == 16 || bits == 24 || bits == 32) {
            return Err(format!("Unsupported bit depth {bits}"));
        }
        let bytes_per = bits / 8;
        let frames = data_len / (bytes_per * channels);
        let mut out = vec![0f32; frames.max(0) as usize];
        for (i, o) in out.iter_mut().enumerate() {
            let mut sum = 0f32;
            for c in 0..channels {
                let at = data_start + (i as i32 * channels + c) * bytes_per;
                sum += if format == 3 && bits == 32 {
                    b.f32(at)?
                } else if bits == 8 {
                    ((b.u8(at)? & 0xFF) - 128) as f32 / 128f32
                } else if bits == 16 {
                    b.i16(at)? as f32 / 32768f32
                } else if bits == 24 {
                    ((b.u8(at + 2)? << 16) | ((b.u8(at + 1)? & 0xFF) << 8) | (b.u8(at)? & 0xFF)) as f32 / 8388608f32
                } else {
                    b.i32(at)? as f32 / 2147483648f32
                };
            }
            *o = sum / channels as f32;
        }
        Ok(Audio { samples: out, sample_rate: rate })
    }

    pub fn encode(audio: &Audio) -> Vec<u8> {
        let mut out = Vec::with_capacity(44 + audio.samples.len() * 2);
        Wav::write(&mut out, audio).expect("writing to a Vec cannot fail");
        out
    }

    pub fn write(out: &mut impl Write, audio: &Audio) -> std::io::Result<()> {
        let n = audio.samples.len() as i32;
        let mut data = Vec::with_capacity(audio.samples.len() * 2);
        for &v in &audio.samples {
            // NaN is written as silence (Kotlin's roundToInt would throw).
            let s = if v.is_nan() { 0 } else { round_f32(v.clamp(-1.0, 1.0) * 32767f32) as i16 };
            data.extend_from_slice(&s.to_le_bytes());
        }
        let mut head = Vec::with_capacity(44);
        head.extend_from_slice(b"RIFF");
        head.extend_from_slice(&(36i32.wrapping_add(n.wrapping_mul(2))).to_le_bytes());
        head.extend_from_slice(b"WAVE");
        head.extend_from_slice(b"fmt ");
        head.extend_from_slice(&16i32.to_le_bytes());
        head.extend_from_slice(&1i16.to_le_bytes());
        head.extend_from_slice(&1i16.to_le_bytes());
        head.extend_from_slice(&audio.sample_rate.to_le_bytes());
        head.extend_from_slice(&audio.sample_rate.wrapping_mul(2).to_le_bytes());
        head.extend_from_slice(&2i16.to_le_bytes());
        head.extend_from_slice(&16i16.to_le_bytes());
        head.extend_from_slice(b"data");
        head.extend_from_slice(&n.wrapping_mul(2).to_le_bytes());
        out.write_all(&head)?;
        out.write_all(&data)
    }
}

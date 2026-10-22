//! Records the microphone into memory, up to a length cap.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, StreamTrait};
use glados_dsp::Audio;

use super::{devices, downmix, AtomicF32};

pub const MAX_SECONDS: u32 = 120;

pub struct Recorder {
    stream: Option<cpal::Stream>,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    capped: Arc<AtomicBool>,
    level: Arc<AtomicF32>,
}

impl Default for Recorder {
    fn default() -> Self {
        Recorder {
            stream: None,
            samples: Arc::new(Mutex::new(Vec::new())),
            rate: 48000,
            capped: Arc::new(AtomicBool::new(false)),
            level: Arc::new(AtomicF32::default()),
        }
    }
}

impl Recorder {
    pub fn start(&mut self, device_id: Option<&str>) -> Result<(), String> {
        self.stop();
        let device = devices::find(device_id, true).ok_or("No microphone was found")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let config = cpal::StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        let channels = config.channels as usize;
        self.rate = config.sample_rate;
        let cap = (MAX_SECONDS * self.rate) as usize;
        self.samples.lock().unwrap().clear();
        self.capped.store(false, Ordering::Relaxed);
        let samples = self.samples.clone();
        let capped = self.capped.clone();
        let level = self.level.clone();
        let mut mono = Vec::new();
        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _| {
                    downmix(data, channels, &mut mono);
                    level.store(mono.iter().fold(0.0f32, |m, v| m.max(v.abs())));
                    let mut s = samples.lock().unwrap();
                    let room = cap.saturating_sub(s.len());
                    s.extend_from_slice(&mono[..mono.len().min(room)]);
                    if s.len() >= cap {
                        capped.store(true, Ordering::Relaxed);
                    }
                },
                |e| eprintln!("glados-voice: recording error: {e}"),
                None,
            )
            .map_err(|e| format!("Could not open the microphone: {e}"))?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    /// True once the length cap is reached; the caller should then [`stop`](Self::stop).
    pub fn is_capped(&self) -> bool {
        self.capped.load(Ordering::Relaxed)
    }

    pub fn level(&self) -> f32 {
        self.level.load()
    }

    pub fn seconds(&self) -> f32 {
        self.samples.lock().unwrap().len() as f32 / self.rate as f32
    }

    /// Stops and returns what was recorded, or None if nothing was.
    pub fn stop(&mut self) -> Option<Audio> {
        let stream = self.stream.take()?;
        drop(stream);
        self.level.store(0.0);
        let samples = std::mem::take(&mut *self.samples.lock().unwrap());
        (samples.len() > (self.rate / 10) as usize).then_some(Audio { samples, sample_rate: self.rate as i32 })
    }
}

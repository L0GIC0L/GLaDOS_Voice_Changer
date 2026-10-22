//! Plays the rendered clip through the effect chain in real time, so effect sliders apply while it plays.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, StreamTrait};
use glados_dsp::model::VoiceParams;
use glados_dsp::FxChain;

use super::{devices, upmix};

struct Shared {
    clip: Mutex<Arc<Vec<f32>>>,
    clip_gen: AtomicU64,
    clip_rate: AtomicU32,
    /// Playback position in clip samples.
    position: AtomicU64,
    /// A requested jump, in clip samples, or -1.
    seek_to: AtomicI64,
    playing: AtomicBool,
    looping: AtomicBool,
    /// Set by the audio thread when the end and the effect tail have played out.
    finished: AtomicBool,
    params: Mutex<VoiceParams>,
    params_gen: AtomicU64,
}

pub struct ClipPlayer {
    shared: Arc<Shared>,
    stream: RefCell<Option<cpal::Stream>>,
    device_id: RefCell<Option<String>>,
}

impl ClipPlayer {
    pub fn new(params: VoiceParams) -> Self {
        ClipPlayer {
            shared: Arc::new(Shared {
                clip: Mutex::new(Arc::new(Vec::new())),
                clip_gen: AtomicU64::new(1),
                clip_rate: AtomicU32::new(48000),
                position: AtomicU64::new(0),
                seek_to: AtomicI64::new(-1),
                playing: AtomicBool::new(false),
                looping: AtomicBool::new(false),
                finished: AtomicBool::new(false),
                params: Mutex::new(params),
                params_gen: AtomicU64::new(1),
            }),
            stream: RefCell::new(None),
            device_id: RefCell::new(None),
        }
    }

    pub fn set_params(&self, p: VoiceParams) {
        *self.shared.params.lock().unwrap() = p;
        self.shared.params_gen.fetch_add(1, Ordering::Release);
    }

    /// Plays to a different output device from the next [`play`](Self::play) on.
    pub fn set_device(&self, id: Option<String>) {
        if *self.device_id.borrow() != id {
            self.device_id.replace(id);
            let was_playing = self.is_playing();
            self.stream.replace(None);
            if was_playing {
                let _ = self.play(false);
            }
        }
    }

    /// Replaces the clip. The playhead is kept if it still fits.
    pub fn set_clip(&self, samples: Arc<Vec<f32>>, rate: i32) {
        let len = samples.len() as u64;
        *self.shared.clip.lock().unwrap() = samples;
        self.shared.clip_rate.store(rate.max(1) as u32, Ordering::Relaxed);
        self.shared.clip_gen.fetch_add(1, Ordering::Release);
        if self.shared.position.load(Ordering::Relaxed) > len {
            self.seek_samples(0);
        }
    }

    pub fn play(&self, from_start: bool) -> Result<(), String> {
        if from_start {
            self.seek_samples(0);
        }
        self.shared.finished.store(false, Ordering::Relaxed);
        if self.stream.borrow().is_none() {
            let stream = self.open()?;
            self.stream.replace(Some(stream));
        }
        if let Some(s) = self.stream.borrow().as_ref() {
            s.play().map_err(|e| e.to_string())?;
        }
        self.shared.playing.store(true, Ordering::Release);
        Ok(())
    }

    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Release);
        if let Some(s) = self.stream.borrow().as_ref() {
            let _ = s.pause();
        }
    }

    pub fn stop(&self) {
        self.pause();
        self.seek_samples(0);
    }

    pub fn toggle(&self) -> Result<(), String> {
        if self.is_playing() {
            self.pause();
            Ok(())
        } else {
            self.play(false)
        }
    }

    pub fn seek(&self, seconds: f32) {
        let rate = self.shared.clip_rate.load(Ordering::Relaxed) as f32;
        self.seek_samples((seconds.max(0.0) * rate) as u64);
    }

    fn seek_samples(&self, s: u64) {
        let len = self.shared.clip.lock().unwrap().len() as u64;
        let s = s.min(len);
        self.shared.position.store(s, Ordering::Relaxed);
        self.shared.seek_to.store(s as i64, Ordering::Release);
    }

    pub fn set_loop(&self, on: bool) {
        self.shared.looping.store(on, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    pub fn position_seconds(&self) -> f32 {
        self.shared.position.load(Ordering::Relaxed) as f32 / self.shared.clip_rate.load(Ordering::Relaxed) as f32
    }

    pub fn duration_seconds(&self) -> f32 {
        self.shared.clip.lock().unwrap().len() as f32 / self.shared.clip_rate.load(Ordering::Relaxed) as f32
    }

    /// Call regularly from the UI: pauses the device once a clip has finished playing.
    pub fn poll(&self) {
        if self.shared.finished.swap(false, Ordering::AcqRel) {
            if let Some(s) = self.stream.borrow().as_ref() {
                let _ = s.pause();
            }
        }
    }

    fn open(&self) -> Result<cpal::Stream, String> {
        let device = devices::find(self.device_id.borrow().as_deref(), false).ok_or("No sound output device was found")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = cpal::StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        let channels = config.channels as usize;
        let rate = config.sample_rate;
        let shared = self.shared.clone();
        let mut fx = FxChain::new(rate as i32);
        let mut fx_gen = 0u64;
        let mut clip: Arc<Vec<f32>> = Arc::new(Vec::new());
        let mut clip_gen = 0u64;
        let mut step = 1.0;
        let mut pos = 0.0f64;
        let mut tail_left: i64 = -1;
        let mut tail_params = VoiceParams::defaults();
        let mut mono: Vec<f32> = Vec::new();
        device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let frames = data.len() / channels.max(1);
                    let g = shared.params_gen.load(Ordering::Acquire);
                    if g != fx_gen {
                        if let Ok(p) = shared.params.try_lock() {
                            fx.configure(&p);
                            tail_params = *p;
                            fx_gen = g;
                        }
                    }
                    let cg = shared.clip_gen.load(Ordering::Acquire);
                    if cg != clip_gen {
                        if let Ok(c) = shared.clip.try_lock() {
                            clip = c.clone();
                            clip_gen = cg;
                            step = shared.clip_rate.load(Ordering::Relaxed) as f64 / rate as f64;
                        }
                    }
                    let seek = shared.seek_to.swap(-1, Ordering::AcqRel);
                    if seek >= 0 {
                        pos = seek as f64;
                        tail_left = -1;
                        fx.reset();
                    }
                    if !shared.playing.load(Ordering::Acquire) {
                        data.fill(0.0);
                        return;
                    }
                    mono.clear();
                    mono.resize(frames, 0.0);
                    let len = clip.len();
                    for m in mono.iter_mut() {
                        let i = pos as usize;
                        if i + 1 < len {
                            let f = (pos - i as f64) as f32;
                            *m = clip[i] + (clip[i + 1] - clip[i]) * f;
                            pos += step;
                        } else if i < len {
                            *m = clip[i];
                            pos += step;
                        } else if shared.looping.load(Ordering::Relaxed) && len > 0 {
                            pos = 0.0;
                        }
                    }
                    if pos as usize >= len {
                        if shared.looping.load(Ordering::Relaxed) {
                            pos = 0.0;
                        } else {
                            if tail_left < 0 {
                                tail_left = (FxChain::tail_seconds(&tail_params) * rate as f64) as i64;
                            }
                            tail_left -= frames as i64;
                            if tail_left <= 0 {
                                shared.playing.store(false, Ordering::Release);
                                shared.finished.store(true, Ordering::Release);
                                pos = 0.0;
                                tail_left = -1;
                            }
                        }
                    }
                    fx.process(&mut mono);
                    upmix(&mono, channels, data);
                    shared.position.store((pos as u64).min(len as u64), Ordering::Relaxed);
                },
                |e| eprintln!("glados-voice: playback error: {e}"),
                None,
            )
            .map_err(|e| format!("Could not open the sound output: {e}"))
    }
}

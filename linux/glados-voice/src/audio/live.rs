//! Live voice: microphone -> noise gate and pitch engine -> effects -> output, in real time.
//!
//! cpal gives the microphone and the output their own callbacks, so the microphone side only pushes samples into a
//! lock-free ring buffer and all processing happens in the output callback, which keeps the delay steady.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use glados_dsp::model::VoiceParams;
use glados_dsp::{FxChain, LivePitchEngine};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;

use super::{devices, downmix, upmix, AtomicF32};

/// Points kept in the pitch trace, one per [`TRACE_MS`] milliseconds.
pub const TRACE_POINTS: usize = 400;
pub const TRACE_MS: u32 = 10;

/// What the UI reads while live. Written by the audio thread.
#[derive(Default)]
pub struct Telemetry {
    pub input_level: AtomicF32,
    pub output_level: AtomicF32,
    pub input_hz: AtomicF32,
    pub output_hz: AtomicF32,
    pub target_midi: AtomicI32,
    /// True while the latest analysis frame is pitched voice.
    pub voice_detected: AtomicBool,
    /// The microphone has heard sound for a while but no pitched voice: noise, whispering, or settings too strict.
    pub no_voice_warning: AtomicBool,
    pub latency_samples: AtomicU32,
    /// Microphone audio kept in hand against timing jitter, in samples. Grows after dropouts.
    pub buffer_samples: AtomicU32,
    pub dropouts: AtomicU64,
    /// (input Hz, output Hz) pairs, 0 where unvoiced; newest last.
    pub trace: Mutex<VecDeque<(f32, f32)>>,
}

struct Shared {
    params: Mutex<VoiceParams>,
    params_gen: AtomicU64,
    bypass: AtomicBool,
    muted: AtomicBool,
    error: Mutex<Option<String>>,
    /// Callback counts for the microphone and the output, so a stream that silently stops can be noticed.
    beats: [AtomicU64; 2],
}

impl Shared {
    /// cpal recovers from underruns and passing backend errors by itself and keeps the stream running, so those only
    /// count as dropouts. Errors that end the stream are reported to the UI.
    fn stream_error(&self, telemetry: &Telemetry, what: &str, e: cpal::Error) {
        use cpal::ErrorKind::*;
        match e.kind() {
            Xrun | BackendError | DeviceBusy | DeviceChanged | RealtimeDenied => {
                telemetry.dropouts.fetch_add(1, Ordering::Relaxed);
            }
            _ => *self.error.lock().unwrap() = Some(format!("{what} stopped: {e}")),
        }
    }
}

/// How long a stream may go without a callback before live mode reports it as stopped.
const STALL: Duration = Duration::from_secs(2);

/// The processing done for each output buffer, separate from the devices so it can be tested.
pub struct LiveProcessor {
    sample_rate: u32,
    engine: LivePitchEngine,
    fx: FxChain,
    out: Vec<f32>,
    loud_unpitched: u64,
    trace_acc: u32,
}

impl LiveProcessor {
    pub fn new(sample_rate: u32, params: &VoiceParams) -> Self {
        let mut fx = FxChain::new(sample_rate as i32);
        fx.configure(params);
        LiveProcessor {
            sample_rate,
            engine: LivePitchEngine::new(sample_rate as i32, params),
            fx,
            out: Vec::new(),
            loud_unpitched: 0,
            trace_acc: 0,
        }
    }

    pub fn configure(&mut self, p: &VoiceParams) {
        self.engine.configure(p);
        self.fx.configure(p);
    }

    pub fn latency_samples(&self) -> u32 {
        self.engine.latency_samples().max(0) as u32
    }

    /// Processes one block of microphone samples into `output` (same length).
    pub fn process(&mut self, input: &[f32], output: &mut [f32], bypass: bool, muted: bool, t: &Telemetry) {
        let n = input.len();
        let peak_in = input.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if bypass {
            output.copy_from_slice(input);
        } else {
            self.out.resize(n, 0.0);
            self.engine.process(input, &mut self.out);
            self.fx.process(&mut self.out);
            output.copy_from_slice(&self.out);
        }
        if muted {
            output.fill(0.0);
        }
        let peak_out = output.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        t.input_level.store(peak_in);
        t.output_level.store(peak_out);

        let in_hz = self.engine.input_hz() as f32;
        let out_hz = if bypass { in_hz } else { self.engine.output_hz() as f32 };
        let pitched = in_hz > 0.0;
        t.input_hz.store(in_hz);
        t.output_hz.store(out_hz);
        t.target_midi.store(self.engine.target_midi(), Ordering::Relaxed);
        t.voice_detected.store(pitched && !bypass, Ordering::Relaxed);
        // Sustained loud-but-unpitched input means detection is failing, not that the speaker paused.
        self.loud_unpitched = if peak_in > 0.02 && !pitched { self.loud_unpitched + n as u64 } else { 0 };
        t.no_voice_warning.store(self.loud_unpitched > self.sample_rate as u64 * 3 / 2, Ordering::Relaxed);
        t.latency_samples.store(self.latency_samples(), Ordering::Relaxed);

        self.trace_acc += n as u32;
        let every = self.sample_rate * TRACE_MS / 1000;
        if self.trace_acc >= every {
            self.trace_acc %= every.max(1);
            if let Ok(mut tr) = t.trace.try_lock() {
                if tr.len() >= TRACE_POINTS {
                    tr.pop_front();
                }
                tr.push_back((in_hz, out_hz));
            }
        }
    }
}

pub struct LiveController {
    shared: Arc<Shared>,
    telemetry: Arc<Telemetry>,
    streams: Option<(cpal::Stream, cpal::Stream)>,
    sample_rate: u32,
    /// The callback counts last seen and when they last moved.
    watch: Cell<([u64; 2], Instant)>,
}

impl LiveController {
    pub fn new(params: VoiceParams) -> Self {
        LiveController {
            shared: Arc::new(Shared {
                params: Mutex::new(params),
                params_gen: AtomicU64::new(1),
                bypass: AtomicBool::new(false),
                muted: AtomicBool::new(false),
                error: Mutex::new(None),
                beats: Default::default(),
            }),
            telemetry: Arc::new(Telemetry { target_midi: AtomicI32::new(i32::MIN), ..Default::default() }),
            streams: None,
            sample_rate: 0,
            watch: Cell::new(([0; 2], Instant::now())),
        }
    }

    pub fn telemetry(&self) -> &Telemetry {
        &self.telemetry
    }

    pub fn set_params(&self, p: VoiceParams) {
        *self.shared.params.lock().unwrap() = p;
        self.shared.params_gen.fetch_add(1, Ordering::Release);
    }

    pub fn set_bypass(&self, on: bool) {
        self.shared.bypass.store(on, Ordering::Relaxed);
    }

    pub fn set_muted(&self, on: bool) {
        self.shared.muted.store(on, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.streams.is_some()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Total delay from microphone to speaker that this app adds (the sound system adds its own on top).
    pub fn latency_ms(&self) -> u32 {
        if self.sample_rate == 0 {
            return 0;
        }
        let engine = self.telemetry.latency_samples.load(Ordering::Relaxed);
        (engine + self.telemetry.buffer_samples.load(Ordering::Relaxed)) * 1000 / self.sample_rate
    }

    /// An error reported by the audio thread since the last call, such as a device being unplugged, or a stream that
    /// has stopped calling back. Call it regularly while live.
    pub fn take_error(&self) -> Option<String> {
        if let Some(e) = self.shared.error.lock().unwrap().take() {
            return Some(e);
        }
        self.streams.as_ref()?;
        let beats = self.shared.beats.each_ref().map(|b| b.load(Ordering::Relaxed));
        let (seen, since) = self.watch.get();
        let now = Instant::now();
        if beats[0] != seen[0] && beats[1] != seen[1] {
            self.watch.set((beats, now));
        } else if now - since > STALL {
            self.watch.set((beats, now));
            let what = if beats[1] == seen[1] { "The sound output" } else { "The microphone" };
            return Some(format!("{what} stopped responding."));
        }
        None
    }

    pub fn start(&mut self, input_id: Option<&str>, output_id: Option<&str>) -> Result<(), String> {
        self.stop();
        let output = devices::find(output_id, false).ok_or("No sound output device was found")?;
        let input = devices::find(input_id, true).ok_or("No microphone was found")?;
        let out_cfg = output.default_output_config().map_err(|e| format!("Output device: {e}"))?;
        let rate = out_cfg.sample_rate();
        let in_cfg = input.default_input_config().map_err(|e| format!("Microphone: {e}"))?;
        // Both sides must run at the same rate. PipeWire and PulseAudio convert for us; raw hardware may not.
        let in_ok = in_cfg.sample_rate() == rate
            || input
                .supported_input_configs()
                .map(|mut it| it.any(|c| c.min_sample_rate() <= rate && rate <= c.max_sample_rate()))
                .unwrap_or(false);
        if !in_ok {
            return Err(format!(
                "The microphone cannot run at {rate} Hz like the output. Pick the \"default\" or \"pipewire\" devices, which convert automatically."
            ));
        }
        let in_channels = in_cfg.channels() as usize;
        let out_channels = out_cfg.channels() as usize;

        // A little audio is kept in hand so the output never waits on the microphone: 15 ms to start with, raised a
        // step each time the microphone is late, up to 120 ms. Anything beyond 4x that is dropped.
        let mut target = (rate * 15 / 1000) as usize;
        let max_target = (rate * 120 / 1000) as usize;
        self.telemetry.buffer_samples.store(target as u32, Ordering::Relaxed);
        let ring = HeapRb::<f32>::new(rate as usize);
        let (mut prod, mut cons) = ring.split();

        let params = *self.shared.params.lock().unwrap();
        let mut proc = LiveProcessor::new(rate, &params);
        self.telemetry.latency_samples.store(proc.latency_samples(), Ordering::Relaxed);
        self.telemetry.trace.lock().unwrap().clear();

        let shared_in = self.shared.clone();
        let shared_in_err = self.shared.clone();
        let telemetry_in = self.telemetry.clone();
        let mut mono_in = Vec::new();
        let in_stream = input
            .build_input_stream(
                cpal::StreamConfig { channels: in_channels as u16, sample_rate: rate, buffer_size: cpal::BufferSize::Default },
                move |data: &[f32], _| {
                    shared_in.beats[0].fetch_add(1, Ordering::Relaxed);
                    downmix(data, in_channels, &mut mono_in);
                    // A NaN or Inf from a misbehaving driver would otherwise poison the filters for good.
                    for v in mono_in.iter_mut() {
                        if !v.is_finite() {
                            *v = 0.0;
                        }
                    }
                    prod.push_slice(&mono_in);
                },
                move |e| shared_in_err.stream_error(&telemetry_in, "The microphone", e),
                None,
            )
            .map_err(|e| format!("Could not open the microphone: {e}"))?;

        let shared = self.shared.clone();
        let telemetry = self.telemetry.clone();
        let shared_err = self.shared.clone();
        let telemetry_err = self.telemetry.clone();
        let mut applied_gen = self.shared.params_gen.load(Ordering::Acquire);
        let mut primed = false;
        let mut block_in = Vec::new();
        let mut block_out = Vec::new();
        let out_stream = output
            .build_output_stream(
                cpal::StreamConfig { channels: out_channels as u16, sample_rate: rate, buffer_size: cpal::BufferSize::Default },
                move |data: &mut [f32], _| {
                    shared.beats[1].fetch_add(1, Ordering::Relaxed);
                    let frames = data.len() / out_channels.max(1);
                    let g = shared.params_gen.load(Ordering::Acquire);
                    if g != applied_gen {
                        if let Ok(p) = shared.params.try_lock() {
                            proc.configure(&p);
                            applied_gen = g;
                        }
                    }
                    let available = cons.occupied_len();
                    if !primed {
                        if available < target + frames {
                            data.fill(0.0);
                            return;
                        }
                        primed = true;
                    }
                    // Input and output clocks drift apart slowly; drop the excess rather than let the delay grow.
                    if available > target * 4 + frames {
                        cons.skip(available - target - frames);
                    }
                    block_in.resize(frames, 0.0);
                    block_out.resize(frames, 0.0);
                    let got = cons.pop_slice(&mut block_in);
                    if got < frames {
                        block_in[got..].fill(0.0);
                        telemetry.dropouts.fetch_add(1, Ordering::Relaxed);
                        primed = false;
                        target = (target * 3 / 2).min(max_target);
                        telemetry.buffer_samples.store(target as u32, Ordering::Relaxed);
                    }
                    proc.process(
                        &block_in,
                        &mut block_out,
                        shared.bypass.load(Ordering::Relaxed),
                        shared.muted.load(Ordering::Relaxed),
                        &telemetry,
                    );
                    upmix(&block_out, out_channels, data);
                },
                move |e| shared_err.stream_error(&telemetry_err, "The sound output", e),
                None,
            )
            .map_err(|e| format!("Could not open the sound output: {e}"))?;

        in_stream.play().map_err(|e| format!("Could not start the microphone: {e}"))?;
        out_stream.play().map_err(|e| format!("Could not start the sound output: {e}"))?;
        self.sample_rate = rate;
        self.streams = Some((in_stream, out_stream));
        self.watch.set(([u64::MAX; 2], Instant::now()));
        Ok(())
    }

    pub fn stop(&mut self) {
        self.streams = None;
        let t = &self.telemetry;
        t.input_level.store(0.0);
        t.output_level.store(0.0);
        t.input_hz.store(0.0);
        t.output_hz.store(0.0);
        t.voice_detected.store(false, Ordering::Relaxed);
        t.no_voice_warning.store(false, Ordering::Relaxed);
        t.target_midi.store(i32::MIN, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glados_dsp::model::PRESETS;

    /// A gliding, vowel-like tone with pauses: something every preset has work to do on.
    fn voice(sr: u32, seconds: f32) -> Vec<f32> {
        let mut phase = 0.0f64;
        (0..(sr as f32 * seconds) as usize)
            .map(|i| {
                let t = i as f64 / sr as f64;
                let hz = 180.0 + 40.0 * (t * 1.3).sin();
                phase += std::f64::consts::TAU * hz / sr as f64;
                let on = if (t % 1.0) < 0.8 { 1.0 } else { 0.0 };
                (on * 0.3 * (1..8).map(|k| (k as f64 * phase).sin() / k as f64).sum::<f64>()) as f32
            })
            .collect()
    }

    #[test]
    fn every_preset_runs_well_inside_real_time() {
        let sr = 48000;
        let x = voice(sr, 4.0);
        let t = Telemetry::default();
        let mut out = vec![0f32; 480];
        let mut worst = (0.0f64, "");
        for preset in PRESETS.iter() {
            let mut proc = LiveProcessor::new(sr, &preset.apply());
            let start = Instant::now();
            for block in x.chunks(480) {
                let o = &mut out[..block.len()];
                proc.process(block, o, false, false, &t);
                assert!(o.iter().all(|v| v.is_finite()), "{} produced NaN", preset.name);
            }
            let load = start.elapsed().as_secs_f64() / 4.0;
            if load > worst.0 {
                worst = (load, preset.name);
            }
        }
        eprintln!("heaviest preset: {} at {:.1}% of one core", worst.1, worst.0 * 100.0);
        assert!(worst.0 < 0.5, "{} needs {:.0}% of a core", worst.1, worst.0 * 100.0);
    }
}

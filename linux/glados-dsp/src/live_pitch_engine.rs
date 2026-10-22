use std::f64::consts::PI;

use crate::grain::Grain;
use crate::kmath::{powf, round_f32, round_f64};
use crate::marks::Marks;
use crate::model::{VoiceParams, P};
use crate::pitch_bandpass::PitchBandpass;
use crate::scales::{self, pitch};
use crate::tuner::Tuner;
use crate::voicing::Voicing;
use crate::yin::Yin;

const RING: i32 = 1 << 15;
const MASK: i32 = RING - 1;
const ACC: i32 = 1 << 14;
const AMASK: i32 = ACC - 1;
const FRAMES: i64 = 256;
const MARKS: i64 = 512;
const NONE: i64 = i64::MIN / 4;

/// Absolute positions are rebased well before they could overflow an Int index (2^31 samples is about 12 h at 48 kHz).
const REBASE_AT: i64 = 1 << 30;
const REBASE_BY: i64 = 1 << 29;

/// Streaming pitch corrector and formant shifter (causal TD-PSOLA).
///
/// Per block it (1) ingests input through a noise gate, (2) runs YIN every [`LivePitchEngine::HOP`] samples and feeds
/// the tuner, (3) tracks pitch marks, and (4) lays down windowed grains. Grains are spaced by the
/// *target* period, which sets the pitch, and are cut from the nearest input mark, which keeps
/// the original vocal tract. Stretching or squeezing each grain sets the formants.
///
/// Output is delayed by [`LivePitchEngine::latency_samples`] so that marks and pitch frames are always available.
/// Not thread-safe: call [`LivePitchEngine::configure`] and [`LivePitchEngine::process`] from the same thread.
pub struct LivePitchEngine {
    sample_rate: i32,

    // ---- configuration ----
    fmin: f64,
    fmax: f64,
    yin: Yin,
    d_map: i32,
    a4: f64,
    shift: f64,
    formant_semis: f64,
    formant_follow: f64,
    formant_ratio: f64,
    breath: f32,
    dry: f32,
    gate_lin: f32,
    yin_threshold: f64,
    auto_register: bool,
    register_midi: f64,
    vibrato_depth: f64,
    vibrato_rate: f64,
    tuner: Tuner,

    // ---- input side ----
    in_buf: Vec<f32>,
    ana_buf: Vec<f32>, // band-passed copy, for pitch analysis only
    band: PitchBandpass,
    w_in: i64,
    dc_x: f32,
    dc_y: f32,
    gate_env: f32,
    gate_hold: i32,
    gate_open: bool,
    gate_gain: f32,
    since_detect: i32,

    // ---- pitch frames (one per HOP) ----
    f_time: Vec<i64>,
    f_voiced: Vec<bool>,
    f_in_hz: Vec<f64>,
    f_out_hz: Vec<f64>,
    f_count: i64,
    hang: i32,
    last_f0: f64,
    was_periodic: bool,
    med: [f64; 3],
    med_n: usize,
    vib_phase: f64,
    reg_avg: f64,
    reg_n: i32,
    auto_shift: f64,

    // ---- pitch marks ----
    m_pos: Vec<i64>,
    m_count: i64,
    chain: bool,
    chain_period: f64,
    last_mark_pos: i64,

    // ---- synthesis ----
    acc: Vec<f32>,
    norm: Vec<f32>,
    w_out: i64,
    s_next: f64,

    // ---- telemetry ----
    input_hz: f64,
    output_hz: f64,
    target_midi: i32,
    starved_grains: i32,
}

fn latency_for(sample_rate: i32, fmin: f64) -> i32 {
    round_f64(3.5 * sample_rate as f64 / fmin) + 64
}

impl LivePitchEngine {
    pub const HOP: i32 = 128;

    pub fn new(sample_rate: i32, params: &VoiceParams) -> Self {
        let fmin = params.get(P::Fmin) as f64;
        let fmax = (params.get(P::Fmax) as f64).max(fmin * 2.0);
        let mut e = LivePitchEngine {
            sample_rate,
            fmin,
            fmax,
            yin: Yin::new(sample_rate, fmin, fmax),
            d_map: latency_for(sample_rate, fmin),
            a4: 440.0,
            shift: 0.0,
            formant_semis: 0.0,
            formant_follow: 0.0,
            formant_ratio: 1.0,
            breath: 0.8,
            dry: 0.0,
            gate_lin: 0.001,
            yin_threshold: 0.16,
            auto_register: true,
            register_midi: 57.0,
            vibrato_depth: 0.0,
            vibrato_rate: 5.5,
            tuner: Tuner::new(Self::HOP as f64 / sample_rate as f64),
            in_buf: vec![0f32; RING as usize],
            ana_buf: vec![0f32; RING as usize],
            band: PitchBandpass::new(sample_rate, fmin, fmax),
            w_in: 0,
            dc_x: 0.0,
            dc_y: 0.0,
            gate_env: 0.0,
            gate_hold: 0,
            gate_open: false,
            gate_gain: 0.0,
            since_detect: 0,
            f_time: vec![0; FRAMES as usize],
            f_voiced: vec![false; FRAMES as usize],
            f_in_hz: vec![0.0; FRAMES as usize],
            f_out_hz: vec![0.0; FRAMES as usize],
            f_count: 0,
            hang: 0,
            last_f0: 0.0,
            was_periodic: false,
            med: [0.0; 3],
            med_n: 0,
            vib_phase: 0.0,
            reg_avg: 0.0,
            reg_n: 0,
            auto_shift: 0.0,
            m_pos: vec![0; MARKS as usize],
            m_count: 0,
            chain: false,
            chain_period: 0.0,
            last_mark_pos: NONE,
            acc: vec![0f32; ACC as usize],
            norm: vec![0f32; ACC as usize],
            w_out: 0,
            s_next: 0.0,
            input_hz: 0.0,
            output_hz: 0.0,
            target_midi: i32::MIN,
            starved_grains: 0,
        };
        e.reset_streams();
        e.configure(params);
        e
    }

    /// Latest detected input pitch in Hz, 0 while unvoiced.
    pub fn input_hz(&self) -> f64 {
        self.input_hz
    }

    /// Latest output pitch in Hz, 0 while unvoiced.
    pub fn output_hz(&self) -> f64 {
        self.output_hz
    }

    /// Latest target note (MIDI), or [`i32::MIN`] before the first voiced frame.
    pub fn target_midi(&self) -> i32 {
        self.target_midi
    }

    /// Grains that wanted samples not yet received. Should stay 0; exposed for tests.
    pub fn starved_grains(&self) -> i32 {
        self.starved_grains
    }

    /// Fixed delay between input and output, in samples.
    pub fn latency_samples(&self) -> i32 {
        self.d_map
    }

    pub fn configure(&mut self, p: &VoiceParams) {
        let new_min = p.get(P::Fmin) as f64;
        let new_max = (p.get(P::Fmax) as f64).max(new_min * 2.0);
        if new_min != self.fmin || new_max != self.fmax {
            self.fmin = new_min;
            self.fmax = new_max;
            self.yin = Yin::new(self.sample_rate, self.fmin, self.fmax);
            self.band = PitchBandpass::new(self.sample_rate, self.fmin, self.fmax);
            self.d_map = latency_for(self.sample_rate, self.fmin);
            self.reset_streams();
        }
        self.a4 = p.get(P::RefA4) as f64;
        self.shift = p.get(P::Shift) as f64;
        self.formant_semis = p.get(P::Formant) as f64;
        self.formant_follow = p.get(P::FormantFollow) as f64 / 100.0;
        self.update_formant();
        self.breath = p.get(P::Breath) / 100f32;
        self.dry = p.get(P::Dry) / 100f32;
        self.gate_lin = powf(10f32, p.get(P::Gate) / 20f32);
        self.yin_threshold = Voicing::threshold(p.get(P::Voicing));
        self.auto_register = p.bool(P::AutoReg);
        self.register_midi = pitch::hz_to_midi(p.get(P::RegHz) as f64, self.a4);
        self.vibrato_depth = p.get(P::VibDepth) as f64 / 100.0;
        self.vibrato_rate = p.get(P::VibRate) as f64;

        self.tuner.mask = scales::mask(p.int(P::Scale), p.int(P::Key), round_f32(p.get(P::CustomMask)));
        self.tuner.retune_ms = p.get(P::Retune) as f64;
        self.tuner.strength = p.get(P::Strength) as f64 / 100.0;
        self.tuner.hold_semitones = p.get(P::Hold) as f64 / 100.0;
        self.tuner.monotone = p.bool(P::Monotone);
        self.tuner.monotone_note = p.int(P::MonoNote);
    }

    /// Clears audio state while keeping the configuration.
    pub fn reset_streams(&mut self) {
        self.in_buf.fill(0.0);
        self.ana_buf.fill(0.0);
        self.band.reset();
        self.was_periodic = false;
        self.dc_x = 0.0;
        self.dc_y = 0.0;
        self.gate_env = 0.0;
        self.acc.fill(0.0);
        self.norm.fill(0.0);
        self.w_in = 0;
        self.w_out = 0;
        self.s_next = 0.0;
        self.f_count = 0;
        self.m_count = 0;
        self.chain = false;
        self.last_mark_pos = NONE;
        self.since_detect = 0;
        self.med_n = 0;
        self.hang = 0;
        self.tuner.reset();
        self.gate_gain = 0.0;
        self.gate_open = false;
        self.gate_hold = 0;
        self.starved_grains = 0;
    }

    /// Processes one block: `input` and `output` must be the same length.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len(), "input and output blocks must be the same length");
        let n = input.len() as i32;
        if self.w_in > REBASE_AT {
            self.shift_clock(-REBASE_BY);
        }
        self.ingest(input);
        self.advance_marks();
        self.synthesize(n);
        self.emit(output);
    }

    /// Moves every absolute position by `delta` samples. `delta` must be a multiple of the ring sizes so that
    /// buffer indices are unchanged. Used to rebase before positions grow past what an Int index can hold, and by
    /// tests to simulate many hours of streaming.
    pub(crate) fn shift_clock(&mut self, delta: i64) {
        assert!(delta % RING as i64 == 0, "delta must keep ring indices aligned");
        self.w_in += delta;
        self.w_out += delta;
        self.s_next += delta as f64;
        if self.last_mark_pos != NONE {
            self.last_mark_pos += delta;
        }
        for m in self.m_pos.iter_mut() {
            *m += delta;
        }
        for t in self.f_time.iter_mut() {
            *t += delta;
        }
    }

    // ------------------------------------------------------------------ input

    fn ingest(&mut self, input: &[f32]) {
        let sample_rate = self.sample_rate as f64;
        let decay = (-1.0 / (0.020 * sample_rate)).exp() as f32;
        let attack = (1.0 - (-1.0 / (0.002 * sample_rate)).exp()) as f32;
        let release = (1.0 - (-1.0 / (0.050 * sample_rate)).exp()) as f32;
        let hold_samples = (0.120 * sample_rate) as i32;
        for &sample in input {
            let mut raw = sample;
            // A NaN or Inf from a misbehaving driver would otherwise poison the filters for good.
            if !(raw > -8f32 && raw < 8f32) {
                raw = 0f32;
            }
            // DC blocker
            let x = raw - self.dc_x + 0.998f32 * self.dc_y;
            self.dc_x = raw;
            self.dc_y = x;

            let ax = x.abs();
            self.gate_env = if ax > self.gate_env { ax } else { self.gate_env * decay };
            let threshold = if self.gate_open { self.gate_lin * 0.7f32 } else { self.gate_lin };
            if self.gate_env > threshold {
                self.gate_hold = hold_samples;
            } else if self.gate_hold > 0 {
                self.gate_hold -= 1;
            }
            self.gate_open = self.gate_hold > 0;
            let open = if self.gate_open { 1f32 } else { 0f32 };
            self.gate_gain += (open - self.gate_gain) * (if self.gate_open { attack } else { release });

            let gated = x * self.gate_gain;
            let idx = (self.w_in as i32 & MASK) as usize;
            self.in_buf[idx] = gated;
            self.ana_buf[idx] = self.band.process(gated);
            self.w_in += 1;
            self.since_detect += 1;
            if self.since_detect >= Self::HOP {
                self.since_detect = 0;
                self.detect_frame();
            }
        }
    }

    fn detect_frame(&mut self) {
        let window = self.yin.window_length;
        if self.w_in < window as i64 {
            self.push_frame(false, 0.0, 0.0);
            return;
        }
        self.yin.detect(&self.ana_buf, MASK, (self.w_in - window as i64) as i32, self.yin_threshold);
        // Hysteresis: a clear onset needs the strict threshold, but once voiced, a frame stays voiced while
        // it is still fairly periodic and its pitch is continuous. Keeps vowels from flickering.
        let ap = self.yin.aperiodicity;
        let clear = ap < self.yin_threshold;
        let continuing = self.was_periodic
            && self.last_f0 > 0.0
            && ap < self.yin_threshold * 1.5
            && (self.yin.f0 / self.last_f0).ln().abs() < 0.12;
        let periodic = clear || continuing;
        self.was_periodic = periodic && self.gate_open;
        let mut voiced = periodic && self.gate_open;
        let mut f0 = self.yin.f0;
        if voiced {
            self.hang = 3;
            self.last_f0 = f0;
        } else if self.hang > 0 && self.last_f0 > 0.0 {
            self.hang -= 1;
            voiced = true;
            f0 = self.last_f0;
        }
        if !voiced {
            self.tuner.update(0.0, false);
            self.med_n = 0;
            self.push_frame(false, 0.0, 0.0);
            return;
        }

        // Median of three keeps single-frame octave glitches out of the tuner.
        if self.med_n < 3 {
            self.med[self.med_n] = f0;
            self.med_n += 1;
        } else {
            self.med[0] = self.med[1];
            self.med[1] = self.med[2];
            self.med[2] = f0;
        }
        let f_use = if self.med_n < 3 { f0 } else { median3(self.med[0], self.med[1], self.med[2]) };

        let midi_in = pitch::hz_to_midi(f_use, self.a4);
        if self.auto_register {
            self.reg_n += 1;
            let n = self.reg_n as f64;
            self.reg_avg += (1.0 / n).max(1.0 / 800.0) * (midi_in - self.reg_avg);
            let target = (self.register_midi - self.reg_avg).clamp(-14.0, 14.0);
            self.auto_shift += 0.003f64.max(1.0 / n) * (target - self.auto_shift);
        } else {
            self.auto_shift += 0.02 * (0.0 - self.auto_shift);
        }

        self.update_formant();
        let mut midi_out = self.tuner.update(midi_in + self.shift + self.auto_shift, true);
        if self.vibrato_depth > 0.0 {
            self.vib_phase += 2.0 * PI * self.vibrato_rate * Self::HOP as f64 / self.sample_rate as f64;
            if self.vib_phase > 2.0 * PI {
                self.vib_phase -= 2.0 * PI;
            }
            midi_out += self.vibrato_depth * self.vib_phase.sin();
        }
        let out_hz = pitch::midi_to_hz(midi_out, self.a4).clamp(40.0, 1600.0);
        self.push_frame(true, f_use, out_hz);
        self.input_hz = f_use;
        self.output_hz = out_hz;
        self.target_midi = self.tuner.target();
    }

    fn update_formant(&mut self) {
        let semis = (self.formant_semis + self.formant_follow * (self.shift + self.auto_shift)).clamp(-12.0, 12.0);
        self.formant_ratio = 2f64.powf(semis / 12.0);
    }

    fn push_frame(&mut self, voiced: bool, in_hz: f64, out_hz: f64) {
        let i = (self.f_count % FRAMES) as usize;
        self.f_time[i] = self.w_in - (self.yin.window_length / 2) as i64;
        self.f_voiced[i] = voiced;
        self.f_in_hz[i] = in_hz;
        self.f_out_hz[i] = out_hz;
        self.f_count += 1;
        if !voiced {
            self.input_hz = 0.0;
            self.output_hz = 0.0;
        }
    }

    // ------------------------------------------------------------------ frames and marks

    /// Index (in the frame ring) of the latest frame at or before `t`; the oldest if none. -1 if empty.
    fn frame_at(&self, t: i64) -> i32 {
        if self.f_count == 0 {
            return -1;
        }
        let oldest = 0i64.max(self.f_count - FRAMES + 1);
        let mut k = self.f_count - 1;
        while k > oldest && self.f_time[(k % FRAMES) as usize] > t {
            k -= 1;
        }
        (k % FRAMES) as i32
    }

    fn mark_at(&self, k: i64) -> i64 {
        self.m_pos[(k % MARKS) as usize]
    }

    fn push_mark(&mut self, pos: i64) {
        self.m_pos[(self.m_count % MARKS) as usize] = pos;
        self.m_count += 1;
        self.last_mark_pos = pos;
    }

    fn advance_marks(&mut self) {
        if self.f_count == 0 {
            return;
        }
        let sr = self.sample_rate as f64;
        let newest = ((self.f_count - 1) % FRAMES) as usize;

        if !self.chain {
            if !self.f_voiced[newest] {
                return;
            }
            let mut k = self.f_count - 1;
            let oldest = 0i64.max(self.f_count - FRAMES + 1);
            while k > oldest && self.f_voiced[((k - 1) % FRAMES) as usize] {
                k -= 1;
            }
            let start_time = self.f_time[(k % FRAMES) as usize];
            let period = sr / self.f_in_hz[newest];
            let from = start_time.max(self.last_mark_pos + 1);
            let len = period as i32;
            if from + len as i64 + 2 > self.w_in || from < self.w_in - (RING / 2) as i64 {
                return;
            }
            let m = Marks::peak(&self.ana_buf, MASK, from, len);
            self.push_mark(m);
            self.chain = true;
            self.chain_period = period;
        }

        while self.chain {
            let last = self.mark_at(self.m_count - 1);
            let fi = self.frame_at(last);
            let mut period =
                if fi >= 0 && self.f_voiced[fi as usize] { sr / self.f_in_hz[fi as usize] } else { self.chain_period };
            period = period.clamp(sr / self.fmax, sr / self.fmin);
            self.chain_period = period;
            let reach = (period * 1.7) as i64 + 4;
            if last + reach >= self.w_in {
                break;
            }
            let ahead = self.frame_at(last + period as i64);
            if ahead >= 0 && !self.f_voiced[ahead as usize] {
                self.chain = false;
                break;
            }
            let m = Marks::next(&self.ana_buf, MASK, last, period);
            self.push_mark(m);
        }
    }

    /// Index of the mark nearest `t`, or -1 if none lies within three quarters of a period.
    fn nearest_mark(&self, t: f64) -> i64 {
        if self.m_count == 0 {
            return -1;
        }
        let oldest = 0i64.max(self.m_count - MARKS + 1);
        let mut k = self.m_count - 1;
        while k > oldest && self.mark_at(k) as f64 > t {
            k -= 1;
        }
        let mut best = k;
        if k + 1 < self.m_count && (self.mark_at(k + 1) as f64 - t).abs() < (self.mark_at(k) as f64 - t).abs() {
            best = k + 1;
        }
        let period = self.period_at(best);
        if (self.mark_at(best) as f64 - t).abs() <= 0.75 * period { best } else { -1 }
    }

    fn period_at(&self, k: i64) -> f64 {
        let oldest = 0i64.max(self.m_count - MARKS + 1);
        let before = if k > oldest { (self.mark_at(k) - self.mark_at(k - 1)) as f64 } else { -1.0 };
        let after = if k + 1 < self.m_count { (self.mark_at(k + 1) - self.mark_at(k)) as f64 } else { -1.0 };
        let sr = self.sample_rate as f64;
        let lo = sr / self.fmax * 0.8;
        let hi = sr / self.fmin * 1.25;
        let p = if before > 0.0 && after > 0.0 {
            if before.max(after) < 1.6 * before.min(after) { 0.5 * (before + after) } else { before.min(after) }
        } else if before > 0.0 {
            before
        } else if after > 0.0 {
            after
        } else {
            self.chain_period
        };
        p.clamp(lo, hi)
    }

    // ------------------------------------------------------------------ synthesis

    fn synthesize(&mut self, n: i32) {
        let horizon = self.w_out + n as i64;
        let unvoiced_half = self.sample_rate as f64 * 0.005;
        let rho = self.formant_ratio;
        loop {
            let a = self.s_next - self.d_map as f64;
            let fi = self.frame_at(a as i64);
            let mut voiced = false;
            let mut tin = 0.0;
            let mut center = a;
            let mut ts = 0.0;
            if fi >= 0 && self.f_voiced[fi as usize] {
                let mi = self.nearest_mark(a);
                if mi >= 0 {
                    voiced = true;
                    tin = self.period_at(mi);
                    center = self.mark_at(mi) as f64;
                    ts = self.sample_rate as f64 / self.f_out_hz[fi as usize];
                }
            }
            let half: f64;
            let gain: f32;
            if voiced {
                half = tin.max(0.6 * ts * rho).min(2.0 * tin);
                gain = 1f32;
            } else {
                half = unvoiced_half;
                ts = half / rho;
                gain = self.breath;
                center = a;
            }
            if self.s_next - half / rho >= horizon as f64 {
                break;
            }
            if center + half + 2.0 > self.w_in as f64 {
                self.starved_grains += 1;
                center = self.w_in as f64 - half - 2.0;
            }
            Grain::add(&self.in_buf, MASK, center, half, &mut self.acc, &mut self.norm, AMASK, self.s_next, rho, gain);
            self.s_next += ts;
        }
    }

    fn emit(&mut self, output: &mut [f32]) {
        for (i, out) in output.iter_mut().enumerate() {
            let pos = self.w_out + i as i64;
            let idx = (pos as i32 & AMASK) as usize;
            let nr = self.norm[idx];
            let mut y = self.acc[idx] / nr.max(Grain::NORM_FLOOR);
            self.acc[idx] = 0.0;
            self.norm[idx] = 0.0;
            if self.dry > 0f32 {
                y = y * (1f32 - self.dry) + self.dry * self.in_buf[((pos - self.d_map as i64) as i32 & MASK) as usize];
            }
            *out = y;
        }
        self.w_out += output.len() as i64;
    }
}

fn median3(a: f64, b: f64, c: f64) -> f64 {
    a.min(b).max(a.max(b).min(c))
}

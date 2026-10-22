use std::f64::consts::PI;

use crate::kmath::{powf, round_f32, round_f64, tanhf};
use crate::model::{VoiceParams, P};

/// One-pole parameter smoother to avoid zipper noise when a slider moves.
struct Smooth {
    v: f32,
    coef: f32,
}

impl Smooth {
    fn new(v: f32, sample_rate: i32) -> Self {
        Smooth::with_seconds(v, sample_rate, 0.015)
    }

    fn with_seconds(v: f32, sample_rate: i32, seconds: f32) -> Self {
        Smooth { v, coef: (1.0 - (-1.0 / (seconds * sample_rate as f32) as f64).exp()) as f32 }
    }

    fn next(&mut self, target: f32) -> f32 {
        self.v += (target - self.v) * self.coef;
        self.v
    }
}

/// RBJ cookbook biquad. Coefficients in double; state in double so low cutoffs stay stable.
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
    bypass: bool,
}

impl Biquad {
    fn new() -> Self {
        Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0, bypass: true }
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    fn set(&mut self, nb0: f64, nb1: f64, nb2: f64, na0: f64, na1: f64, na2: f64) {
        self.b0 = nb0 / na0;
        self.b1 = nb1 / na0;
        self.b2 = nb2 / na0;
        self.a1 = na1 / na0;
        self.a2 = na2 / na0;
        self.bypass = false;
    }

    fn off(&mut self) {
        self.bypass = true;
    }

    fn highpass(&mut self, f: f64, q: f64, sr: i32) {
        let w = 2.0 * PI * f.min(sr as f64 * 0.45) / sr as f64;
        let c = w.cos();
        let alpha = w.sin() / (2.0 * q);
        self.set((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    fn lowpass(&mut self, f: f64, q: f64, sr: i32) {
        let w = 2.0 * PI * f.min(sr as f64 * 0.45) / sr as f64;
        let c = w.cos();
        let alpha = w.sin() / (2.0 * q);
        self.set((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    fn peaking(&mut self, f: f64, q: f64, gain_db: f64, sr: i32) {
        if gain_db.abs() < 0.05 {
            return self.off();
        }
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f.min(sr as f64 * 0.45) / sr as f64;
        let c = w.cos();
        let alpha = w.sin() / (2.0 * q);
        self.set(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a);
    }

    fn low_shelf(&mut self, f: f64, gain_db: f64, sr: i32) {
        if gain_db.abs() < 0.05 {
            return self.off();
        }
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f.min(sr as f64 * 0.45) / sr as f64;
        let c = w.cos();
        let s = w.sin();
        let beta = 2.0 * a.sqrt() * (s / 2.0 * 2f64.sqrt());
        self.set(
            a * ((a + 1.0) - (a - 1.0) * c + beta),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - beta),
            (a + 1.0) + (a - 1.0) * c + beta,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - beta,
        );
    }

    fn high_shelf(&mut self, f: f64, gain_db: f64, sr: i32) {
        if gain_db.abs() < 0.05 {
            return self.off();
        }
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f.min(sr as f64 * 0.45) / sr as f64;
        let c = w.cos();
        let s = w.sin();
        let beta = 2.0 * a.sqrt() * (s / 2.0 * 2f64.sqrt());
        self.set(
            a * ((a + 1.0) + (a - 1.0) * c + beta),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - beta),
            (a + 1.0) - (a - 1.0) * c + beta,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - beta,
        );
    }

    fn process(&mut self, x: f32) -> f32 {
        if self.bypass {
            return x;
        }
        let x = x as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }
}

/// Fractional delay line with linear interpolation.
struct DelayLine {
    size: i32,
    buf: Vec<f32>,
    w: i32,
}

impl DelayLine {
    fn new(max_samples: i32) -> Self {
        let size = max_samples + 4;
        DelayLine { size, buf: vec![0f32; size as usize], w: 0 }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
    }

    fn write(&mut self, x: f32) {
        self.buf[self.w as usize] = x;
        self.w = if self.w + 1 == self.size { 0 } else { self.w + 1 };
    }

    /// Sample written `d` samples ago (d >= 1 counts the most recent write as 1).
    fn read(&self, d: f64) -> f32 {
        let pos = self.w as f64 - d;
        let i = pos.floor() as i32;
        let f = (pos - i as f64) as f32;
        let a = self.buf[i.rem_euclid(self.size) as usize];
        let b = self.buf[(i + 1).rem_euclid(self.size) as usize];
        a + (b - a) * f
    }
}

/// One step of the 32-bit xorshift the effects use for noise.
#[inline]
fn xorshift(rng: &mut i32) -> i32 {
    let mut r = *rng;
    r ^= r.wrapping_shl(13);
    r ^= ((r as u32) >> 17) as i32;
    r ^= r.wrapping_shl(5);
    *rng = r;
    r
}

/// Uniform in [-1, 1).
#[inline]
fn rand_bipolar(rng: &mut i32) -> f32 {
    ((xorshift(rng) as u32) >> 8) as f32 / 8388608f32 - 1f32
}

/// Uniform in [0, 1).
#[inline]
fn rand_unit(rng: &mut i32) -> f32 {
    ((xorshift(rng) as u32) >> 8) as f32 / 16777216f32
}

/// Vocal roughness: the level flutters irregularly at 50 to 170 Hz (the cycle-to-cycle unsteadiness of a strained or
/// gravelly voice), with a little breath noise riding on the voice. Silence stays silent.
struct Rasp {
    sr: i32,
    target: f32,
    modulation: f32,
    hold_left: i32,
    env: f32,
    hiss_lp: f32,
    rng: i32,
    mod_coef: f32,
    env_coef: f32,
    hiss_coef: f32,
}

impl Rasp {
    fn new(sr: i32) -> Self {
        Rasp {
            sr,
            target: 0.0,
            modulation: 0.0,
            hold_left: 0,
            env: 0.0,
            hiss_lp: 0.0,
            rng: 0x6D2B79F5,
            mod_coef: (1.0 - (-2.0 * PI * 300.0 / sr as f64).exp()) as f32,
            env_coef: (1.0 - (-1.0 / (0.01 * sr as f64)).exp()) as f32,
            hiss_coef: (1.0 - (-2.0 * PI * 1500.0 / sr as f64).exp()) as f32,
        }
    }

    fn process(&mut self, x: f32, amount: f32) -> f32 {
        self.hold_left -= 1;
        if self.hold_left <= 0 {
            self.target = rand_bipolar(&mut self.rng);
            let r = rand_bipolar(&mut self.rng);
            self.hold_left = (((0.006f32 + 0.014f32 * (r * 0.5f32 + 0.5f32)) * self.sr as f32) as i32).max(1);
        }
        self.modulation += (self.target - self.modulation) * self.mod_coef;
        self.env += (x.abs() - self.env) * self.env_coef;
        // High-passed noise for the breathy part, scaled by how loud the voice is right now.
        let n = rand_bipolar(&mut self.rng);
        self.hiss_lp += (n - self.hiss_lp) * self.hiss_coef;
        let breath = (n - self.hiss_lp) * self.env * 0.5f32;
        x * (1f32 + 0.6f32 * amount * self.modulation) + breath * amount
    }
}

/// Short feedback comb with a slow LFO on the delay: the "metallic" and flanged character.
struct ModComb {
    sr: i32,
    line: DelayLine,
    lfo: f64,
    mix_s: Smooth,
}

impl ModComb {
    fn new(sr: i32) -> Self {
        ModComb { sr, line: DelayLine::new((sr as f64 * 0.04) as i32), lfo: 0.0, mix_s: Smooth::new(0.0, sr) }
    }

    fn clear(&mut self) {
        self.line.clear();
    }

    fn process(&mut self, x: f32, mix: f32, ms: f32, fb: f32, depth: f32, rate_hz: f32) -> f32 {
        self.lfo += 2.0 * PI * rate_hz as f64 / self.sr as f64;
        if self.lfo > 2.0 * PI {
            self.lfo -= 2.0 * PI;
        }
        let base = ms as f64 * 0.001 * self.sr as f64;
        let d = 2f64.max(base * (1.0 + 0.5 * depth as f64 * self.lfo.sin()));
        let v = x + fb * self.line.read(d);
        self.line.write(v);
        let m = self.mix_s.next(mix);
        x * (1f32 - m) + v * 0.7f32 * m
    }
}

struct BitCrusher {
    held: f32,
    count: i32,
    mix_s: Smooth,
}

impl BitCrusher {
    fn new(sr: i32) -> Self {
        BitCrusher { held: 0.0, count: 0, mix_s: Smooth::new(0.0, sr) }
    }

    fn process(&mut self, x: f32, mix: f32, bits: i32, hold: i32) -> f32 {
        if self.count <= 0 {
            self.held = x;
            self.count = hold;
        }
        self.count -= 1;
        let q = (1i32 << (bits - 1)) as f32;
        let crushed = round_f32(self.held * q) as f32 / q;
        let m = self.mix_s.next(mix);
        x * (1f32 - m) + crushed * m
    }
}

/// Soft-clipping overdrive. Scaled so a typical speech peak keeps roughly its level as the drive goes up.
struct Drive {
    gain: f32,
    make_up: f32,
}

impl Drive {
    fn new() -> Self {
        Drive { gain: 1.0, make_up: 1.0 }
    }

    fn set(&mut self, amount: f32) {
        self.gain = 1f32 + 25f32 * amount;
        self.make_up = 0.3f32 / tanhf(0.3f32 * self.gain);
    }

    fn process(&self, x: f32) -> f32 {
        tanhf(self.gain * x) * self.make_up
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Stutter,
    Reverse,
    Tape,
    Pitch,
    Drop,
    Static,
}

impl Kind {
    const ENTRIES: [Kind; 6] = [Kind::Stutter, Kind::Reverse, Kind::Tape, Kind::Pitch, Kind::Drop, Kind::Static];
}

/// Random malfunctions: stutter, reverse, tape stop, pitch jumps, dropouts and static. At most one runs at a time,
/// and a new one only starts while there is voice, so silence stays silent.
struct Glitches {
    sr: i32,
    /// Events per second for each [`Kind`], set by the chain.
    rates: [f32; 6],
    stutter_len: i32,
    jump_ratio: f64,

    ring: Vec<f32>,
    looped: Vec<f32>,
    w: i32,
    kind: Option<Kind>,
    len: i32,
    pos: i32,
    fade_len: i32,
    loop_len: i32,
    delay: f64,
    level: f32,
    rng: i32,
    fade: i32,
}

impl Glitches {
    fn new(sr: i32) -> Self {
        Glitches {
            sr,
            rates: [0.0; 6],
            stutter_len: 3000,
            jump_ratio: 2.0,
            ring: vec![0f32; sr as usize],
            looped: vec![0f32; (sr / 2) as usize],
            w: 0,
            kind: None,
            len: 0,
            pos: 0,
            fade_len: 1,
            loop_len: 0,
            delay: 0.0,
            level: 0.0,
            rng: 0x2545F491,
            fade: 8.max(sr / 1000),
        }
    }

    fn clear(&mut self) {
        self.ring.fill(0.0);
        self.kind = None;
    }

    fn rand01(&mut self) -> f32 {
        rand_unit(&mut self.rng)
    }

    fn ms(&mut self, lo: i32, hi: i32) -> i32 {
        let r = self.rand01();
        round_f32((lo as f32 + r * (hi - lo) as f32) * 0.001f32 * self.sr as f32)
    }

    fn ring_size(&self) -> i32 {
        self.ring.len() as i32
    }

    /// Sample written `d` samples ago, where 1 is the most recent.
    fn ring_at(&self, d: f64) -> f32 {
        let p = self.w as f64 - d;
        let i = p.floor() as i32;
        let f = (p - i as f64) as f32;
        let size = self.ring_size();
        let a = self.ring[i.rem_euclid(size) as usize];
        let b = self.ring[(i + 1).rem_euclid(size) as usize];
        a + (b - a) * f
    }

    fn capture(&mut self, n: i32) {
        self.loop_len = n.clamp(self.fade * 2, self.looped.len() as i32);
        let size = self.ring_size();
        for i in 0..self.loop_len {
            self.looped[i as usize] = self.ring[(self.w - self.loop_len + i).rem_euclid(size) as usize];
        }
    }

    fn start(&mut self, k: Kind) {
        self.fade_len = self.fade;
        match k {
            Kind::Stutter => {
                self.capture(self.stutter_len);
                self.len = self.loop_len * (2 + (self.rand01() * 3f32) as i32);
            }
            Kind::Reverse => {
                let n = self.ms(150, 400);
                self.capture(n);
                self.len = self.loop_len;
                self.fade_len = self.fade * 4;
            }
            Kind::Tape => {
                self.len = self.ms(250, 600);
                self.delay = 1.0;
                self.fade_len = self.fade * 10;
            }
            Kind::Pitch => {
                let r = self.jump_ratio;
                if (r - 1.0).abs() < 0.01 {
                    return;
                }
                // Read from far enough back that a speed-up arrives at the present just as the jump ends.
                let m = self.ms(80, 250);
                self.len = m.min(((self.ring_size() - 8) as f64 / (r - 1.0).abs()) as i32);
                self.delay = if r > 1.0 { 1.0 + self.len as f64 * (r - 1.0) } else { 1.0 };
                self.fade_len = self.fade * 3;
            }
            Kind::Drop => {
                self.len = self.ms(25, 150);
                self.fade_len = self.fade * 3;
            }
            Kind::Static => {
                self.len = self.ms(60, 300);
                self.fade_len = self.fade * 5;
            }
        }
        self.pos = 0;
        self.kind = Some(k);
    }

    fn process(&mut self, x: f32) -> f32 {
        self.ring[self.w as usize] = x;
        self.w = if self.w + 1 == self.ring_size() { 0 } else { self.w + 1 };
        self.level += (x.abs() - self.level) * 0.001f32;
        let k = match self.kind {
            None => {
                if self.level > 0.004f32 {
                    let mut total = 0f32;
                    for r in self.rates {
                        total += r;
                    }
                    if total > 0f32 && self.rand01() < total / self.sr as f32 {
                        // Pick which kind in proportion to its rate.
                        let mut pick = self.rand01() * total;
                        for kk in Kind::ENTRIES {
                            pick -= self.rates[kk as usize];
                            if pick <= 0f32 && self.rates[kk as usize] > 0f32 {
                                self.start(kk);
                                break;
                            }
                        }
                    }
                }
                return x;
            }
            Some(k) => k,
        };
        let y = match k {
            Kind::Stutter => self.looped[(self.pos % self.loop_len) as usize],
            Kind::Reverse => self.looped[(self.loop_len - 1 - self.pos) as usize],
            Kind::Tape => {
                // Slows from full speed to a stop, getting quieter and duller as it goes.
                let speed = 1.0 - self.pos as f64 / self.len as f64;
                self.delay = (self.delay + 1.0 - speed).min(self.ring_size() as f64 - 4.0);
                self.ring_at(self.delay) * 1f32.min(speed as f32 * 3f32)
            }
            Kind::Pitch => {
                self.delay = (self.delay + 1.0 - self.jump_ratio).clamp(1.0, self.ring_size() as f64 - 4.0);
                self.ring_at(self.delay)
            }
            Kind::Drop => 0f32,
            Kind::Static => {
                let n = self.rand01() * 2f32 - 1f32;
                let crackle = if self.rand01() < 0.003f32 { (self.rand01() * 2f32 - 1f32) * 0.8f32 } else { 0f32 };
                x * 0.5f32 + n * 0.25f32.min(self.level * 6f32 + 0.05f32) + crackle
            }
        };
        // Crossfade from the dry signal into the event and back out again.
        let edge = self.pos.min(self.len - 1 - self.pos);
        let g = if edge < self.fade_len { edge as f32 / self.fade_len as f32 } else { 1f32 };
        self.pos += 1;
        if self.pos >= self.len {
            self.kind = None;
        }
        x + (y - x) * g
    }
}

/// A mains-style hum that swells with the voice, like a grounding fault in a speaker.
struct Buzz {
    sr: i32,
    phase: f64,
    env: f32,
    lp: f32,
    attack: f32,
    release: f32,
}

impl Buzz {
    fn new(sr: i32) -> Self {
        Buzz {
            sr,
            phase: 0.0,
            env: 0.0,
            lp: 0.0,
            attack: (1.0 - (-1.0 / (0.005 * sr as f64)).exp()) as f32,
            release: (1.0 - (-1.0 / (0.08 * sr as f64)).exp()) as f32,
        }
    }

    fn process(&mut self, x: f32, amount: f32, hz: f32) -> f32 {
        let a = x.abs();
        self.env += (a - self.env) * (if a > self.env { self.attack } else { self.release });
        self.phase += (hz / self.sr as f32) as f64;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        // A softened sawtooth: rich in harmonics like real hum, without the harshest aliasing.
        self.lp += ((2.0 * self.phase - 1.0) as f32 - self.lp) * 0.2f32;
        x + self.lp * 1f32.min(self.env * 5f32) * amount * 0.35f32
    }
}

/// Three slowly drifting delayed copies mixed under the voice: a small crowd.
struct Chorus {
    sr: i32,
    line: DelayLine,
    phase: [f64; 3],
    mix_s: Smooth,
}

const CHORUS_RATE: [f64; 3] = [0.31, 0.43, 0.57];
const CHORUS_BASE: [f64; 3] = [0.012, 0.019, 0.027];

impl Chorus {
    fn new(sr: i32) -> Self {
        Chorus { sr, line: DelayLine::new((sr as f64 * 0.05) as i32), phase: [0.0, 2.1, 4.2], mix_s: Smooth::new(0.0, sr) }
    }

    fn clear(&mut self) {
        self.line.clear();
    }

    fn process(&mut self, x: f32, mix: f32) -> f32 {
        self.line.write(x);
        let mut wet = 0f32;
        for v in 0..3 {
            self.phase[v] += 2.0 * PI * CHORUS_RATE[v] / self.sr as f64;
            if self.phase[v] > 2.0 * PI {
                self.phase[v] -= 2.0 * PI;
            }
            wet += self.line.read((CHORUS_BASE[v] + 0.003 * self.phase[v].sin()) * self.sr as f64);
        }
        let m = self.mix_s.next(mix);
        (x + wet * 0.67f32 * m) / (1f32 + 0.6f32 * m)
    }
}

/// Six swept all-pass stages mixed with the dry voice, carving moving notches: the classic jet whoosh.
struct Phaser {
    sr: i32,
    z: [f32; 6],
    lfo: f64,
    feedback: f32,
    mix_s: Smooth,
}

impl Phaser {
    fn new(sr: i32) -> Self {
        Phaser { sr, z: [0.0; 6], lfo: 0.0, feedback: 0.0, mix_s: Smooth::new(0.0, sr) }
    }

    fn clear(&mut self) {
        self.z.fill(0.0);
        self.feedback = 0.0;
    }

    fn process(&mut self, x: f32, mix: f32, rate_hz: f32) -> f32 {
        self.lfo += 2.0 * PI * rate_hz as f64 / self.sr as f64;
        if self.lfo > 2.0 * PI {
            self.lfo -= 2.0 * PI;
        }
        let f = 300.0 * 2f64.powf(3.0 * (0.5 + 0.5 * self.lfo.sin())); // 300 Hz to 2.4 kHz
        let t = (PI * f / self.sr as f64).tan();
        let a = ((t - 1.0) / (t + 1.0)) as f32;
        let mut y = x + self.feedback * 0.5f32;
        for z in self.z.iter_mut() {
            let o = a * y + *z;
            *z = y - a * o;
            y = o;
        }
        self.feedback = y;
        let m = self.mix_s.next(mix);
        x * (1f32 - 0.5f32 * m) + y * 0.5f32 * m
    }
}

/// Tape wow and flutter: a slowly wandering delay, which bends the pitch a little up and down.
struct Warble {
    sr: i32,
    line: DelayLine,
    wow: f64,
    flutter: f64,
    drift: f32,
    drift_target: f32,
    drift_left: i32,
    rng: i32,
    amount_s: Smooth,
}

impl Warble {
    fn new(sr: i32) -> Self {
        Warble {
            sr,
            line: DelayLine::new((sr as f64 * 0.03) as i32),
            wow: 0.0,
            flutter: 0.0,
            drift: 0.0,
            drift_target: 0.0,
            drift_left: 0,
            rng: 0x1B873593,
            amount_s: Smooth::with_seconds(0.0, sr, 0.05),
        }
    }

    fn clear(&mut self) {
        self.line.clear();
    }

    fn process(&mut self, x: f32, amount: f32) -> f32 {
        let sr = self.sr;
        self.line.write(x);
        self.wow += 2.0 * PI * 0.55 / sr as f64;
        if self.wow > 2.0 * PI {
            self.wow -= 2.0 * PI;
        }
        self.flutter += 2.0 * PI * 7.5 / sr as f64;
        if self.flutter > 2.0 * PI {
            self.flutter -= 2.0 * PI;
        }
        self.drift_left -= 1;
        if self.drift_left <= 0 {
            self.drift_target = rand_bipolar(&mut self.rng);
            let r = rand_bipolar(&mut self.rng);
            self.drift_left = ((0.3f32 + 0.35f32 * (r + 1f32)) * sr as f32) as i32;
        }
        self.drift += (self.drift_target - self.drift) / (0.3f32 * sr as f32);
        let a = self.amount_s.next(amount);
        let d = 0.008 * sr as f64
            + (a * sr as f32) as f64
                * (0.0035 * self.wow.sin() + 0.0015 * self.drift as f64 + 0.00008 * self.flutter.sin());
        self.line.read(d)
    }
}

/// Record surface noise: random clicks and the odd pop over a faint hiss. Unlike the glitches it never stops.
struct Crackle {
    sr: i32,
    click: f32,
    hiss_lp: f32,
    rng: i32,
    decay: f32,
}

impl Crackle {
    fn new(sr: i32) -> Self {
        Crackle { sr, click: 0.0, hiss_lp: 0.0, rng: 0x27D4EB2F, decay: (-1.0 / (0.0003 * sr as f64)).exp() as f32 }
    }

    fn rand01(&mut self) -> f32 {
        rand_unit(&mut self.rng)
    }

    fn process(&mut self, x: f32, amount: f32) -> f32 {
        if self.rand01() < amount * 40f32 / self.sr as f32 {
            let size = self.rand01();
            let sign = if self.rand01() < 0.5f32 { -1f32 } else { 1f32 };
            self.click += sign * (0.03f32 + 0.3f32 * size * size * size) * amount;
        }
        self.click *= self.decay;
        let r = self.rand01();
        self.hiss_lp += ((r - 0.5f32) - self.hiss_lp) * 0.3f32;
        x + self.click + self.hiss_lp * 0.01f32 * amount
    }
}

struct Echo {
    sr: i32,
    line: DelayLine,
    damp: f32,
    mix_s: Smooth,
}

impl Echo {
    fn new(sr: i32) -> Self {
        Echo { sr, line: DelayLine::new(sr), damp: 0.0, mix_s: Smooth::new(0.0, sr) }
    }

    fn clear(&mut self) {
        self.line.clear();
        self.damp = 0.0;
    }

    fn process(&mut self, x: f32, mix: f32, ms: f32, fb: f32) -> f32 {
        let d = self.line.read(2f64.max(ms as f64 * 0.001 * self.sr as f64));
        self.damp += (d - self.damp) * 0.35f32;
        self.line.write(x + self.damp * fb);
        x + d * self.mix_s.next(mix)
    }
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Comb {
    fn process(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let out = self.buf[self.i];
        self.store = out * (1f32 - damp) + self.store * damp;
        self.buf[self.i] = x + self.store * fb;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        out
    }
}

struct AllPass {
    buf: Vec<f32>,
    i: usize,
}

impl AllPass {
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        let out = -x + b;
        self.buf[self.i] = x + b * 0.5f32;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        out
    }
}

/// Freeverb: eight damped combs into four allpasses. Mono.
struct Reverb {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
    mix_s: Smooth,
}

impl Reverb {
    fn new(sr: i32) -> Self {
        let scale = sr as f64 / 44100.0;
        let len = |n: i32| 8.max(round_f64(n as f64 * scale)) as usize;
        Reverb {
            combs: [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617]
                .iter()
                .map(|&n| Comb { buf: vec![0f32; len(n)], i: 0, store: 0.0 })
                .collect(),
            allpasses: [556, 441, 341, 225].iter().map(|&n| AllPass { buf: vec![0f32; len(n)], i: 0 }).collect(),
            mix_s: Smooth::new(0.0, sr),
        }
    }

    fn clear(&mut self) {
        for c in &mut self.combs {
            c.buf.fill(0.0);
            c.store = 0.0;
        }
        for a in &mut self.allpasses {
            a.buf.fill(0.0);
        }
    }

    fn process(&mut self, x: f32, mix: f32, size: f32, damping: f32) -> f32 {
        let fb = 0.7f32 + 0.28f32 * size;
        let damp = 0.1f32 + 0.6f32 * damping;
        let input = x * 0.015f32;
        let mut sum = 0f32;
        for c in &mut self.combs {
            sum += c.process(input, fb, damp);
        }
        for a in &mut self.allpasses {
            sum = a.process(sum);
        }
        x + sum * 3.0f32 * self.mix_s.next(mix)
    }
}

struct Limiter {
    gain: f32,
    release: f32,
    ceiling: f32,
    knee: f32,
}

impl Limiter {
    fn new(sr: i32) -> Self {
        Limiter { gain: 1.0, release: (1.0 - (-1.0 / (0.080 * sr as f64)).exp()) as f32, ceiling: 0.89, knee: 0.9 }
    }

    fn process(&mut self, x: f32) -> f32 {
        let a = x.abs();
        let target = if a > self.ceiling { self.ceiling / a } else { 1f32 };
        self.gain = if target < self.gain { target } else { self.gain + (1f32 - self.gain) * self.release };
        let y = x * self.gain;
        // Linear below the knee; only the last few percent are rounded off, so ordinary levels are untouched.
        let m = y.abs();
        if m <= self.knee {
            return y;
        }
        let soft = self.knee + (1f32 - self.knee) * tanhf((m - self.knee) / (1f32 - self.knee));
        if y < 0f32 { -soft } else { soft }
    }
}

/// The GLaDOS effect chain, applied to a mono stream: ring mod, rasp, distortion, metal comb, bit crush, glitches, buzz,
/// tape warble, crackle, EQ, chorus, phaser, tremolo, echo, reverb, output gain and a safety limiter. One instance per
/// stream. [`FxChain::configure`] and [`FxChain::process`] must be called from the same thread.
pub struct FxChain {
    sample_rate: i32,
    comb: ModComb,
    crusher: BitCrusher,
    drive: Drive,
    rasp: Rasp,
    glitches: Glitches,
    buzz: Buzz,
    warble: Warble,
    crackle: Crackle,
    chorus: Chorus,
    phaser: Phaser,
    tremolo_depth_s: Smooth,
    tremolo_phase: f64,
    echo: Echo,
    reverb: Reverb,
    limiter: Limiter,
    hpf: Biquad,
    low_shelf: Biquad,
    presence: Biquad,
    high_shelf: Biquad,
    lpf: Biquad,

    ring_mix_s: Smooth,
    out_s: Smooth,
    ring_phase: f64,

    ring_mix: f32,
    ring_hz: f64,
    comb_mix: f32,
    comb_ms: f32,
    comb_fb: f32,
    comb_depth: f32,
    comb_rate: f32,
    crush_mix: f32,
    crush_bits: i32,
    crush_hold: i32,
    rasp_amount: f32,
    buzz_amount: f32,
    warble_amount: f32,
    crackle_amount: f32,
    chorus_mix: f32,
    phaser_mix: f32,
    phaser_rate: f32,
    tremolo_depth: f32,
    tremolo_rate: f32,
    buzz_hz: f32,
    echo_mix: f32,
    echo_ms: f32,
    echo_fb: f32,
    reverb_mix: f32,
    reverb_size: f32,
    reverb_damp: f32,
    out_gain: f32,

    rasp_on: bool,
    drive_on: bool,
    comb_on: bool,
    crush_on: bool,
    glitch_on: bool,
    buzz_on: bool,
    warble_on: bool,
    crackle_on: bool,
    chorus_on: bool,
    phaser_on: bool,
    tremolo_on: bool,
    echo_on: bool,
    reverb_on: bool,
}

impl FxChain {
    pub fn new(sample_rate: i32) -> Self {
        FxChain {
            sample_rate,
            comb: ModComb::new(sample_rate),
            crusher: BitCrusher::new(sample_rate),
            drive: Drive::new(),
            rasp: Rasp::new(sample_rate),
            glitches: Glitches::new(sample_rate),
            buzz: Buzz::new(sample_rate),
            warble: Warble::new(sample_rate),
            crackle: Crackle::new(sample_rate),
            chorus: Chorus::new(sample_rate),
            phaser: Phaser::new(sample_rate),
            tremolo_depth_s: Smooth::new(0.0, sample_rate),
            tremolo_phase: 0.0,
            echo: Echo::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            limiter: Limiter::new(sample_rate),
            hpf: Biquad::new(),
            low_shelf: Biquad::new(),
            presence: Biquad::new(),
            high_shelf: Biquad::new(),
            lpf: Biquad::new(),
            ring_mix_s: Smooth::new(0.0, sample_rate),
            out_s: Smooth::new(1.0, sample_rate),
            ring_phase: 0.0,
            ring_mix: 0.0,
            ring_hz: 85.0,
            comb_mix: 0.0,
            comb_ms: 3.0,
            comb_fb: 0.5,
            comb_depth: 0.15,
            comb_rate: 0.35,
            crush_mix: 0.0,
            crush_bits: 10,
            crush_hold: 3,
            rasp_amount: 0.0,
            buzz_amount: 0.0,
            warble_amount: 0.0,
            crackle_amount: 0.0,
            chorus_mix: 0.0,
            phaser_mix: 0.0,
            phaser_rate: 0.4,
            tremolo_depth: 0.0,
            tremolo_rate: 6.0,
            buzz_hz: 100.0,
            echo_mix: 0.0,
            echo_ms: 220.0,
            echo_fb: 0.35,
            reverb_mix: 0.0,
            reverb_size: 0.55,
            reverb_damp: 0.5,
            out_gain: 1.0,
            rasp_on: false,
            drive_on: false,
            comb_on: false,
            crush_on: false,
            glitch_on: false,
            buzz_on: false,
            warble_on: false,
            crackle_on: false,
            chorus_on: false,
            phaser_on: false,
            tremolo_on: false,
            echo_on: false,
            reverb_on: false,
        }
    }

    pub fn configure(&mut self, p: &VoiceParams) {
        let sample_rate = self.sample_rate;
        self.ring_mix = p.get(P::RingMix) / 100f32;
        self.ring_hz = p.get(P::RingHz) as f64;
        self.comb_mix = p.get(P::CombMix) / 100f32;
        self.comb_ms = p.get(P::CombMs);
        self.comb_fb = p.get(P::CombFb) / 100f32;
        self.comb_depth = p.get(P::CombMod) / 100f32;
        self.comb_rate = p.get(P::CombRate);
        self.crush_mix = p.get(P::CrushMix) / 100f32;
        self.crush_bits = p.int(P::CrushBits);
        self.crush_hold = p.int(P::CrushHold);
        let rates = &mut self.glitches.rates;
        rates[Kind::Stutter as usize] = p.get(P::Glitch) / 100f32 * 2.5f32;
        rates[Kind::Reverse as usize] = p.get(P::GlitchReverse) / 100f32 * 1.2f32;
        rates[Kind::Tape as usize] = p.get(P::GlitchTape) / 100f32 * 0.8f32;
        rates[Kind::Pitch as usize] = p.get(P::GlitchPitch) / 100f32 * 2f32;
        rates[Kind::Drop as usize] = p.get(P::GlitchDrop) / 100f32 * 3f32;
        rates[Kind::Static as usize] = p.get(P::GlitchStatic) / 100f32 * 2f32;
        self.glitches.stutter_len = round_f32(p.get(P::GlitchMs) * 0.001f32 * sample_rate as f32);
        self.glitches.jump_ratio = 2f64.powf(p.get(P::GlitchJump) as f64 / 12.0);
        self.drive.set(p.get(P::Drive) / 100f32);
        self.rasp_amount = p.get(P::Rasp) / 100f32;
        self.buzz_amount = p.get(P::Buzz) / 100f32;
        self.warble_amount = p.get(P::Warble) / 100f32;
        self.crackle_amount = p.get(P::Crackle) / 100f32;
        self.chorus_mix = p.get(P::Chorus) / 100f32;
        self.phaser_mix = p.get(P::Phaser) / 100f32;
        self.phaser_rate = p.get(P::PhaserRate);
        self.tremolo_depth = p.get(P::Tremolo) / 100f32;
        self.tremolo_rate = p.get(P::TremoloRate);
        self.buzz_hz = p.get(P::BuzzHz);
        self.echo_mix = p.get(P::EchoMix) / 100f32;
        self.echo_ms = p.get(P::EchoMs);
        self.echo_fb = p.get(P::EchoFb) / 100f32;
        self.reverb_mix = p.get(P::ReverbMix) / 100f32;
        self.reverb_size = p.get(P::ReverbSize) / 100f32;
        self.reverb_damp = p.get(P::ReverbDamp) / 100f32;
        self.out_gain = powf(10f32, p.get(P::OutDb) / 20f32);

        self.rasp_on = self.rasp_amount > 0.001f32;
        self.drive_on = p.get(P::Drive) > 0.5f32;
        self.comb_on = self.comb_mix > 0.001f32;
        self.crush_on = self.crush_mix > 0.001f32;
        self.glitch_on = self.glitches.rates.iter().any(|r| *r > 0f32);
        self.buzz_on = self.buzz_amount > 0.001f32;
        self.warble_on = self.warble_amount > 0.001f32;
        self.crackle_on = self.crackle_amount > 0.001f32;
        self.chorus_on = self.chorus_mix > 0.001f32;
        self.phaser_on = self.phaser_mix > 0.001f32;
        self.tremolo_on = self.tremolo_depth > 0.001f32;
        if !self.warble_on {
            self.warble.clear();
        }
        if !self.chorus_on {
            self.chorus.clear();
        }
        if !self.phaser_on {
            self.phaser.clear();
        }
        self.echo_on = self.echo_mix > 0.001f32;
        self.reverb_on = self.reverb_mix > 0.001f32;
        if !self.comb_on {
            self.comb.clear();
        }
        if !self.glitch_on {
            self.glitches.clear();
        }
        if !self.echo_on {
            self.echo.clear();
        }
        if !self.reverb_on {
            self.reverb.clear();
        }

        self.hpf.highpass(p.get(P::Hpf) as f64, 0.707, sample_rate);
        self.low_shelf.low_shelf(200.0, p.get(P::LowDb) as f64, sample_rate);
        self.presence.peaking(p.get(P::MidHz) as f64, 1.0, p.get(P::MidDb) as f64, sample_rate);
        self.high_shelf.high_shelf(6000.0, p.get(P::HighDb) as f64, sample_rate);
        self.lpf.lowpass(p.get(P::Lpf) as f64, 0.707, sample_rate);
    }

    pub fn reset(&mut self) {
        self.comb.clear();
        self.glitches.clear();
        self.echo.clear();
        self.reverb.clear();
        self.warble.clear();
        self.chorus.clear();
        self.phaser.clear();
        self.hpf.reset();
        self.low_shelf.reset();
        self.presence.reset();
        self.high_shelf.reset();
        self.lpf.reset();
    }

    /// Processes `buf` in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        let sample_rate = self.sample_rate;
        let ring_step = 2.0 * PI * self.ring_hz / sample_rate as f64;
        for sample in buf.iter_mut() {
            let mut x = *sample;

            let rm = self.ring_mix_s.next(self.ring_mix);
            if rm > 0.001f32 {
                self.ring_phase += ring_step;
                if self.ring_phase > 2.0 * PI {
                    self.ring_phase -= 2.0 * PI;
                }
                x = x * (1f32 - rm) + x * self.ring_phase.sin() as f32 * rm;
            }
            if self.rasp_on {
                x = self.rasp.process(x, self.rasp_amount);
            }
            if self.drive_on {
                x = self.drive.process(x);
            }
            if self.comb_on {
                x = self.comb.process(x, self.comb_mix, self.comb_ms, self.comb_fb, self.comb_depth, self.comb_rate);
            }
            if self.crush_on {
                x = self.crusher.process(x, self.crush_mix, self.crush_bits, self.crush_hold);
            }
            if self.glitch_on {
                x = self.glitches.process(x);
            }
            if self.buzz_on {
                x = self.buzz.process(x, self.buzz_amount, self.buzz_hz);
            }
            if self.warble_on {
                x = self.warble.process(x, self.warble_amount);
            }
            if self.crackle_on {
                x = self.crackle.process(x, self.crackle_amount);
            }

            x = self.hpf.process(x);
            x = self.low_shelf.process(x);
            x = self.presence.process(x);
            x = self.high_shelf.process(x);
            x = self.lpf.process(x);

            if self.chorus_on {
                x = self.chorus.process(x, self.chorus_mix);
            }
            if self.phaser_on {
                x = self.phaser.process(x, self.phaser_mix, self.phaser_rate);
            }
            if self.tremolo_on {
                self.tremolo_phase += 2.0 * PI * self.tremolo_rate as f64 / sample_rate as f64;
                if self.tremolo_phase > 2.0 * PI {
                    self.tremolo_phase -= 2.0 * PI;
                }
                x *= 1f32
                    - self.tremolo_depth_s.next(self.tremolo_depth) * (0.5f32 - 0.5f32 * self.tremolo_phase.cos() as f32);
            }

            if self.echo_on {
                x = self.echo.process(x, self.echo_mix, self.echo_ms, self.echo_fb);
            }
            if self.reverb_on {
                x = self.reverb.process(x, self.reverb_mix, self.reverb_size, self.reverb_damp);
            }

            *sample = self.limiter.process(x * self.out_s.next(self.out_gain));
        }
    }

    /// Seconds of silence worth appending when rendering so echo and reverb tails are not cut off.
    pub fn tail_seconds(p: &VoiceParams) -> f64 {
        let mut t: f64 = 0.0;
        if p.get(P::EchoMix) > 0.5f32 {
            let fb = (p.get(P::EchoFb) as f64 / 100.0).min(0.9);
            t = t.max(4f64.min(p.get(P::EchoMs) as f64 / 1000.0 * (1.0 + 4.0 * fb / (1.0 - fb) / 4.0) * 3.0));
        }
        if p.get(P::ReverbMix) > 0.5f32 {
            t = t.max(1.0 + 3.0 * p.get(P::ReverbSize) as f64 / 100.0);
        }
        t
    }
}

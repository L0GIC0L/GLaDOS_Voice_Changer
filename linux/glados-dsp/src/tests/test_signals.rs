//! Port of `TestSignals.kt`: synthetic voices and measurements shared by the tests.

use std::f64::consts::PI;

use crate::java_random::JavaRandom;
use crate::yin::Yin;

pub const VOWEL_A: [(f64, f64); 3] = [(700.0, 110.0), (1200.0, 120.0), (2600.0, 160.0)];

/// The optional arguments of the Kotlin `voice`, with its defaults.
#[derive(Clone, Copy)]
pub struct VoiceOpts<'a> {
    pub formants: &'a [(f64, f64)],
    pub amp: f32,
    pub jitter: f64,
    pub seed: i64,
}

impl Default for VoiceOpts<'_> {
    fn default() -> Self {
        VoiceOpts { formants: &VOWEL_A, amp: 0.3, jitter: 0.002, seed: 1 }
    }
}

/// Additive vowel-like signal with the default options.
pub fn voice(sr: i32, seconds: f64, f0: impl Fn(f64) -> f64) -> Vec<f32> {
    voice_with(sr, seconds, f0, VoiceOpts::default())
}

/// Additive vowel-like signal: harmonics of a time-varying f0 shaped by formant peaks and a
/// 12 dB/octave tilt. Light jitter keeps it from being unrealistically perfect.
pub fn voice_with(sr: i32, seconds: f64, f0: impl Fn(f64) -> f64, o: VoiceOpts) -> Vec<f32> {
    let n = (seconds * sr as f64) as i32;
    let mut out = vec![0f32; n as usize];
    let mut rnd = JavaRandom::new(o.seed);
    let mut phase = 0.0;
    let mut amps = [0.0f64; 160];
    let mut harmonics = 0;
    for i in 0..n {
        let t = i as f64 / sr as f64;
        let f = f0(t) * (1.0 + o.jitter * rnd.next_gaussian() * 0.3);
        if i % 64 == 0 {
            harmonics = 1.max((amps.len() as i32).min((0.45 * sr as f64 / f) as i32));
            for k in 1..=harmonics {
                let hz = k as f64 * f;
                let mut env = 0.02;
                for &(fc, bw) in o.formants {
                    let q = (hz - fc) / bw;
                    env += 1.0 / (1.0 + q * q);
                }
                amps[(k - 1) as usize] = env / (k as f64 * k as f64);
            }
        }
        phase += 2.0 * PI * f / sr as f64;
        if phase > 2.0 * PI {
            phase -= 2.0 * PI;
        }
        let mut s = 0.0;
        for k in 1..=harmonics {
            s += amps[(k - 1) as usize] * (k as f64 * phase).sin();
        }
        out[i as usize] = s as f32;
    }
    normalise(&mut out, o.amp);
    out
}

pub fn normalise(x: &mut [f32], peak: f32) {
    let mut m = 0f32;
    for v in x.iter() {
        m = m.max(v.abs());
    }
    if m > 0.0 {
        for v in x.iter_mut() {
            *v = *v / m * peak;
        }
    }
}

/// f0 per hop (Hz), 0 where unvoiced. Frames start at `from` samples. Searches 60 to 900 Hz.
pub fn pitch_track(x: &[f32], sr: i32, from: i32, to: i32, hop: i32) -> Vec<f64> {
    pitch_track_range(x, sr, from, to, hop, 60.0, 900.0)
}

pub fn pitch_track_range(x: &[f32], sr: i32, from: i32, to: i32, hop: i32, fmin: f64, fmax: f64) -> Vec<f64> {
    let mut yin = Yin::new(sr, fmin, fmax);
    let w = yin.window_length;
    let mut out = Vec::new();
    let mut s = from;
    while s + w <= to {
        out.push(if yin.detect(x, -1, s, 0.15) { yin.f0 } else { 0.0 });
        s += hop;
    }
    out
}

pub fn median(v: &[f64]) -> f64 {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| *x > 0.0).collect();
    s.sort_by(f64::total_cmp);
    if s.is_empty() { 0.0 } else { s[s.len() / 2] }
}

pub fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

pub fn rms(x: &[f32]) -> f64 {
    rms_range(x, 0, x.len() as i32)
}

pub fn rms_range(x: &[f32], from: i32, to: i32) -> f64 {
    let mut s = 0.0;
    for i in from..to {
        let v = x[i as usize] as f64;
        s += v * v;
    }
    (s / 1.max(to - from) as f64).sqrt()
}

/// Power-weighted mean frequency of `x` in `[from, to)` using a plain DFT over the audible range.
pub fn spectral_centroid(x: &[f32], sr: i32, from: i32, to: i32) -> f64 {
    let n = to - from;
    let mut num = 0.0;
    let mut den = 0.0;
    let mut f = 100.0;
    while f < 6000.0 {
        let mut re = 0.0;
        let mut im = 0.0;
        let w = 2.0 * PI * f / sr as f64;
        for i in 0..n {
            let win = 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos();
            let v = x[(from + i) as usize] as f64;
            re += v * win * (w * i as f64).cos();
            im -= v * win * (w * i as f64).sin();
        }
        let p = re * re + im * im;
        num += f * p;
        den += p;
        f += 25.0;
    }
    num / den
}

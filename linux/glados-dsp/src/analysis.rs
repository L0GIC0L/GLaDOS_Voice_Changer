use std::sync::OnceLock;

use crate::kmath::round_f64;
use crate::marks::Marks;
use crate::model::{VoiceParams, P};
use crate::pitch_bandpass::PitchBandpass;
use crate::scales::pitch;
use crate::voicing::Voicing;
use crate::yin::Yin;

/// Pitch, voicing, loudness and pitch marks for a whole clip. Frame i is centred on sample i * hop.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub sample_rate: i32,
    pub hop: i32,
    pub length: i32,
    pub f0: Vec<f32>,
    pub voiced: Vec<bool>,
    pub level: Vec<f32>,
    pub marks: Vec<i32>,
    pub a4: f64,
    pub aperiodicity: Vec<f32>,
    loud_level: OnceLock<f32>,
    median: OnceLock<f64>,
}

impl Analysis {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sample_rate: i32, hop: i32, length: i32, f0: Vec<f32>, voiced: Vec<bool>, level: Vec<f32>, marks: Vec<i32>,
        a4: f64, aperiodicity: Vec<f32>,
    ) -> Self {
        Analysis {
            sample_rate, hop, length, f0, voiced, level, marks, a4, aperiodicity,
            loud_level: OnceLock::new(),
            median: OnceLock::new(),
        }
    }

    pub fn frames(&self) -> i32 {
        self.f0.len() as i32
    }

    pub fn midi(&self, frame: i32) -> f64 {
        pitch::hz_to_midi(self.f0[frame as usize] as f64, self.a4)
    }

    /// Interpolated pitch (MIDI) at a fractional frame, using the nearest voiced frames.
    pub fn midi_at(&self, frame: f64) -> f64 {
        let frames = self.frames();
        let i = (frame.floor() as i32).clamp(0, frames - 1);
        let j = (i + 1).min(frames - 1);
        let f = (frame - i as f64).clamp(0.0, 1.0);
        let (vi, vj) = (self.voiced[i as usize], self.voiced[j as usize]);
        if vi && vj {
            self.midi(i) * (1.0 - f) + self.midi(j) * f
        } else if vi {
            self.midi(i)
        } else if vj {
            self.midi(j)
        } else {
            f64::NAN
        }
    }

    /// Loudness near the top of the clip (95th percentile frame level), used to scale note thickness.
    pub fn loud_level(&self) -> f32 {
        *self.loud_level.get_or_init(|| {
            let mut sorted = self.level.clone();
            sorted.sort_by(f32::total_cmp);
            if sorted.is_empty() {
                1e-4
            } else {
                let v = sorted[((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1)];
                if v < 1e-4 { 1e-4 } else { v }
            }
        })
    }

    /// Median pitch of voiced frames (MIDI), or NaN for a clip with no voiced sound.
    pub fn median_midi(&self) -> f64 {
        *self.median.get_or_init(|| {
            let mut v: Vec<f64> = (0..self.frames()).filter(|&i| self.voiced[i as usize]).map(|i| self.midi(i)).collect();
            v.sort_by(f64::total_cmp);
            if v.is_empty() { f64::NAN } else { v[v.len() / 2] }
        })
    }
}

pub struct Analyzer;

impl Analyzer {
    pub fn analyze(
        x: &[f32],
        sample_rate: i32,
        params: &VoiceParams,
        progress: Option<&dyn Fn(f32)>,
        cancelled: &dyn Fn() -> bool,
        threshold_override: Option<f64>,
    ) -> Analysis {
        let fmin = params.get(P::Fmin) as f64;
        let fmax = (params.get(P::Fmax) as f64).max(fmin * 2.0);
        let a4 = params.get(P::RefA4) as f64;
        let threshold = threshold_override.unwrap_or_else(|| Voicing::threshold(params.get(P::Voicing)));

        let n = x.len() as i32;
        let hop = 32.max(round_f64(sample_rate as f64 * 0.005));
        let frames = n / hop + 1;
        let mut yin = Yin::new(sample_rate, fmin, fmax);
        let w = yin.window_length;
        let pad = w + 64;
        // Pitch is tracked on a band-passed copy (rumble and hiss removed); the audio itself is untouched.
        let band = PitchBandpass::zero_phase(x, sample_rate, fmin, fmax);
        let mut xp = vec![0f32; x.len() + 2 * pad as usize];
        xp[pad as usize..pad as usize + x.len()].copy_from_slice(&band);

        let fr = frames as usize;
        let mut f0 = vec![0f32; fr];
        let mut periodic = vec![false; fr];
        let mut aperiodicity = vec![0f32; fr];
        let mut level = vec![0f32; fr];
        for i in 0..frames {
            if cancelled() {
                break;
            }
            let iu = i as usize;
            let center = i * hop;
            periodic[iu] = yin.detect(&xp, -1, center - w / 2 + pad, threshold);
            f0[iu] = yin.f0 as f32;
            aperiodicity[iu] = yin.aperiodicity as f32;
            let mut s = 0.0;
            let a = 0.max(center - hop);
            let b = n.min(center + hop);
            for k in a..b {
                let v = band[k as usize] as f64;
                s += v * v;
            }
            level[iu] = (s / 1.max(b - a) as f64).sqrt() as f32;
            if i % 256 == 0 {
                if let Some(p) = progress {
                    p(i as f32 / frames as f32 * 0.9);
                }
            }
        }

        let mut sorted = level.clone();
        sorted.sort_by(f32::total_cmp);
        let loud = sorted[((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1)];
        let silence = 2e-5f32.max(loud * 0.01);
        let (lo, hi) = (fmin as f32, fmax as f32);
        let mut voiced: Vec<bool> =
            (0..fr).map(|i| periodic[i] && level[i] > silence && f0[i] >= lo && f0[i] <= hi).collect();

        grow_voiced(&f0, &mut voiced, &aperiodicity, &level, silence, threshold);
        fix_octaves(&mut f0, &voiced);
        fill_gaps(&mut f0, &mut voiced);
        drop_short_runs(&mut voiced, 4);
        median_smooth(&mut f0, &voiced);

        let marks = track_marks(&xp, pad, n, sample_rate, hop, &f0, &voiced, cancelled);
        if let Some(p) = progress {
            p(1.0);
        }
        Analysis::new(sample_rate, hop, n, f0, voiced, level, marks, a4, aperiodicity)
    }
}

/// Hysteresis: a frame next to confirmed voice is kept if it is nearly periodic and its pitch is
/// continuous with the neighbour, so onsets, offsets and brief dips in a vowel are not lost.
fn grow_voiced(f0: &[f32], voiced: &mut [bool], ap: &[f32], level: &[f32], silence: f32, threshold: f64) {
    let loose = threshold * 1.5;
    let ok = |voiced: &[bool], i: usize, from: usize| -> bool {
        !voiced[i] && (ap[i] as f64) < loose && level[i] > silence && (f0[i] as f64 / f0[from] as f64).ln().abs() < 0.12
    };
    for i in 1..f0.len() {
        if voiced[i - 1] && ok(voiced, i, i - 1) {
            voiced[i] = true;
        }
    }
    for i in (0..f0.len().saturating_sub(1)).rev() {
        if voiced[i + 1] && ok(voiced, i, i + 1) {
            voiced[i] = true;
        }
    }
}

/// Pulls isolated octave jumps back to the pitch of their neighbours.
fn fix_octaves(f0: &mut [f32], voiced: &[bool]) {
    let ln2 = 2.0f64.ln();
    let l2: Vec<f64> = (0..f0.len()).map(|i| if voiced[i] { (f0[i] as f64).ln() / ln2 } else { 0.0 }).collect();
    let mut tmp: Vec<f64> = Vec::with_capacity(9);
    for i in 0..f0.len() {
        if !voiced[i] {
            continue;
        }
        tmp.clear();
        for k in -4i32..=4 {
            let j = i as i64 + k as i64;
            if j >= 0 && (j as usize) < f0.len() && voiced[j as usize] {
                tmp.push(l2[j as usize]);
            }
        }
        tmp.sort_by(f64::total_cmp);
        let med = tmp[tmp.len() / 2];
        let d = l2[i] - med;
        let k = round_f64(d);
        if k != 0 && (d - k as f64).abs() < 0.12 {
            f0[i] = 2.0f64.powf(l2[i] - k as f64) as f32;
        }
    }
}

/// Bridges one or two unvoiced frames inside a voiced stretch.
fn fill_gaps(f0: &mut [f32], voiced: &mut [bool]) {
    let size = f0.len();
    let mut i = 1;
    while i + 1 < size {
        if !voiced[i] && voiced[i - 1] {
            let mut j = i;
            while j < size && !voiced[j] {
                j += 1;
            }
            if j < size && j - i <= 4 && (f0[j] as f64 / f0[i - 1] as f64).ln().abs() < 0.12 {
                for k in i..j {
                    let t = (k - i + 1) as f32 / (j - i + 1) as f32;
                    f0[k] = f0[i - 1] * (1.0 - t) + f0[j] * t;
                    voiced[k] = true;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

fn drop_short_runs(voiced: &mut [bool], min_frames: usize) {
    let mut i = 0;
    while i < voiced.len() {
        if voiced[i] {
            let mut j = i;
            while j < voiced.len() && voiced[j] {
                j += 1;
            }
            if j - i < min_frames {
                for v in &mut voiced[i..j] {
                    *v = false;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

fn median_smooth(f0: &mut [f32], voiced: &[bool]) {
    let src = f0.to_vec();
    for i in 1..f0.len().saturating_sub(1) {
        if voiced[i - 1] && voiced[i] && voiced[i + 1] {
            let (a, b, c) = (src[i - 1], src[i], src[i + 1]);
            f0[i] = a.min(b).max(a.max(b).min(c));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn track_marks(
    xp: &[f32], pad: i32, n: i32, sr: i32, hop: i32, f0: &[f32], voiced: &[bool], cancelled: &dyn Fn() -> bool,
) -> Vec<i32> {
    let mut marks: Vec<i32> = Vec::new();
    let size = f0.len() as i32;
    let period_at = |t: i32| -> f64 {
        let i = round_f64(t as f64 / hop as f64).clamp(0, size - 1);
        let mut k = i;
        let mut d = 0;
        while !voiced[k as usize] && d < 6 {
            d += 1;
            k = (i + if d % 2 == 1 { (d + 1) / 2 } else { -(d / 2) }).clamp(0, size - 1);
        }
        sr as f64 / f0[k as usize] as f64
    };

    let mut i = 0;
    while i < size {
        if !voiced[i as usize] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < size && voiced[j as usize] {
            j += 1;
        }
        let s0 = 0.max(i * hop - hop / 2);
        let s1 = (n - 1).min((j - 1) * hop + hop / 2);
        let mut m = (Marks::peak(xp, -1, (s0 + pad) as i64, period_at(s0) as i32) - pad as i64) as i32;
        marks.push(m);
        while !cancelled() {
            let t = period_at(m);
            let next = (Marks::next(xp, -1, (m + pad) as i64, t) - pad as i64) as i32;
            if next > s1 || next <= m {
                break;
            }
            marks.push(next);
            m = next;
        }
        i = j;
    }
    marks
}

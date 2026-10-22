use std::f64::consts::PI;
use std::sync::Arc;

use crate::analysis::Analysis;
use crate::grain::Grain;
use crate::kmath::powf;
use crate::notes::{Edit, TimeMap};
use crate::scales::pitch;
use crate::speed_warp::SpeedWarp;

/// Global settings applied on top of the per-note edits.
#[derive(Clone, Debug)]
pub struct RenderSettings {
    pub pitch_shift: f64,
    pub formant_shift: f64,
    pub breath: f32,
    pub vibrato_cents: f64,
    pub vibrato_hz: f64,
    /// 0..1 share of the original, unedited voice blended into the result.
    pub dry: f32,
    /// 0..1 level of a second voice [`RenderSettings::harmony_st`] semitones away.
    pub harmony_mix: f32,
    pub harmony_st: f64,
    /// Tempo and random speed changes; `None` keeps the timeline's own speed. Must be built for the clip's length.
    pub warp: Option<Arc<SpeedWarp>>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        RenderSettings {
            pitch_shift: 0.0,
            formant_shift: 0.0,
            breath: 1.0,
            vibrato_cents: 0.0,
            vibrato_hz: 5.5,
            dry: 0.0,
            harmony_mix: 0.0,
            harmony_st: -12.0,
            warp: None,
        }
    }
}

const PAD: usize = 8192;

/// Non-real-time PSOLA: rebuilds the clip from the edited notes, honouring pitch, formant, gain and
/// timing edits. Marks and the pitch contour come from a full-clip [`Analysis`], so it can look ahead and stretch time.
/// The result is [`SpeedWarp::rendered_length`] samples long when [`RenderSettings::warp`] is set, otherwise as long
/// as `x`.
pub struct OfflineRenderer;

impl OfflineRenderer {
    pub fn render(x: &[f32], an: &Analysis, edit: &Edit, settings: &RenderSettings, cancelled: &dyn Fn() -> bool) -> Vec<f32> {
        let n = x.len() as i32;
        let identity;
        let warp: &SpeedWarp = match &settings.warp {
            Some(w) => w,
            None => {
                identity = SpeedWarp::identity(n);
                &identity
            }
        };
        let time_map = edit.time_map(n);
        let mut out = render_voice(x, an, edit, &time_map, warp, settings, settings.pitch_shift, cancelled);
        let h = settings.harmony_mix.clamp(0.0, 1.0);
        if h > 0.0 {
            let harmony =
                render_voice(x, an, edit, &time_map, warp, settings, settings.pitch_shift + settings.harmony_st, cancelled);
            let level = 1f32 / (1f32 + 0.5f32 * h);
            for (o, hv) in out.iter_mut().zip(&harmony) {
                *o = (*o + hv * h) * level;
            }
        }
        let d = settings.dry.clamp(0.0, 1.0);
        if d > 0.0 {
            // The original voice, following any timing and speed changes so it stays in step with the tuned one.
            for (i, o) in out.iter_mut().enumerate() {
                let a = time_map.to_input(warp.to_timeline(i as f64)).clamp(0.0, (n - 1) as f64);
                let j = a as i32;
                let f = (a - j as f64) as f32;
                let (j, j1) = (j as usize, (j + 1).min(n - 1) as usize);
                let dry_v = x[j] + (x[j1] - x[j]) * f;
                *o = *o * (1f32 - d) + dry_v * d;
            }
        }
        out
    }
}

#[allow(clippy::too_many_arguments)]
fn render_voice(
    x: &[f32], an: &Analysis, edit: &Edit, time_map: &TimeMap, warp: &SpeedWarp, settings: &RenderSettings,
    pitch_shift: f64, cancelled: &dyn Fn() -> bool,
) -> Vec<f32> {
    let n = x.len() as i32;
    let out_len = warp.rendered_length;
    let sr = an.sample_rate;
    let mut src = vec![0f32; x.len() + 2 * PAD];
    src[PAD..PAD + x.len()].copy_from_slice(x);
    let mut acc = vec![0f32; out_len as usize + 2 * PAD];
    let mut norm = vec![0f32; out_len as usize + 2 * PAD];

    let frames = an.frames();
    let mut note_of_frame = vec![-1i32; frames as usize];
    for (idx, note) in edit.notes.iter().enumerate() {
        for f in 0.max(note.f0)..=(frames - 1).min(note.f1) {
            note_of_frame[f as usize] = idx as i32;
        }
    }
    let note_at = |fi: i32| {
        let k = note_of_frame[fi as usize];
        if k >= 0 { edit.notes.get(k as usize) } else { None }
    };
    let marks = &an.marks;
    let unvoiced_half = 16f64.max(sr as f64 * 0.005);
    let base_rho = 2f64.powf(settings.formant_shift / 12.0);
    let pad = PAD as f64;

    let mut s = 0.0;
    let mut grains = 0;
    while s < out_len as f64 {
        grains += 1;
        if grains % 512 == 0 && cancelled() {
            break;
        }
        let a = time_map.to_input(warp.to_timeline(s)).clamp(0.0, (n - 1) as f64);
        let frame = a / an.hop as f64;
        let fi = (frame as i32).clamp(0, frames - 1);
        let mut mark_idx: i32 = -1;
        let mut tin = 0.0;
        if an.voiced[fi as usize] && !marks.is_empty() {
            mark_idx = nearest_mark(marks, a);
            tin = period_at(an, marks[mark_idx as usize]);
            if (marks[mark_idx as usize] as f64 - a).abs() > 0.75 * tin {
                mark_idx = -1;
            }
        }

        let note = note_at(fi);
        let rho = (base_rho * 2f64.powf(note.map_or(0f32, |nt| nt.formant) as f64 / 12.0)).clamp(0.4, 2.5);
        let note_gain = match note {
            Some(nt) if nt.gain_db != 0.0 => powf(10.0, nt.gain_db / 20.0),
            _ => 1.0,
        };
        if mark_idx >= 0 {
            let in_midi = an.midi_at(frame);
            let vibrato = settings.vibrato_cents / 100.0 * (2.0 * PI * settings.vibrato_hz * s / sr as f64).sin();
            let out_midi = note.map_or(in_midi, |nt| nt.pitch_at(frame)) + pitch_shift + vibrato;
            let ts = sr as f64 / pitch::midi_to_hz(out_midi, an.a4).clamp(40.0, 1600.0);
            let gain = note_gain;
            let half = tin.max(0.6 * ts * rho).min(2.0 * tin);
            let center = (marks[mark_idx as usize] + PAD as i32) as f64;
            Grain::add(&src, -1, center, half, &mut acc, &mut norm, -1, s + pad, rho, gain);
            s += ts;
        } else {
            let gain = settings.breath * note_gain;
            Grain::add(&src, -1, a + pad, unvoiced_half, &mut acc, &mut norm, -1, s + pad, rho, gain);
            s += unvoiced_half / rho;
        }
    }

    (0..out_len as usize).map(|i| acc[i + PAD] / norm[i + PAD].max(Grain::NORM_FLOOR)).collect()
}

fn nearest_mark(marks: &[i32], t: f64) -> i32 {
    let mut lo = 0usize;
    let mut hi = marks.len() - 1;
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if (marks[mid] as f64) < t {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo > 0 && (marks[lo - 1] as f64 - t).abs() <= (marks[lo] as f64 - t).abs() { lo as i32 - 1 } else { lo as i32 }
}

fn period_at(an: &Analysis, t: i32) -> f64 {
    let frames = an.frames();
    let i = ((t as f64 / an.hop as f64) as i32).clamp(0, frames - 1);
    let mut k = i;
    let mut d = 0;
    while !an.voiced[k as usize] && d < 6 {
        d += 1;
        k = (i + if d % 2 == 1 { (d + 1) / 2 } else { -(d / 2) }).clamp(0, frames - 1);
    }
    an.sample_rate as f64 / an.f0[k as usize] as f64
}

//! Port of `OfflineTest.kt`.

use std::sync::Arc;

use super::test_signals::{self as ts, VoiceOpts};
use super::{assert_close, never};
use crate::analysis::{Analysis, Analyzer};
use crate::model::{VoiceParams, P};
use crate::notes::{Edit, NoteOps};
use crate::offline_renderer::{OfflineRenderer, RenderSettings};
use crate::scales::{self, pitch, CHROMATIC_MASK};
use crate::speed_warp::SpeedWarp;

const SR: i32 = 48000;

fn params() -> VoiceParams {
    VoiceParams::defaults().with_all([("autoReg", 0.0), ("formant", 0.0), ("formantFollow", 0.0), ("shift", 0.0)])
}

/// Three "syllables": 150 Hz, 200 Hz, 120 Hz with 80 ms silences between. Returns signal and note start/end seconds.
fn phrase() -> (Vec<f32>, Vec<(f64, f64)>) {
    let hz = [150.0, 200.0, 120.0];
    let dur = 0.4;
    let gap = 0.08;
    let total = 0.1 + 3.0 * dur + 2.0 * gap + 0.1;
    let mut x = vec![0f32; (total * SR as f64) as usize];
    let mut spans = Vec::new();
    let mut t = 0.1;
    for (k, h) in hz.iter().enumerate() {
        let seg = ts::voice_with(SR, dur, |_| *h, VoiceOpts { seed: k as i64 + 1, ..Default::default() });
        let n = seg.len();
        let ramp = (0.015 * SR as f64) as i32;
        let start = (t * SR as f64) as usize;
        for i in 0..n {
            let env = 1f32.min(i as f32 / ramp as f32).min((n - i) as f32 / ramp as f32);
            x[start + i] = seg[i] * env;
        }
        spans.push((t, t + dur));
        t += dur + gap;
    }
    (x, spans)
}

fn analyse(x: &[f32]) -> Analysis {
    Analyzer::analyze(x, SR, &params(), None, &never, None)
}

fn segment(an: &Analysis) -> Edit {
    NoteOps::segment(an, 80.0, 0.5)
}

fn render(x: &[f32], an: &Analysis, edit: &Edit, settings: &RenderSettings) -> Vec<f32> {
    OfflineRenderer::render(x, an, edit, settings, &never)
}

fn median_hz(y: &[f32], from: f64, to: f64) -> f64 {
    ts::median(&ts::pitch_track(y, SR, (from * SR as f64) as i32, (to * SR as f64) as i32, 480))
}

fn hz_to_midi(hz: f64) -> f64 {
    pitch::hz_to_midi(hz, 440.0)
}

fn midi_to_hz(m: f64) -> f64 {
    pitch::midi_to_hz(m, 440.0)
}

fn secs(frame: i32, an: &Analysis) -> f64 {
    (frame * an.hop) as f64 / SR as f64
}

#[test]
fn finds_one_note_per_syllable() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let edit = segment(&an);
    let desc: Vec<String> =
        edit.notes.iter().map(|n| format!("{}..{} c={:.2}", secs(n.f0, &an), secs(n.f1, &an), n.center)).collect();
    println!("notes: {desc:?}");
    assert_eq!(3, edit.notes.len());
    let expected: Vec<f64> = [150.0, 200.0, 120.0].iter().map(|h| hz_to_midi(*h)).collect();
    for k in 0..3 {
        assert_close(expected[k], edit.notes[k].center, 0.15, "centre");
        assert_close(spans[k].0, secs(edit.notes[k].f0, &an), 0.06, "start");
        assert_close(spans[k].1, secs(edit.notes[k].f1, &an), 0.06, "end");
    }
    assert!(an.marks.len() > 100, "marks found");
}

#[test]
fn unedited_render_keeps_pitch_and_level() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let edit = segment(&an);
    let y = render(&x, &an, &edit, &RenderSettings::default());
    for (a, b) in &spans {
        let hz = median_hz(&y, a + 0.08, b - 0.08);
        let hz_in = median_hz(&x, a + 0.08, b - 0.08);
        assert_close(0.0, ts::cents(hz, hz_in), 8.0, "pitch");
    }
    let ratio = ts::rms(&y) / ts::rms(&x);
    assert_close(1.0, ratio, 0.1, "level");
    for v in &y {
        assert!(!(v.is_nan() || v.abs() > 1.5));
    }
}

#[test]
fn pitch_offset_moves_only_that_note() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    edit.notes[1].offset = 3.0;
    let y = render(&x, &an, &edit, &RenderSettings::default());
    let before = median_hz(&x, spans[1].0 + 0.08, spans[1].1 - 0.08);
    assert_close(
        300.0 / 100.0 * 100.0 / 100.0 * 0.0 + 3.0 * 100.0,
        ts::cents(median_hz(&y, spans[1].0 + 0.08, spans[1].1 - 0.08), before),
        15.0,
        "moved note",
    );
    assert_close(
        0.0,
        ts::cents(median_hz(&y, spans[0].0 + 0.08, spans[0].1 - 0.08), median_hz(&x, spans[0].0 + 0.08, spans[0].1 - 0.08)),
        8.0,
        "other note",
    );
}

#[test]
fn correction_snaps_notes_to_the_scale_and_flattens() {
    let (mut x, spans) = phrase();
    // Add vibrato to the middle syllable so there is something to flatten.
    let v = ts::voice_with(
        SR,
        0.4,
        |t| 200.0 * 2f64.powf(0.5 * (2.0 * std::f64::consts::PI * 5.5 * t).sin() / 12.0),
        VoiceOpts { seed: 9, ..Default::default() },
    );
    let start = (spans[1].0 * SR as f64) as usize;
    x[start..start + v.len()].copy_from_slice(&v);
    let an = analyse(&x);
    let mut edit = segment(&an);
    assert_eq!(3, edit.notes.len());
    NoteOps::correct(&mut edit.notes, CHROMATIC_MASK, 1.0, 1.0, 1.0, 0.0, false, 57);
    let y = render(&x, &an, &edit, &RenderSettings::default());
    for (k, hz) in [150.0, 200.0, 120.0].iter().enumerate() {
        let target = midi_to_hz(hz_to_midi(*hz).round_ties_even());
        let track = ts::pitch_track(
            &y,
            SR,
            ((spans[k].0 + 0.08) * SR as f64) as i32,
            ((spans[k].1 - 0.06) * SR as f64) as i32,
            480,
        );
        assert!(track.iter().any(|f| *f > 0.0), "note {k} has no voiced frames");
        let worst = track.iter().filter(|f| **f > 0.0).map(|f| ts::cents(*f, target).abs()).fold(f64::MIN, f64::max);
        println!("note {k} target={target:.2} worst deviation={worst:.1} cents");
        assert!(worst < 20.0, "note {k} worst {worst}");
    }
}

#[test]
fn scale_correction_moves_to_allowed_notes() {
    let (x, _) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    let mask = scales::mask(1, 0, 0); // C major
    NoteOps::correct(&mut edit.notes, mask, 1.0, 1.0, 1.0, 0.0, false, 57);
    for n in &edit.notes {
        let final_note = n.center + n.offset;
        assert_close(0.0, final_note - final_note.round_ties_even(), 1e-6, "on a note");
        assert!(scales::is_allowed(mask, final_note.round_ties_even() as i32));
    }
}

#[test]
fn formant_edit_shifts_spectrum_not_pitch() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    let y0 = render(&x, &an, &edit, &RenderSettings::default());
    edit.notes[0].formant = 5.0;
    let y1 = render(&x, &an, &edit, &RenderSettings::default());
    let a = (spans[0].0 * SR as f64) as i32 + 4800;
    let c0 = ts::spectral_centroid(&y0, SR, a, a + 4800);
    let c1 = ts::spectral_centroid(&y1, SR, a, a + 4800);
    println!("centroid {c0} -> {c1}");
    assert!(c1 > c0 * 1.1, "centroid {c0} -> {c1}");
    assert_close(
        0.0,
        ts::cents(median_hz(&y1, spans[0].0 + 0.08, spans[0].1 - 0.08), median_hz(&y0, spans[0].0 + 0.08, spans[0].1 - 0.08)),
        10.0,
        "pitch",
    );
}

#[test]
fn gain_edit_changes_level() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    edit.notes[2].gain_db = -12.0;
    let y = render(&x, &an, &edit, &RenderSettings::default());
    let a = ((spans[2].0 + 0.1) * SR as f64) as i32;
    let b = ((spans[2].1 - 0.1) * SR as f64) as i32;
    let db = 20.0 * (ts::rms_range(&y, a, b) / ts::rms_range(&x, a, b)).log10();
    assert_close(-12.0, db, 1.5, "gain");
}

#[test]
fn timing_edit_stretches_a_note() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    let n = &edit.notes[0];
    let key = n.in_end(&an);
    let length_before = (n.in_end(&an) - n.in_start(&an)) as f64;
    let applied = edit.move_anchors([key], 0.15 * length_before, 0.005 * SR as f64, an.length);
    println!("applied {applied} of {}", 0.15 * length_before);
    assert_close(0.15 * length_before, applied, 1.0, "applied");
    let y = render(&x, &an, &edit, &RenderSettings::default());
    assert_eq!(x.len(), y.len());
    // Pitch of the stretched note unchanged, and sound now continues past the original end.
    let stretched_end = spans[0].1 + 0.15 * length_before / SR as f64;
    assert_close(0.0, ts::cents(median_hz(&y, spans[0].0 + 0.08, spans[0].1 - 0.08), 150.0), 10.0, "stretched pitch");
    let tail_level =
        ts::rms_range(&y, ((spans[0].1 + 0.01) * SR as f64) as i32, ((stretched_end - 0.02) * SR as f64) as i32);
    assert!(tail_level > 0.02, "stretched region silent: {tail_level}");
    // The next note keeps its pitch.
    assert_close(0.0, ts::cents(median_hz(&y, spans[1].0 + 0.1, spans[1].1 - 0.08), 200.0), 12.0, "next note");
}

#[test]
fn anchor_moves_are_constrained() {
    let (x, _) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    let key = edit.notes[0].in_end(&an);
    let huge = edit.move_anchors([key], 1e9, 0.005 * SR as f64, an.length);
    let next_start = edit.notes[1].in_start(&an);
    assert!(edit.anchors[&key] < edit.anchors[&next_start], "moved past the next anchor");
    assert!(huge > 0.0);
    let keys = [key];
    let negative = edit.move_anchors(keys.iter(), -1e9, 0.005 * SR as f64, an.length); // borrowed keys work too
    assert!(negative < 0.0);
    assert!(edit.anchors[&key] > edit.anchors[&edit.notes[0].in_start(&an)]);
}

#[test]
fn split_and_merge_round_trip() {
    let (x, _) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    let n0 = &edit.notes[0];
    let mid = (n0.f0 + n0.f1) / 2;
    assert!(NoteOps::split(&mut edit, &an, 0, mid));
    assert_eq!(4, edit.notes.len());
    assert_eq!(mid, edit.notes[1].f0);
    assert_eq!(mid - 1, edit.notes[0].f1);
    let anchors_before = edit.anchors.len();
    assert!(NoteOps::merge(&mut edit, &an, 0));
    assert_eq!(3, edit.notes.len());
    assert_eq!(anchors_before - 1, edit.anchors.len());
    // Unedited split + merge must not change the audio.
    let y = render(&x, &an, &edit, &RenderSettings::default());
    let y0 = render(&x, &an, &segment(&an), &RenderSettings::default());
    let mut d = 0.0;
    for (a, b) in y.iter().zip(&y0) {
        d += (a - b).abs() as f64;
    }
    assert!(d / (y.len() as f64) < 0.01, "render changed by split+merge: {d}");
}

#[test]
fn pitch_shift_moves_whole_clip() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let edit = segment(&an);
    let y = render(&x, &an, &edit, &RenderSettings { pitch_shift: 7.0, ..Default::default() });
    for (a, b) in &spans {
        let before = median_hz(&x, a + 0.08, b - 0.08);
        assert_close(700.0, ts::cents(median_hz(&y, a + 0.08, b - 0.08), before), 15.0, "shift");
    }
}

#[test]
fn full_dry_blend_returns_the_original() {
    let (x, _) = phrase();
    let an = analyse(&x);
    let y = render(&x, &an, &segment(&an), &RenderSettings { pitch_shift: 5.0, dry: 1.0, ..Default::default() });
    for (a, b) in x.iter().zip(&y) {
        assert_close(*a as f64, *b as f64, 1e-6, "dry");
    }
}

#[test]
fn vibrato_wobbles_the_pitch() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let y = render(&x, &an, &segment(&an), &RenderSettings { vibrato_cents: 80.0, vibrato_hz: 5.0, ..Default::default() });
    let (a, b) = spans[0];
    let track: Vec<f64> = ts::pitch_track(&y, SR, ((a + 0.05) * SR as f64) as i32, ((b - 0.05) * SR as f64) as i32, 480)
        .into_iter()
        .filter(|f| *f > 0.0)
        .collect();
    let lowest = track.iter().copied().fold(f64::MAX, f64::min);
    let cents: Vec<f64> = track.iter().map(|f| ts::cents(*f, lowest)).collect();
    let spread = cents.iter().copied().fold(f64::MIN, f64::max) - cents.iter().copied().fold(f64::MAX, f64::min);
    println!("vibrato spread {spread} cents");
    assert!(spread > 80.0, "vibrato spread {spread} cents");
}

#[test]
fn double_speed_halves_the_length_but_keeps_the_pitch() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let warp = Arc::new(SpeedWarp::build(x.len() as i32, SR, 2.0, 0.0, x.len() as i64));
    let y = render(&x, &an, &segment(&an), &RenderSettings { warp: Some(warp.clone()), ..Default::default() });
    assert_close(x.len() as f64 / 2.0, y.len() as f64, x.len() as f64 * 0.01, "length");
    let sr = SR as f64;
    for (a, b) in &spans {
        let mid = (a + b) / 2.0 * sr;
        let ra = warp.to_rendered(mid - 0.1 * sr) / sr;
        let rb = warp.to_rendered(mid + 0.1 * sr) / sr;
        assert_close(0.0, ts::cents(median_hz(&y, ra, rb), median_hz(&x, mid / sr - 0.1, mid / sr + 0.1)), 15.0, "pitch");
    }
}

#[test]
fn random_speed_is_repeatable_and_maps_both_ways() {
    let n = SR * 6;
    let a = SpeedWarp::build(n, SR, 1.0, 0.8, n as i64);
    let b = SpeedWarp::build(n, SR, 1.0, 0.8, n as i64);
    assert_eq!(a.rendered_length, b.rendered_length);
    assert!((a.rendered_length - n).abs() as f64 > SR as f64 * 0.05, "random speed should change the length");
    let mut prev = -1.0;
    let mut fastest: f64 = 0.0;
    let mut slowest = f64::MAX;
    let mut u = 0.0;
    while u < n as f64 {
        let s = a.to_rendered(u);
        assert!(s > prev, "warp must move forward");
        assert_close(u, a.to_timeline(s), 1e-6, "round trip");
        assert_eq!(s, b.to_rendered(u));
        if prev >= 0.0 {
            let speed = (SR as f64 * 0.01) / (s - prev);
            fastest = fastest.max(speed);
            slowest = slowest.min(speed);
        }
        prev = s;
        u += SR as f64 * 0.01;
    }
    println!("random speed: {slowest:.2}x to {fastest:.2}x, length {} for {n}", a.rendered_length);
    assert!(fastest > 1.15, "never sped up: {fastest}");
    assert!(slowest < 0.87, "never slowed down: {slowest}");
    assert!(SpeedWarp::build(n, SR, 1.0, 0.0, n as i64).is_identity());
}

#[test]
fn an_octave_down_harmony_adds_the_lower_voice() {
    let (x, spans) = phrase();
    let an = analyse(&x);
    let edit = segment(&an);
    let y = render(&x, &an, &edit, &RenderSettings { harmony_mix: 1.0, harmony_st: -12.0, ..Default::default() });
    let main = render(&x, &an, &edit, &RenderSettings::default());
    let low = render(&x, &an, &edit, &RenderSettings { pitch_shift: -12.0, ..Default::default() });
    let (a, b) = spans[0];
    assert_close(-1200.0, ts::cents(median_hz(&low, a + 0.08, b - 0.08), median_hz(&x, a + 0.08, b - 0.08)), 15.0, "octave");
    // The result is the voice plus the octave-down copy, levelled so the pair is not much louder than one voice.
    for i in 0..y.len() {
        assert_close(((main[i] + low[i]) / 1.5) as f64, y[i] as f64, 1e-5, "mix");
    }
    for v in &y {
        assert!(!(v.is_nan() || v.abs() > 1.5));
    }
}

#[test]
fn global_shift_helper_matches_register() {
    let (x, _) = phrase();
    let an = analyse(&x);
    let p = params().with(P::AutoReg, 1.0).with(P::RegHz, 220.0);
    let shift = NoteOps::global_shift(&an, &p);
    let med = an.median_midi();
    assert_close(hz_to_midi(220.0), med + shift, 0.01, "register");
}

#[test]
fn odd_hop_keeps_touching_notes_on_one_shared_anchor() {
    // 44.1 kHz gives a 221-sample hop. Touching notes must share a single boundary anchor, or the Timing tool
    // cannot move them and splitting leaves a gap or an overlap.
    let sr44 = 44100;
    // Legato steps, no gaps, so the notes touch each other.
    let x = ts::voice_with(
        sr44,
        2.1,
        |t| if t < 0.7 { 120.0 } else if t < 1.4 { 150.0 } else { 190.0 },
        VoiceOpts { seed: 4, ..Default::default() },
    );
    let an = Analyzer::analyze(&x, sr44, &params(), None, &never, None);
    assert!(an.hop % 2 == 1, "expected an odd hop, got {}", an.hop);
    let mut edit = segment(&an);
    assert!(edit.notes.len() >= 3, "need several notes, got {}", edit.notes.len());
    assert!(edit.notes.windows(2).any(|w| w[1].f0 == w[0].f1 + 1), "need touching notes");
    for w in edit.notes.windows(2) {
        if w[1].f0 == w[0].f1 + 1 {
            assert_eq!(w[0].in_end(&an), w[1].in_start(&an), "touching notes must meet exactly");
        }
    }
    let i = edit.notes.iter().position(|n| n.frames() >= 12).unwrap();
    let n = &edit.notes[i];
    let mid = (n.f0 + n.f1) / 2;
    assert!(NoteOps::split(&mut edit, &an, i as i32, mid));
    let left = &edit.notes[i];
    let right = &edit.notes[i + 1];
    assert_eq!(left.in_end(&an), right.in_start(&an));
    let boundary = right.in_start(&an);
    assert!(edit.anchors.contains_key(&boundary), "the split boundary must be a real anchor");
    // The shared anchor moves both neighbours.
    let moved = edit.move_anchors([boundary], 20.0, 0.005 * sr44 as f64, an.length);
    assert!(moved != 0.0, "the boundary would not move ({moved})");
}

#[test]
fn split_and_merge_keep_the_edited_pitch() {
    let (x, _) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    let i = 0;
    NoteOps::correct([&mut edit.notes[i]], CHROMATIC_MASK, 1.0, 1.0, 1.0, 0.0, false, 57);
    let n = &edit.notes[i];
    let target = n.center + n.offset;
    let mid = (n.f0 + n.f1) / 2;
    assert!(NoteOps::split(&mut edit, &an, i as i32, mid));
    for half in [&edit.notes[i], &edit.notes[i + 1]] {
        assert_close(target, half.center + half.offset, 1e-9, "a split half drifted off the corrected pitch");
    }
    assert!(NoteOps::merge(&mut edit, &an, i as i32));
    assert_close(target, edit.notes[i].center + edit.notes[i].offset, 1e-9, "a merged note drifted off the corrected pitch");
}

#[test]
fn correct_to_settings_and_helpers_follow_the_voice_tab() {
    // Not in the Kotlin suite: exercises the settings-driven path the app uses, and the other helpers.
    let (x, _) = phrase();
    let an = analyse(&x);
    let mut edit = segment(&an);
    assert!(!edit.is_edited(&an));
    let p = params().with(P::Scale, 1.0).with(P::Retune, 0.0).with(P::Strength, 100.0);
    NoteOps::correct_to_settings(&mut edit.notes, &an, &p);
    assert!(edit.is_edited(&an));
    for n in &edit.notes {
        let m = n.center + n.offset;
        assert!(scales::is_allowed(NoteOps::scale_mask(&p), m.round_ties_even() as i32));
        assert_eq!(0.0, n.drift);
        assert_eq!(0.0, n.modulation);
    }
    let mono = p.with(P::Monotone, 1.0);
    let note = NoteOps::monotone_note(&an, &mono, 0.0, NoteOps::scale_mask(&mono));
    NoteOps::correct_to_settings(edit.notes.iter_mut().take(2), &an, &mono);
    for n in &edit.notes[..2] {
        assert_close(note as f64, n.center + n.offset, 1e-9, "monotone");
    }
    assert_close(1.5 + 0.35 * 7.0, NoteOps::formant_shift(&VoiceParams::defaults(), 7.0), 1e-6, "formant follow");
    let copy = edit.clone();
    assert_eq!(copy.notes.len(), edit.notes.len());
    assert!(an.loud_level() > 1e-3);
}

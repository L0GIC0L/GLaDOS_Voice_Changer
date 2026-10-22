//! Port of `LivePitchEngineTest.kt` as it was at 5fcfef1, before live mode left the Android app.

use super::assert_close;
use super::test_signals::{self as ts, VoiceOpts};
use crate::java_random::JavaRandom;
use crate::live_pitch_engine::LivePitchEngine;
use crate::model::{VoiceParams, P};
use crate::scales::pitch;

const SR: i32 = 48000;

fn base() -> VoiceParams {
    VoiceParams::defaults().with_all([
        ("autoReg", 0.0), ("formant", 0.0), ("formantFollow", 0.0), ("strength", 100.0), ("retuneMs", 0.0),
        ("shift", 0.0), ("gateDb", -80.0), ("breath", 100.0), ("dry", 0.0),
    ])
}

fn run_blocks(x: &[f32], p: &VoiceParams, block: usize) -> (Vec<f32>, LivePitchEngine) {
    let mut e = LivePitchEngine::new(SR, p);
    let mut out = vec![0f32; x.len()];
    for (inp, o) in x.chunks(block).zip(out.chunks_mut(block)) {
        e.process(inp, o);
    }
    (out, e)
}

fn run(x: &[f32], p: &VoiceParams) -> (Vec<f32>, LivePitchEngine) {
    run_blocks(x, p, 128)
}

fn assert_sane(y: &[f32]) {
    for v in y {
        assert!(v.is_finite(), "NaN/Inf in output");
        assert!(v.abs() < 2.0, "output exceeds range: {v}");
    }
}

fn track_from(y: &[f32], from: i32) -> Vec<f64> {
    ts::pitch_track(y, SR, from, y.len() as i32, 480)
}

fn hz_to_midi(hz: f64) -> f64 {
    pitch::hz_to_midi(hz, 440.0)
}

fn midi_to_hz(m: f64) -> f64 {
    pitch::midi_to_hz(m, 440.0)
}

/// Kotlin's `round`, which rounds halves to even.
fn kround(x: f64) -> f64 {
    x.round_ties_even()
}

#[test]
fn hard_tune_snaps_to_nearest_semitone() {
    // 150 Hz is 50.3 in MIDI: nearest note is D3 (50) = 146.83 Hz.
    let x = ts::voice(SR, 2.0, |_| 150.0);
    let (y, e) = run(&x, &base());
    assert_sane(&y);
    assert_eq!(0, e.starved_grains());
    let track = track_from(&y, SR / 2);
    let expected = midi_to_hz(50.0);
    assert_close(0.0, ts::cents(ts::median(&track), expected), 8.0, "median");
    let voiced = track.iter().filter(|f| **f > 0.0).count();
    assert!(voiced as f64 > track.len() as f64 * 0.9, "voiced {voiced}/{}", track.len());
    // The corrected pitch should be steady, not wobbling with the input jitter.
    let spread = track.iter().filter(|f| **f > 0.0).map(|f| ts::cents(*f, expected).abs()).fold(f64::MIN, f64::max);
    assert!(spread < 25.0, "max deviation {spread} cents");
}

#[test]
fn pitch_shift_moves_an_octave() {
    let x = ts::voice(SR, 1.5, |_| 140.0);
    let p = base().with(P::Strength, 0.0).with(P::Shift, 12.0);
    let (y, e) = run(&x, &p);
    assert_sane(&y);
    assert_eq!(0, e.starved_grains());
    let track = track_from(&y, SR / 2);
    assert_close(0.0, ts::cents(ts::median(&track), 280.0), 15.0, "octave up");
}

#[test]
fn octave_down_is_clean() {
    let x = ts::voice(SR, 1.5, |_| 240.0);
    let p = base().with(P::Strength, 0.0).with(P::Shift, -12.0);
    let (y, e) = run(&x, &p);
    assert_sane(&y);
    assert_eq!(0, e.starved_grains());
    let track = track_from(&y, SR / 2);
    let sample: Vec<i32> = track.iter().take(12).map(|f| *f as i32).collect();
    println!(
        "octave-down median={} voiced={}/{} sample={sample:?}",
        ts::median(&track),
        track.iter().filter(|f| **f > 0.0).count(),
        track.len()
    );
    assert_close(0.0, ts::cents(ts::median(&track), 120.0), 15.0, "octave down");
}

#[test]
fn formant_shift_leaves_pitch_alone() {
    let x = ts::voice(SR, 1.5, |_| 150.0);
    let p0 = base().with(P::Strength, 0.0);
    let (y0, _) = run(&x, &p0);
    let (y1, _) = run(&x, &p0.with(P::Formant, 5.0));
    let t0 = ts::median(&track_from(&y0, SR / 2));
    let t1 = ts::median(&track_from(&y1, SR / 2));
    assert_close(0.0, ts::cents(t1, t0), 15.0, "pitch moved by formant shift");
    let c0 = ts::spectral_centroid(&y0, SR, SR, SR + 4800);
    let c1 = ts::spectral_centroid(&y1, SR, SR, SR + 4800);
    println!("centroid no-shift={c0} shifted={c1}");
    assert!(c1 > c0 * 1.1, "formant shift should raise the spectral centroid ({c0} -> {c1})");
}

#[test]
fn formant_follows_pitch_shift() {
    let x = ts::voice(SR, 1.5, |_| 140.0);
    let p = base().with(P::Strength, 0.0).with(P::Shift, 7.0);
    let (flat, _) = run(&x, &p);
    let (follow, _) = run(&x, &p.with(P::FormantFollow, 100.0));
    let c0 = ts::spectral_centroid(&flat, SR, SR, SR + 4800);
    let c1 = ts::spectral_centroid(&follow, SR, SR, SR + 4800);
    println!("follow centroid {c0} -> {c1}");
    assert!(c1 > c0 * 1.15, "formants should rise when following the shift ({c0} -> {c1})");
    let t0 = ts::median(&track_from(&flat, SR / 2));
    let t1 = ts::median(&track_from(&follow, SR / 2));
    assert_close(0.0, ts::cents(t1, t0), 15.0, "pitch");
}

#[test]
fn no_correction_is_nearly_transparent() {
    // PSOLA keeps pitch and waveform shape but may slip by up to half a period, so compare
    // short windows allowing a local lag search, and check the level is preserved.
    let raw = ts::voice_with(SR, 1.5, |_| 150.0, VoiceOpts { jitter: 0.0, ..Default::default() });
    // The engine high-passes its input at ~15 Hz; apply the same filter to the reference.
    let mut x = vec![0f32; raw.len()];
    let mut px = 0f32;
    let mut py = 0f32;
    for (i, r) in raw.iter().enumerate() {
        let v = r - px + 0.998f32 * py;
        px = *r;
        py = v;
        x[i] = v;
    }
    let p = base().with(P::Strength, 0.0);
    let (y, e) = run(&raw, &p);
    assert_sane(&y);
    let lag = e.latency_samples() as usize;
    let win = 960;
    let mut residuals = Vec::new();
    let mut start = SR as usize;
    while start + win < y.len() - 400 {
        let mut best = f64::MAX;
        for d in lag - 330..=lag + 330 {
            let mut err = 0.0;
            let mut reference = 0.0;
            for i in start..start + win {
                let r = x[i - d] as f64;
                let diff = y[i] as f64 - r;
                err += diff * diff;
                reference += r * r;
            }
            best = best.min(err / reference);
        }
        residuals.push(10.0 * best.log10());
        start += 2400;
    }
    let mut sorted = residuals.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let all: Vec<i32> = residuals.iter().map(|r| *r as i32).collect();
    println!("transparent local residual median={median} dB all={all:?}");
    assert!(median < -24.0, "residual {median} dB");
    let n = y.len() as i32;
    let ratio = ts::rms_range(&y, SR, n - 400) / ts::rms_range(&x, SR, n - 400);
    assert_close(1.0, ratio, 0.12, "level ratio");
}

#[test]
fn glide_becomes_staircase() {
    let x = ts::voice(SR, 2.0, |t| 110.0 + 70.0 * t / 2.0);
    let (y, _) = run(&x, &base());
    let track = track_from(&y, SR / 4);
    let mut off = 0;
    let mut n = 0;
    for f in &track {
        if *f <= 0.0 {
            continue;
        }
        let m = hz_to_midi(*f);
        let dev = (m - kround(m)).abs() * 100.0;
        n += 1;
        if dev > 30.0 {
            off += 1;
        }
    }
    println!("off-grid frames {off}/{n}");
    assert!(off <= 2.max(n / 8), "too many frames off the semitone grid: {off}/{n}");
}

#[test]
fn custom_scale_only_uses_allowed_notes() {
    // C major only: every output note must be a white key.
    let x = ts::voice(SR, 2.0, |t| 120.0 + 90.0 * t / 2.0);
    let p = base().with(P::Scale, 1.0).with(P::Key, 0.0);
    let (y, _) = run(&x, &p);
    let track = track_from(&y, SR / 4);
    let white = [0, 2, 4, 5, 7, 9, 11];
    let mut bad = 0;
    let mut n = 0;
    for f in &track {
        if *f <= 0.0 {
            continue;
        }
        let m = kround(hz_to_midi(*f)) as i32;
        n += 1;
        if !white.contains(&m.rem_euclid(12)) {
            bad += 1;
        }
    }
    println!("non-scale frames {bad}/{n}");
    assert!(bad <= 2.max(n / 8), "non-scale frames {bad}/{n}");
}

#[test]
fn monotone_holds_one_note() {
    let x = ts::voice(SR, 2.0, |t| 130.0 + 30.0 * (t * 3.0).sin());
    let p = base().with(P::Monotone, 1.0).with(P::MonoNote, 57.0);
    let (y, _) = run(&x, &p);
    let track = track_from(&y, SR / 2);
    let target = midi_to_hz(57.0);
    let bad = track.iter().filter(|f| **f > 0.0 && ts::cents(**f, target).abs() > 40.0).count();
    println!("monotone off-note frames {bad}/{}", track.len());
    assert!(bad <= track.len() / 8, "off-note frames {bad}");
}

#[test]
fn auto_register_moves_voice_toward_target() {
    let x = ts::voice(SR, 4.0, |_| 110.0);
    let p = base().with(P::AutoReg, 1.0).with(P::RegHz, 220.0).with(P::Strength, 0.0);
    let (y, _) = run(&x, &p);
    let track = track_from(&y, 3 * SR);
    assert_close(0.0, ts::cents(ts::median(&track), 220.0), 40.0, "register");
}

#[test]
fn silence_stays_silent() {
    let (y, e) = run(&vec![0f32; SR as usize], &base());
    assert_sane(&y);
    assert!(ts::rms(&y) < 1e-6);
    assert_eq!(0, e.starved_grains());
}

#[test]
fn unvoiced_noise_passes_through_at_comparable_level() {
    let mut rnd = JavaRandom::new(5);
    let x: Vec<f32> = (0..SR).map(|_| (rnd.next_gaussian() * 0.05) as f32).collect();
    let (y, _) = run(&x, &base());
    let rin = ts::rms_range(&x, SR / 2, SR);
    let rout = ts::rms_range(&y, SR / 2, SR);
    println!("noise rms in={rin} out={rout}");
    assert!(rout > rin * 0.5 && rout < rin * 1.6, "noise level changed too much: {rin} -> {rout}");
}

#[test]
fn odd_block_sizes_are_equivalently_stable() {
    let x = ts::voice(SR, 1.5, |_| 176.0);
    for block in [64, 100, 240, 480, 1024] {
        let (y, e) = run_blocks(&x, &base(), block);
        assert_sane(&y);
        assert_eq!(0, e.starved_grains(), "block {block}");
        let med = ts::median(&track_from(&y, SR / 2));
        let expected = midi_to_hz(kround(hz_to_midi(176.0)));
        assert_close(0.0, ts::cents(med, expected), 12.0, &format!("block {block}"));
    }
}

#[test]
fn speech_like_bursts_do_not_click() {
    // Voiced bursts separated by silence: output should not contain spikes far above the input peak.
    let seg = ts::voice(SR, 0.4, |t| 140.0 + 20.0 * t);
    let mut x = vec![0f32; (SR * 3) as usize];
    for k in 0..3 {
        let at = k * SR as usize + 4000;
        x[at..at + seg.len()].copy_from_slice(&seg);
    }
    let (y, _) = run(&x, &base());
    assert_sane(&y);
    let peak_in = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    let peak_out = y.iter().fold(0f32, |m, v| m.max(v.abs()));
    println!("peak in={peak_in} out={peak_out}");
    assert!(peak_out < peak_in * 1.6, "output peak {peak_out} vs input {peak_in}");
}

#[test]
fn survives_more_than_2_to_the_31_samples_of_streaming() {
    // Simulate about 13 hours of continuous use without processing that many samples.
    let x = ts::voice(SR, 3.0, |_| 176.0);
    let mut e = LivePitchEngine::new(SR, &base());
    let mut out = vec![0f32; x.len()];
    let mut i = 0;
    while i + 128 <= x.len() {
        if i == SR as usize {
            e.shift_clock((1i64 << 31) + (1i64 << 20)); // multiple of the ring size
        }
        e.process(&x[i..i + 128], &mut out[i..i + 128]);
        i += 128;
    }
    assert_sane(&out);
    assert_eq!(0, e.starved_grains());
    let med = ts::median(&track_from(&out, 2 * SR));
    assert_close(0.0, ts::cents(med, midi_to_hz(kround(hz_to_midi(176.0)))), 15.0, "pitch after the clock jump");
}

#[test]
fn a_single_nan_or_inf_sample_does_not_silence_the_engine() {
    let mut x = ts::voice(SR, 3.0, |_| 176.0);
    x[SR as usize + 100] = f32::NAN;
    x[SR as usize + 5000] = f32::INFINITY;
    let (y, e) = run(&x, &base());
    assert_sane(&y);
    assert_eq!(0, e.starved_grains());
    let med = ts::median(&track_from(&y, 2 * SR));
    assert_close(0.0, ts::cents(med, midi_to_hz(kround(hz_to_midi(176.0)))), 15.0, "pitch after NaN");
}

#[test]
fn telemetry_reports_pitch_and_target() {
    // Not in the Kotlin suite: the read-outs the app shows.
    let x = ts::voice(SR, 1.0, |_| 150.0);
    let mut e = LivePitchEngine::new(SR, &base());
    assert_eq!(i32::MIN, e.target_midi());
    let mut out = vec![0f32; x.len()];
    e.process(&x, &mut out);
    assert_close(150.0, e.input_hz(), 3.0, "input");
    assert_close(midi_to_hz(50.0), e.output_hz(), 0.5, "output");
    assert_eq!(50, e.target_midi());
    assert!(e.latency_samples() > 0);
    e.reset_streams();
    e.configure(&base().with(P::Fmin, 100.0));
    let mut silent = vec![0f32; 4096];
    e.process(&vec![0f32; 4096], &mut silent);
    assert_eq!(0.0, e.input_hz());
    assert!(silent.iter().all(|v| *v == 0.0));
}

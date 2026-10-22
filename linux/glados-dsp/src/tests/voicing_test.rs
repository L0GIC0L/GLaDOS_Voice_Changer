//! Port of `VoicingTest.kt`, plus `liveEngineHearsVoiceThroughRumbleAndHiss` from its version at 5fcfef1.
//! Guards against the voicing detector going deaf on realistic recordings (rumble, hiss, a quiet mic).

use std::f64::consts::PI;

use super::test_signals::{self as ts, VoiceOpts};
use super::{assert_close, never};
use crate::analysis::Analyzer;
use crate::java_random::JavaRandom;
use crate::live_pitch_engine::LivePitchEngine;
use crate::model::{VoiceParams, P};
use crate::notes::NoteOps;
use crate::voicing::Voicing;

const SR: i32 = 16000;

/// Three vowel-like syllables with gaps, roughly one second each, at a speaking pitch.
fn phrase() -> Vec<f32> {
    let mut x = vec![0f32; (SR * 4) as usize];
    let hz = [118.0, 135.0, 110.0];
    let mut t = (0.2 * SR as f64) as usize;
    for (k, h) in hz.iter().enumerate() {
        let seg = ts::voice_with(
            SR,
            0.7,
            |s| h * (1.0 + 0.05 * (2.0 * PI * 3.0 * s).sin()),
            VoiceOpts { jitter: 0.006, seed: k as i64 + 7, ..Default::default() },
        );
        let ramp = (0.03 * SR as f64) as i32;
        for (i, v) in seg.iter().enumerate() {
            let env = 1f32.min(i as f32 / ramp as f32).min((seg.len() - i) as f32 / ramp as f32);
            x[t + i] = v * env;
        }
        t += (1.1 * SR as f64) as usize;
    }
    x
}

fn with_noise(x: &[f32], snr_db: f64) -> Vec<f32> {
    let mut r = JavaRandom::new(3);
    let sigma = ts::rms(x) / 10f64.powf(snr_db / 20.0);
    x.iter().map(|v| (*v as f64 + sigma * r.next_gaussian()) as f32).collect()
}

fn with_rumble(x: &[f32], amount: f64) -> Vec<f32> {
    let a = (ts::rms(x) * amount) as f32;
    let sr = SR as f64;
    x.iter()
        .enumerate()
        .map(|(i, v)| {
            let i = i as f64;
            (*v as f64 + a as f64 * ((2.0 * PI * 38.0 * i / sr).sin() + 0.6 * (2.0 * PI * 17.0 * i / sr).sin())) as f32
        })
        .collect()
}

fn voiced_frames(x: &[f32], p: &VoiceParams) -> usize {
    Analyzer::analyze(x, SR, p, None, &never, None).voiced.iter().filter(|v| **v).count()
}

#[test]
fn higher_sensitivity_never_finds_less_voice() {
    let noisy = with_noise(&phrase(), 6.0);
    let mut last: i64 = -1;
    for s in [0, 25, 50, 75, 100] {
        let n = voiced_frames(&noisy, &VoiceParams::defaults().with(P::Voicing, s as f32)) as i64;
        println!("voicing {s}%: {n} voiced frames");
        assert!(n >= last, "voiced frames fell from {last} to {n} when sensitivity rose to {s}%");
        last = n;
    }
}

#[test]
fn sensitivity_maps_to_threshold_in_the_right_direction() {
    assert!(Voicing::threshold(100.0) > Voicing::threshold(60.0));
    assert!(Voicing::threshold(60.0) > Voicing::threshold(0.0));
    assert_close(0.08, Voicing::threshold(0.0), 1e-9, "0%");
    assert_close(0.40, Voicing::threshold(100.0), 1e-9, "100%");
}

#[test]
fn default_settings_find_most_of_the_voice_despite_rumble() {
    let d = VoiceParams::defaults();
    let clean = voiced_frames(&phrase(), &d);
    for (label, sig) in [
        // Rumble at twice the voice level silences a detector that listens to the full spectrum.
        ("rumble 2x", with_rumble(&phrase(), 2.0)),
        ("hiss 6 dB", with_noise(&phrase(), 6.0)),
        ("rumble + hiss", with_noise(&with_rumble(&phrase(), 1.5), 12.0)),
    ] {
        let n = voiced_frames(&sig, &d);
        println!("{label}: {n} of {clean} clean-voiced frames");
        assert!(n as f64 >= clean as f64 * 0.8, "{label}: only {n} of {clean} voiced frames found");
    }
}

#[test]
fn quiet_recording_is_still_analysed() {
    let mut quiet = phrase();
    for v in quiet.iter_mut() {
        *v *= 0.004; // about -48 dBFS peak
    }
    let d = VoiceParams::defaults();
    let n = voiced_frames(&quiet, &d);
    assert!(n as f64 >= voiced_frames(&phrase(), &d) as f64 * 0.9, "quiet clip found only {n} voiced frames");
}

#[test]
fn notes_follow_syllables_on_noisy_audio() {
    let an = Analyzer::analyze(&with_noise(&with_rumble(&phrase(), 1.5), 12.0), SR, &VoiceParams::defaults(), None, &never, None);
    let notes = NoteOps::segment(&an, 80.0, 0.5).notes.len();
    println!("noisy phrase: {notes} notes for 3 syllables");
    assert!((3..=6).contains(&notes), "expected about 3 notes, got {notes}");
}

#[test]
fn live_engine_hears_voice_through_rumble_and_hiss() {
    let sig = with_noise(&with_rumble(&phrase(), 2.0), 12.0);
    let mut e = LivePitchEngine::new(SR, &VoiceParams::defaults());
    let mut bo = [0f32; 128];
    let mut voiced = 0;
    let mut frames = 0;
    let mut i = 0;
    while i + 128 <= sig.len() {
        e.process(&sig[i..i + 128], &mut bo);
        // Count only the stretches where a syllable is sounding.
        let t = i as f64 / SR as f64;
        let in_syllable = (0..3).any(|k| {
            let k = k as f64;
            t >= 0.2 + k * 1.1 + 0.15 && t <= 0.2 + k * 1.1 + 0.55
        });
        if in_syllable {
            frames += 1;
            if e.input_hz() > 0.0 {
                voiced += 1;
            }
        }
        i += 128;
    }
    println!("live: {voiced} of {frames} frames inside syllables were detected as voice");
    assert!(voiced as f64 >= frames as f64 * 0.75, "live engine detected only {voiced} of {frames} voiced frames");
}

//! Port of `FxChainTest.kt`.

use std::f64::consts::PI;

use super::assert_close;
use super::test_signals::{self as ts, VoiceOpts};
use crate::fx_chain::FxChain;
use crate::java_random::JavaRandom;
use crate::model::{Preset, VoiceParams, P, PRESETS};

const SR: i32 = 48000;

fn process_blocks(x: &[f32], p: &VoiceParams, block: usize) -> Vec<f32> {
    let mut fx = FxChain::new(SR);
    fx.configure(p);
    let mut y = x.to_vec();
    for chunk in y.chunks_mut(block) {
        fx.process(chunk);
    }
    y
}

fn process(x: &[f32], p: &VoiceParams) -> Vec<f32> {
    process_blocks(x, p, 256)
}

fn speech_like() -> Vec<f32> {
    ts::voice_with(SR, 1.5, |t| 140.0 + 40.0 * (t * 4.0).sin(), VoiceOpts { amp: 0.5, ..Default::default() })
}

fn assert_finite_and_bounded(y: &[f32], label: &str) {
    for v in y {
        assert!(v.is_finite(), "{label}: non-finite");
        assert!(v.abs() <= 1.0, "{label}: out of range {v}");
    }
}

fn natural() -> VoiceParams {
    Preset::find("Natural").unwrap().apply() // no effects
}

#[test]
fn every_preset_is_stable() {
    let x = speech_like();
    for preset in PRESETS.iter() {
        assert_finite_and_bounded(&process(&x, &preset.apply()), preset.name);
    }
}

#[test]
fn every_parameter_at_its_extremes_is_stable() {
    let x = speech_like();
    for p in P::ALL {
        let spec = p.spec();
        for v in [spec.min, spec.max] {
            let params = VoiceParams::defaults().with(p, v);
            assert_finite_and_bounded(&process(&x, &params), &format!("{}={v}", spec.id));
        }
    }
}

#[test]
fn all_effects_maxed_stays_bounded() {
    let maxed: Vec<(&str, f32)> = P::ALL
        .iter()
        .filter(|p| !p.spec().hidden)
        .map(|&p| {
            let v = match p {
                P::CombFb => 95.0,
                P::CrushBits => 3.0,
                _ => p.spec().max,
            };
            (p.spec().id, v)
        })
        .collect();
    let y = process(&speech_like(), &VoiceParams::from_pairs(maxed));
    assert_finite_and_bounded(&y, "maxed");
}

#[test]
fn limiter_holds_loud_input() {
    let loud: Vec<f32> = (0..SR).map(|i| (5.0 * (2.0 * PI * 220.0 * i as f64 / SR as f64).sin()) as f32).collect();
    let y = process(&loud, &natural());
    let peak = y.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak <= 0.98, "peak {peak}");
}

#[test]
fn effects_off_is_transparent_apart_from_eq() {
    let x = speech_like();
    let p = VoiceParams::defaults().with_all([
        ("ringMix", 0.0), ("combMix", 0.0), ("crushMix", 0.0), ("glitch", 0.0), ("echoMix", 0.0), ("reverbMix", 0.0),
        ("hpf", 20.0), ("lpf", 20000.0), ("lowDb", 0.0), ("midDb", 0.0), ("highDb", 0.0), ("outDb", 0.0),
    ]);
    let y = process(&x, &p);
    // The 20 Hz high-pass legitimately shifts the phase of a 140 Hz voice, so judge level and shape, not sample equality.
    let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
    for i in 4800..x.len() {
        let (a, b) = (x[i] as f64, y[i] as f64);
        xy += a * b;
        xx += a * a;
        yy += b * b;
    }
    let corr = xy / (xx * yy).sqrt();
    let level = (yy / xx).sqrt();
    println!("fx-off correlation={corr} level={level}");
    assert!(corr > 0.95, "correlation {corr}");
    assert_close(1.0, level, 0.05, "level");
}

#[test]
fn echo_repeats_at_the_requested_time() {
    let mut impulse = vec![0f32; SR as usize];
    impulse[1000] = 0.5;
    let p = VoiceParams::defaults().with_all([
        ("ringMix", 0.0), ("combMix", 0.0), ("crushMix", 0.0), ("glitch", 0.0), ("reverbMix", 0.0),
        ("echoMix", 60.0), ("echoMs", 250.0), ("echoFb", 0.0),
        ("hpf", 20.0), ("lpf", 20000.0), ("lowDb", 0.0), ("midDb", 0.0), ("highDb", 0.0),
    ]);
    let y = process(&impulse, &p);
    let mut best = 0;
    let mut best_v = 0f32;
    for (i, v) in y.iter().enumerate().skip(3000) {
        if v.abs() > best_v {
            best_v = v.abs();
            best = i;
        }
    }
    assert_close(1000.0 + 0.25 * SR as f64, best as f64, 30.0, "echo time");
    assert!(best_v > 0.05, "echo level {best_v}");
}

#[test]
fn reverb_adds_a_tail_at_sensible_level() {
    let mut rnd = JavaRandom::new(2);
    let mut burst = vec![0f32; (SR * 2) as usize];
    for v in burst.iter_mut().take(4800) {
        *v = (rnd.next_gaussian() * 0.2) as f32;
    }
    let p = VoiceParams::defaults().with_all([
        ("ringMix", 0.0), ("combMix", 0.0), ("crushMix", 0.0), ("glitch", 0.0), ("echoMix", 0.0),
        ("reverbMix", 50.0), ("reverbSize", 70.0),
        ("hpf", 20.0), ("lpf", 20000.0), ("lowDb", 0.0), ("midDb", 0.0), ("highDb", 0.0),
    ]);
    let y = process(&burst, &p);
    let tail = ts::rms_range(&y, 9600, 19200);
    let body = ts::rms_range(&burst, 0, 4800);
    println!("reverb tail rms={tail} burst rms={body} ratio={}", tail / body);
    assert!((0.02..=0.8).contains(&(tail / body)), "tail should be audible but below the source: {}", tail / body);
    assert!(ts::rms_range(&y, 76800, 86400) < tail, "tail should decay");
}

fn wobbling_tone(seconds: i32) -> Vec<f32> {
    (0..SR * seconds)
        .map(|i| {
            // kotlin.math.sin(Float) works in double
            let wobble = 1f32 + 0.5f32 * ((i as f32 * 0.0005f32) as f64).sin() as f32;
            (0.3 * (2.0 * PI * 200.0 * i as f64 / SR as f64).sin()) as f32 * wobble
        })
        .collect()
}

fn changed_blocks(a: &[f32], b: &[f32]) -> usize {
    let mut changed = 0;
    let mut i = 0;
    while i + 4800 < a.len() {
        let mut d = 0.0;
        for k in i..i + 4800 {
            d += (a[k] - b[k]).abs() as f64;
        }
        if d / 4800.0 > 0.01 {
            changed += 1;
        }
        i += 4800;
    }
    changed
}

#[test]
fn glitch_fires_at_roughly_the_requested_rate() {
    let x = wobbling_tone(20);
    let off = [("ringMix", 0.0), ("combMix", 0.0), ("crushMix", 0.0), ("reverbMix", 0.0)];
    let clean = process(&x, &VoiceParams::defaults().with_all(off).with(P::Glitch, 0.0));
    let glitchy = process(&x, &VoiceParams::defaults().with_all(off).with(P::Glitch, 60.0));
    let changed = changed_blocks(&glitchy, &clean);
    println!("glitched blocks {changed} of {}", x.len() / 4800);
    assert!(changed > 3, "glitch never fired");
    assert!(changed < x.len() / 4800, "glitch fired constantly");
}

#[test]
fn every_glitch_kind_and_rasp_change_voice_but_leave_silence_silent() {
    let x = wobbling_tone(10);
    let base = natural();
    let clean = process(&x, &base);
    for p in [
        P::Glitch, P::GlitchReverse, P::GlitchTape, P::GlitchPitch, P::GlitchDrop, P::GlitchStatic, P::Buzz, P::Rasp,
        P::Chorus, P::Phaser, P::Tremolo, P::Warble,
    ] {
        let params = base.with(p, 100.0);
        let y = process(&x, &params);
        let changed = changed_blocks(&y, &clean);
        let id = p.spec().id;
        println!("{id}: changed {changed} of {} blocks", x.len() / 4800);
        assert!(changed > 2, "{id} never changed anything");
        assert_finite_and_bounded(&y, id);
        let quiet = process(&vec![0f32; (SR * 2) as usize], &params);
        assert!(quiet.iter().all(|v| *v == 0.0), "{id} made sound out of silence");
    }
}

#[test]
fn crackle_adds_clicks_even_in_silence() {
    let p = natural().with(P::Crackle, 100.0);
    let y = process(&vec![0f32; (SR * 2) as usize], &p);
    let mut clicks = 0;
    for i in 1..y.len() {
        if y[i].abs() > 0.02 && y[i - 1].abs() <= 0.02 {
            clicks += 1;
        }
    }
    println!("crackle: {clicks} clicks in 2 s");
    assert!((20..=200).contains(&clicks), "crackle clicks {clicks}");
    assert_finite_and_bounded(&y, "crackle");
}

#[test]
fn tail_length_is_zero_without_time_effects() {
    let p = VoiceParams::defaults().with_all([("echoMix", 0.0), ("reverbMix", 0.0)]);
    assert_close(0.0, FxChain::tail_seconds(&p), 1e-9, "no tail");
    assert!(FxChain::tail_seconds(&p.with(P::ReverbMix, 30.0)) > 1.0);
}

#[test]
fn block_size_does_not_change_the_sound() {
    // Not in the Kotlin suite: the chain is a pure stream, so the block size must not change a single sample.
    let x = speech_like();
    let p = Preset::find("Natural").unwrap().apply().with_all([("echoMix", 40.0), ("chorusMix", 50.0), ("reverbMix", 30.0)]);
    let a = process_blocks(&x, &p, 256);
    let b = process_blocks(&x, &p, 77);
    assert_eq!(a, b);
}

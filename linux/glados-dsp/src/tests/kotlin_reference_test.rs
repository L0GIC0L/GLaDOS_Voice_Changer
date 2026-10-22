//! Not a Kotlin test: checks the port against numbers printed by the Kotlin engine (OpenJDK 21) for the same input,
//! through analysis, segmentation, correction, rendering with every global setting, the effect chain, noise reduction,
//! WAV encoding and the live engine. On x86-64 Linux with glibc every output is bit-identical to the Kotlin one; the
//! exact bits depend on the platform's maths library (sin, exp, pow), so they are printed, and only the sums are
//! asserted, to a tolerance.

use super::test_signals::{self as ts, VoiceOpts};
use super::{assert_close, never};
use crate::analysis::Analyzer;
use crate::denoiser::Denoiser;
use crate::fx_chain::FxChain;
use crate::java_random::JavaRandom;
use crate::model::{Preset, VoiceParams};
use crate::notes::NoteOps;
use crate::offline_renderer::{OfflineRenderer, RenderSettings};
use crate::speed_warp::SpeedWarp;
use crate::wav::{Audio, Wav};
use std::sync::Arc;

struct Summary {
    n: usize,
    sum: f64,
    abs: f64,
    bits: i64,
}

fn summary(x: &[f32]) -> Summary {
    let mut s = Summary { n: x.len(), sum: 0.0, abs: 0.0, bits: 0 };
    for v in x {
        s.sum += *v as f64;
        s.abs += (*v as f64).abs();
        s.bits = s.bits.wrapping_mul(31).wrapping_add(v.to_bits() as i32 as i64);
    }
    s
}

#[track_caller]
fn check(label: &str, x: &[f32], n: usize, sum: f64, abs: f64, bits: i64) {
    let s = summary(x);
    let same = if s.bits == bits { ", identical" } else { "" };
    println!("{label}: n={} sum={} abs={} bits={} (Kotlin bits {bits}{same})", s.n, s.sum, s.abs, s.bits);
    assert_eq!(n, s.n, "{label} length");
    assert_close(sum, s.sum, 1e-6 * abs.max(1.0), &format!("{label} sum"));
    assert_close(abs, s.abs, 1e-6 * abs.max(1.0), &format!("{label} abs"));
}

#[test]
fn matches_the_kotlin_engine() {
    let sr = 48000;
    let mut x = ts::voice_with(sr, 1.2, |t| 130.0 + 40.0 * (t * 5.0).sin(), VoiceOpts { seed: 3, ..Default::default() });
    let mut r = JavaRandom::new(11);
    for v in x.iter_mut() {
        *v += (r.next_gaussian() * 0.01) as f32;
    }
    let p = VoiceParams::defaults();
    let an = Analyzer::analyze(&x, sr, &p, None, &never, None);
    check("analysis f0", &an.f0, 241, 31501.215942382812, 31501.215942382812, 405748770800343283);
    check("analysis level", &an.level, 241, 26.912961773574352, 26.912961773574352, -3283863601870283843);
    check("analysis aperiodicity", &an.aperiodicity, 241, 3.714061351143755, 3.714061351143755, -7521806620074381726);
    assert_eq!(238, an.voiced.iter().filter(|v| **v).count());
    assert_eq!(155, an.marks.len());
    assert_eq!(4036006i64, an.marks.iter().map(|m| *m as i64).sum::<i64>());
    assert_eq!(0.14433995f32, an.loud_level());
    assert_close(48.41266459151194, an.median_midi(), 1e-9, "median");

    let mut edit = NoteOps::segment(&an, 80.0, 0.5);
    assert_eq!(1, edit.notes.len());
    assert_eq!((2, 239), (edit.notes[0].f0, edit.notes[0].f1));
    assert_close(47.48448043755971, edit.notes[0].center, 1e-9, "centre");
    assert_eq!(vec![(360, 360.0), (57480, 57480.0)], edit.anchors.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>());
    NoteOps::correct_to_settings(&mut edit.notes, &an, &p);
    assert_close(-0.4844804375597107, edit.notes[0].offset, 1e-9, "offset");
    assert_eq!(0.06666667f32, edit.notes[0].drift);
    assert_eq!(0.06666667f32, edit.notes[0].modulation);

    let shift = NoteOps::global_shift(&an, &p);
    let warp = Arc::new(SpeedWarp::build(x.len() as i32, sr, 1.3, 0.5, x.len() as i64));
    assert_eq!(34913, warp.rendered_length);
    let settings = RenderSettings {
        pitch_shift: shift + 2.0,
        formant_shift: NoteOps::formant_shift(&p, shift),
        breath: 0.8,
        vibrato_cents: 20.0,
        vibrato_hz: 5.5,
        dry: 0.1,
        harmony_mix: 0.3,
        harmony_st: 7.0,
        warp: Some(warp),
    };
    let y = OfflineRenderer::render(&x, &an, &edit, &settings, &never);
    check("render", &y, 34913, 46.045869940833654, 2425.3033045321063, -87533605752510656);

    for (name, sum, abs, bits, tail) in [
        ("Portal 1", -0.3463016347905068, 1431.975364776683, -1876346790251152508, 2.65),
        ("Announcer", 0.394567367016748, 2316.5189796485865, -4276535508383005400, 3.55),
        ("Natural", 2.0130968672639824, 2232.2628053560716, 1757294361798271818, 0.0),
    ] {
        let preset = Preset::find(name).unwrap().apply();
        let mut fx = FxChain::new(sr);
        fx.configure(&preset);
        let mut z = y.clone();
        fx.process(&mut z);
        check(&format!("fx {name}"), &z, 34913, sum, abs, bits);
        assert_close(tail, FxChain::tail_seconds(&preset), 1e-12, "tail");
    }
    let maxed = VoiceParams::defaults().with_all([
        ("glitch", 80.0), ("glitchReverse", 50.0), ("glitchTape", 50.0), ("glitchPitch", 50.0), ("glitchDrop", 50.0),
        ("glitchStatic", 50.0), ("crackle", 40.0), ("buzz", 30.0), ("rasp", 40.0), ("chorusMix", 40.0),
        ("phaserMix", 40.0), ("tremolo", 30.0), ("warble", 50.0), ("echoMix", 30.0), ("drive", 30.0),
    ]);
    let mut fx = FxChain::new(sr);
    fx.configure(&maxed);
    let mut z = y.clone();
    fx.process(&mut z);
    check("fx every effect", &z, 34913, 2.9875225216979615, 1464.6737344209293, -2404949446628051737);

    let d = Denoiser::process(&x, sr, 0.6, &never);
    check("denoise", &d, 57600, 1.8958931684110212, 2623.6562250652196, 1341642765891872887);

    let wav = Wav::encode(&Audio { samples: y, sample_rate: sr });
    let mut h: i64 = 0;
    for b in &wav {
        h = h.wrapping_mul(31).wrapping_add(*b as i8 as i64);
    }
    println!("wav hash {h} (Kotlin 2771409141686084318)");
}

/// The live engine against the Kotlin one as it was at 5fcfef1, with every setting it reads given explicitly.
#[test]
fn live_engine_matches_the_kotlin_engine() {
    let sr = 48000;
    let v = ts::voice_with(
        sr,
        1.5,
        |t| 120.0 + 60.0 * t + 10.0 * (t * 20.0).sin(),
        VoiceOpts { seed: 5, ..Default::default() },
    );
    let mut x = vec![0f32; (sr * 2) as usize];
    x[6000..6000 + v.len()].copy_from_slice(&v);
    let mut r = JavaRandom::new(13);
    for s in x.iter_mut() {
        *s += (r.next_gaussian() * 0.003) as f32;
    }
    let base = VoiceParams::defaults().with_all([
        ("key", 2.0), ("scale", 1.0), ("customMask", 4095.0), ("retuneMs", 30.0), ("strength", 90.0),
        ("holdCents", 25.0), ("shift", 1.0), ("autoReg", 1.0), ("regHz", 200.0), ("monotone", 0.0), ("monoNote", 57.0),
        ("vibDepth", 20.0), ("vibRate", 5.5), ("refA4", 440.0), ("formant", 1.0), ("formantFollow", 30.0),
        ("fmin", 80.0), ("fmax", 600.0), ("gateDb", -60.0), ("voicing", 60.0), ("breath", 80.0), ("dry", 10.0),
    ]);
    let mono = base.with_all([("monotone", 1.0), ("autoReg", 0.0), ("monoNote", 52.0), ("vibDepth", 0.0)]);
    for (label, p, block, sum, abs, bits, tele) in [
        ("auto", base, 256, 84.12582618719117, 5309.148506174172, 8379836097297459032, 140946.6805715671),
        ("auto", base, 77, 84.34924365833754, 5309.179832456412, -5928688623110647282, 469196.8948724941),
        ("mono", mono, 256, -209.95656222778425, 5750.595953265423, 8752947497486187727, 111955.07061892762),
        ("mono", mono, 77, -209.9418307736415, 5750.602820963988, -4213545682739289286, 372669.8065759704),
    ] {
        let mut e = crate::live_pitch_engine::LivePitchEngine::new(sr, &p);
        let mut out = vec![0f32; x.len()];
        let mut telemetry = 0.0;
        for (inp, o) in x.chunks(block).zip(out.chunks_mut(block)) {
            e.process(inp, o);
            telemetry += e.input_hz() + e.output_hz() + e.target_midi().max(0) as f64;
        }
        check(&format!("live {label} block {block}"), &out, 96000, sum, abs, bits);
        assert_close(tele, telemetry, 1e-6 * tele, "telemetry");
        assert_eq!(2164, e.latency_samples());
        assert_eq!(0, e.starved_grains());
    }
}

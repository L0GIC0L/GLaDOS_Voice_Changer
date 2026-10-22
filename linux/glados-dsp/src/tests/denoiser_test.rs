//! Port of `DenoiserTest.kt`.

use std::f64::consts::PI;
use std::time::Instant;

use super::test_signals::{self as ts, VoiceOpts};
use super::{assert_close, never};
use crate::denoiser::{Denoiser, Fft};
use crate::java_random::JavaRandom;

const SR: i32 = 44100;

/// Three vowel "syllables" with pauses between them and room tone before and after.
fn phrase() -> Vec<f32> {
    let mut out = vec![0f32; (SR as f64 * 2.4) as usize];
    let mut t = (0.3 * SR as f64) as usize;
    for (k, hz) in [140.0, 180.0, 120.0].iter().enumerate() {
        let seg = ts::voice_with(SR, 0.4, |_| *hz, VoiceOpts { seed: k as i64 + 1, ..Default::default() });
        let ramp = (0.02 * SR as f64) as i32;
        for (i, v) in seg.iter().enumerate() {
            out[t + i] = v * 1f32.min(i as f32 / ramp as f32).min((seg.len() - i) as f32 / ramp as f32);
        }
        t += seg.len() + (0.25 * SR as f64) as usize;
    }
    out
}

fn noise(n: usize, rms: f64, seed: i64) -> Vec<f32> {
    let mut r = JavaRandom::new(seed);
    (0..n).map(|_| (r.next_gaussian() * rms) as f32).collect()
}

fn plus(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

fn snr(clean: &[f32], test: &[f32]) -> f64 {
    let mut s = 0.0;
    let mut e = 0.0;
    for (c, t) in clean.iter().zip(test) {
        s += *c as f64 * *c as f64;
        let d = *t as f64 - *c as f64;
        e += d * d;
    }
    10.0 * (s / e).log10()
}

/// RMS over the pauses, where the clean phrase is silent.
fn pause_rms(clean: &[f32], x: &[f32]) -> f64 {
    let mut s = 0.0;
    let mut n = 0;
    for (c, v) in clean.iter().zip(x) {
        if *c == 0.0 {
            s += *v as f64 * *v as f64;
            n += 1;
        }
    }
    (s / n as f64).sqrt()
}

fn denoise(x: &[f32], sr: i32, amount: f32) -> Vec<f32> {
    Denoiser::process(x, sr, amount, &never)
}

#[test]
fn fft_round_trips_and_finds_a_tone() {
    let n = 512;
    let fft = Fft::new(n as i32);
    let mut r = JavaRandom::new(3);
    let mut re: Vec<f64> = (0..n).map(|_| r.next_gaussian()).collect();
    let mut im = vec![0.0; n];
    let orig = re.clone();
    fft.transform(&mut re, &mut im, false);
    fft.transform(&mut re, &mut im, true);
    for i in 0..n {
        assert_close(orig[i], re[i], 1e-9, "re");
        assert_close(0.0, im[i], 1e-9, "im");
    }
    let mut tone: Vec<f64> = (0..n).map(|i| (2.0 * PI * 5.0 * i as f64 / n as f64).cos()).collect();
    let mut ti = vec![0.0; n];
    fft.transform(&mut tone, &mut ti, false);
    assert_close(n as f64 / 2.0, tone[5], 1e-6, "bin 5");
    assert_close(n as f64 / 2.0, tone[n - 5], 1e-6, "bin n-5");
    assert_close(0.0, tone[6], 1e-6, "bin 6");
}

#[test]
fn zero_amount_and_silence_are_left_alone() {
    // Kotlin returns the very same array; the Rust API returns an unchanged copy.
    let x = phrase();
    assert_eq!(x, denoise(&x, SR, 0.0));
    let silent = vec![0f32; SR as usize];
    assert_eq!(silent, denoise(&silent, SR, 1.0));
    assert!(denoise(&[], SR, 1.0).is_empty());
}

#[test]
fn clean_speech_is_left_almost_untouched() {
    let clean = phrase();
    let x = plus(&clean, &noise(clean.len(), 1e-5, 1)); // a -100 dB noise floor, like a good converter
    for amount in [0.5f32, 1.0] {
        let y = denoise(&x, SR, amount);
        assert_eq!(x.len(), y.len());
        let s = snr(&x, &y);
        println!("clean speech at {amount}: output differs from input by {s:.1} dB SNR");
        assert!(s > 35.0, "clean speech changed too much: {s} dB");
    }
}

#[test]
fn white_noise_is_removed() {
    let clean = phrase();
    let mut speech = 0.0;
    for v in &clean {
        speech += *v as f64 * *v as f64;
    }
    let sigma = (speech / clean.len() as f64).sqrt() / 10f64.powf(5.0 / 20.0); // 5 dB SNR
    let noisy = plus(&clean, &noise(clean.len(), sigma, 2));
    let before = snr(&clean, &noisy);
    for amount in [0.25f32, 0.5, 1.0] {
        let y = denoise(&noisy, SR, amount);
        for v in &y {
            assert!(v.is_finite());
        }
        let gain = snr(&clean, &y) - before;
        let pause_drop = 20.0 * (pause_rms(&clean, &noisy) / pause_rms(&clean, &y)).log10();
        println!("white noise at 5 dB, amount {amount}: SNR +{gain:.1} dB, pauses down {pause_drop:.1} dB");
        assert!(gain > 3.0, "SNR should improve at {amount}: {gain}");
        assert!(pause_drop > 6.0 * amount as f64, "pauses should get quieter at {amount}: {pause_drop}");
    }
}

#[test]
fn a_recorder_warm_up_of_digital_silence_does_not_hide_the_noise() {
    let clean = phrase();
    let mut noisy = plus(&clean, &noise(clean.len(), 0.02, 4));
    for v in noisy.iter_mut().take((0.1 * SR as f64) as usize) {
        *v = 0.0;
    }
    let y = denoise(&noisy, SR, 0.5);
    let drop = 20.0 * (pause_rms(&clean, &noisy) / pause_rms(&clean, &y)).log10();
    assert!(drop > 6.0, "pauses down only {drop} dB");
}

#[test]
fn noise_that_slowly_swells_and_fades_is_removed_throughout() {
    // The phrase twice over, with noise that swells slowly from 6 dB below its average level to 6 dB above and back,
    // as a phone's automatic gain or a drifting fan would.
    let one = phrase();
    let clean = [one.as_slice(), one.as_slice()].concat();
    let mut r = JavaRandom::new(6);
    let len = clean.len();
    let noisy: Vec<f32> = (0..len)
        .map(|i| {
            let level = 10f64.powf(-0.3 * (2.0 * PI * i as f64 / len as f64).cos());
            clean[i] + (r.next_gaussian() * 0.01 * level) as f32
        })
        .collect();
    let y = denoise(&noisy, SR, 0.5);
    let pause_drop = |from: usize, to: usize| {
        let mut a = 0.0;
        let mut b = 0.0;
        for i in from..to {
            if clean[i] == 0.0 {
                a += noisy[i] as f64 * noisy[i] as f64;
                b += y[i] as f64 * y[i] as f64;
            }
        }
        10.0 * (a / b).log10()
    };
    let quarter = len / 4;
    let quiet = pause_drop(0, quarter);
    let loud = pause_drop(quarter, 3 * quarter);
    println!("drifting noise: pauses down {quiet:.1} dB where it is quiet, {loud:.1} dB where it is loud");
    assert!(quiet > 10.0, "quiet half only {quiet} dB");
    assert!(loud > 10.0, "loud half only {loud} dB");
}

#[test]
fn a_long_recording_is_quick() {
    let sr48 = 48000;
    let mut r = JavaRandom::new(7);
    let x: Vec<f32> =
        (0..sr48 * 120).map(|i| (0.1 * (i as f64 * 0.05).sin() + r.next_gaussian() * 0.01) as f32).collect();
    denoise(&x[..(sr48 * 5) as usize], sr48, 0.5); // warm up, as the Kotlin test warms up the JIT
    let t0 = Instant::now();
    let y = denoise(&x, sr48, 0.5);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("120 s at 48 kHz: {ms:.0} ms");
    assert_eq!(x.len(), y.len());
    assert!(ms < 20_000.0, "took {ms} ms");
}

#[test]
fn very_short_clips_work() {
    let x = noise(100, 0.1, 5);
    let y = denoise(&x, 8000, 0.7);
    assert_eq!(100, y.len());
    for v in &y {
        assert!(!v.is_nan());
    }
}

#[test]
fn cancelling_stops_early() {
    // Not in the Kotlin suite: a cancelled run returns promptly with a result of the right length.
    let x = noise(SR as usize * 10, 0.05, 8);
    let y = Denoiser::process(&x, SR, 0.5, &|| true);
    assert_eq!(x.len(), y.len());
}

//! The parts of `ModelTest.kt` not already covered by the tests in `model/` and `scales.rs`: the WAV reader and writer,
//! and `tunerSnapsHoldsAndSmooths` from its version at 5fcfef1.

use super::assert_close;
use crate::tuner::Tuner;
use crate::wav::{Audio, Wav};

#[test]
fn tuner_snaps_holds_and_smooths() {
    // (hysteresis only engages after the first few settled frames of a phrase)
    let mut t = Tuner::new(0.0029);
    t.retune_ms = 0.0;
    assert_close(60.0, t.update(60.3, true), 1e-9, "snap");
    for _ in 0..5 {
        t.update(60.3, true); // let the pitch settle: hysteresis starts after the first few frames
    }
    // Hysteresis keeps the note until the pitch is clearly nearer the next one.
    t.hold_semitones = 0.25;
    assert_close(60.0, t.update(60.6, true), 1e-9, "hold");
    assert_close(61.0, t.update(60.9, true), 1e-9, "switch");
    // A wrong first guess at the start of a phrase is not held.
    t.reset();
    t.strength = 1.0;
    assert_close(60.0, t.update(60.4, true), 1e-9, "first guess");
    assert_close(61.0, t.update(60.55, true), 1e-9, "first guess not held");
    // Strength blends toward the target.
    t.reset();
    t.strength = 0.5;
    assert_close(60.15, t.update(60.3, true), 1e-9, "strength");
    // Slow retune moves gradually after the first voiced frame.
    t.reset();
    t.strength = 1.0;
    t.retune_ms = 200.0;
    t.update(60.4, true);
    let y = t.update(60.4 + 0.5, true); // pitch jumps up: correction needs time to follow
    assert!((61.0 - y).abs() > 1e-6, "slow retune jumped straight to the note: {y}");
}

#[test]
fn wav_round_trip() {
    let x: Vec<f32> = (0..1000).map(|i| (0.5 * (i as f64 * 0.1).sin()) as f32).collect();
    let back = Wav::read(&Wav::encode(&Audio { samples: x.clone(), sample_rate: 22050 })).unwrap();
    assert_eq!(22050, back.sample_rate);
    assert_eq!(x.len(), back.samples.len());
    for (a, b) in x.iter().zip(&back.samples) {
        assert_close(*a as f64, *b as f64, 1e-3, "sample");
    }
    assert_close(1000.0 / 22050.0, back.seconds(), 1e-12, "seconds");
}

#[test]
fn wav_with_zero_data_length_still_reads() {
    let x = vec![0.25f32; 500];
    let mut bytes = Wav::encode(&Audio { samples: x, sample_rate: 22050 });
    // Zero the RIFF and data sizes as a streaming writer would.
    bytes[4..8].fill(0);
    bytes[40..44].fill(0);
    assert_eq!(500, Wav::read(&bytes).unwrap().samples.len());
}

#[test]
fn malformed_wav_headers_are_rejected_cleanly() {
    let good = Wav::encode(&Audio { samples: vec![0.1f32; 100], sample_rate: 22050 });
    // Zero channels, which would divide by zero.
    let mut no_channels = good.clone();
    no_channels[22] = 0;
    no_channels[23] = 0;
    assert!(Wav::read(&no_channels).is_err(), "zero channels accepted");
    // A chunk length near 2^32 must not overflow the bounds check.
    let mut huge = good.clone();
    huge[40..44].fill(0xFF);
    assert_eq!(100, Wav::read(&huge).unwrap().samples.len());
    // Truncated file.
    assert!(Wav::read(&good[..20]).is_err(), "truncated file accepted");
    // Not a WAV file at all, and an empty one.
    assert!(Wav::read(b"definitely not a wav file").is_err());
    assert!(Wav::read(&[]).is_err());
}

#[test]
fn wav_reads_other_formats() {
    // 24-bit stereo PCM and mono float32, built by hand.
    fn file(format: u16, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&format.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&(8000 * (bits / 8 * channels) as u32).to_le_bytes());
        b.extend_from_slice(&(bits / 8 * channels).to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data);
        b
    }
    // Left +0.5, right -0.25 in 24-bit: the mono mix is 0.125.
    let l = (0.5 * 8388608.0) as i32;
    let r = (-0.25 * 8388608.0) as i32;
    let mut data = Vec::new();
    data.extend_from_slice(&l.to_le_bytes()[..3]);
    data.extend_from_slice(&r.to_le_bytes()[..3]);
    let a = Wav::read(&file(1, 2, 24, &data)).unwrap();
    assert_eq!(8000, a.sample_rate);
    assert_eq!(vec![0.125f32], a.samples);
    let f = Wav::read(&file(3, 1, 32, &[0.75f32.to_le_bytes(), (-1.5f32).to_le_bytes()].concat())).unwrap();
    assert_eq!(vec![0.75f32, -1.5], f.samples);
    let b8 = Wav::read(&file(1, 1, 8, &[0, 128, 255])).unwrap();
    assert_eq!(vec![-1.0f32, 0.0, 127.0 / 128.0], b8.samples);
}

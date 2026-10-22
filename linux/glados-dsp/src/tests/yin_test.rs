//! Port of `YinTest.kt`.

use super::assert_close;
use super::test_signals as ts;
use crate::java_random::JavaRandom;
use crate::yin::Yin;

#[test]
fn detects_steady_voice_pitch_across_range() {
    for sr in [48000, 44100, 22050] {
        for hz in [82.0, 110.0, 150.0, 220.0, 330.0, 523.0] {
            let x = ts::voice(sr, 0.6, |_| hz);
            let track = ts::pitch_track(&x, sr, 0, x.len() as i32, 256);
            let med = ts::median(&track);
            assert_close(0.0, ts::cents(med, hz), 6.0, &format!("sr={sr} hz={hz}"));
            let voiced = track.iter().filter(|v| **v > 0.0).count();
            assert!(voiced + 1 >= track.len(), "sr={sr} hz={hz} voiced {voiced}/{}", track.len());
        }
    }
}

#[test]
fn noise_is_not_voiced() {
    let mut rnd = JavaRandom::new(3);
    let x: Vec<f32> = (0..48000).map(|_| (rnd.next_gaussian() * 0.1) as f32).collect();
    let track = ts::pitch_track(&x, 48000, 0, x.len() as i32, 512);
    let voiced = track.iter().filter(|v| **v > 0.0).count();
    assert!(voiced <= track.len() / 10, "false positives {voiced}/{}", track.len());
}

#[test]
fn silence_is_not_voiced() {
    let x = vec![0f32; 24000];
    let mut yin = Yin::new(48000, 80.0, 700.0);
    assert!(!yin.detect(&x, -1, 0, 0.15));
}

#[test]
fn tracks_glide() {
    let x = ts::voice(48000, 1.0, |t| 120.0 + 80.0 * t);
    let track = ts::pitch_track(&x, 48000, 0, x.len() as i32, 480);
    let yin = Yin::new(48000, 60.0, 900.0);
    for (i, f) in track.iter().enumerate() {
        let centre = (i as i32 * 480 + yin.window_length / 2) as f64 / 48000.0;
        let expected = 120.0 + 80.0 * centre;
        assert_close(0.0, ts::cents(*f, expected), 40.0, &format!("frame {i}"));
    }
}

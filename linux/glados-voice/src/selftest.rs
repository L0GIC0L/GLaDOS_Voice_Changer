//! `glados-voice --selftest-live SECONDS [PRESET]`: runs live mode on the default devices without the window and
//! reports what it heard, for checking the real audio path on a headless machine (for example PulseAudio null sinks).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use glados_dsp::model::{Preset, VoiceParams};

use crate::audio::live::LiveController;

pub fn live(seconds: f64, preset: Option<&str>) -> i32 {
    let params = preset.and_then(Preset::find).map(|p| p.apply()).unwrap_or_else(VoiceParams::defaults);
    let mut live = LiveController::new(params);
    if let Err(e) = live.start(None, None) {
        eprintln!("selftest: could not start live mode: {e}");
        return 2;
    }
    println!("selftest: live at {} Hz, about {} ms added delay", live.sample_rate(), live.latency_ms());
    let start = Instant::now();
    let (mut ticks, mut voiced_ticks, mut max_in, mut max_out) = (0, 0, 0.0f32, 0.0f32);
    while start.elapsed().as_secs_f64() < seconds {
        std::thread::sleep(Duration::from_millis(50));
        let t = live.telemetry();
        ticks += 1;
        if t.voice_detected.load(Ordering::Relaxed) {
            voiced_ticks += 1;
        }
        max_in = max_in.max(t.input_level.load());
        max_out = max_out.max(t.output_level.load());
        if let Some(e) = live.take_error() {
            eprintln!("selftest: audio error: {e}");
            return 3;
        }
    }
    let t = live.telemetry();
    println!(
        "selftest: voice detected in {voiced_ticks} of {ticks} checks; peak in {max_in:.3}, peak out {max_out:.3}; dropouts {}; delay now {} ms",
        t.dropouts.load(Ordering::Relaxed),
        live.latency_ms()
    );
    live.stop();
    0
}

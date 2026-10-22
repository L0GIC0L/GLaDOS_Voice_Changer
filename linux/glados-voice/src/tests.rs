//! End-to-end checks of the app model and pages, run against a real GTK main loop. They need a display (Xvfb is
//! fine) and skip themselves without one. GTK must stay on one thread, so everything runs in a single test.

use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glados_dsp::model::{Preset, P};
use glados_dsp::{Audio, NoteOps};
use gtk4::glib;

use crate::app::App;
use crate::tts;

fn wait_until(what: &str, secs: f64, mut f: impl FnMut() -> bool) {
    let ctx = glib::MainContext::default();
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed().as_secs_f64() < secs, "timed out waiting for {what}");
        while ctx.iteration(false) {}
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Four sung vowels at different pitches with gaps between them, like the Android UI tests' clip.
fn speech_clip() -> Audio {
    let sr = 22050;
    let hz = [130.0, 170.0, 110.0, 150.0];
    let mut x = vec![0f32; (sr as f64 * (0.2 + hz.len() as f64 * 0.5)) as usize];
    let mut t0 = (0.1 * sr as f64) as usize;
    for f in hz {
        let n = (0.4 * sr as f64) as usize;
        let mut phase = 0.0f64;
        for i in 0..n {
            phase += 2.0 * PI * f / sr as f64;
            let mut s = 0.0;
            for k in 1..=14 {
                let kf = k as f64;
                s += (kf * phase).sin() / (kf * kf) * (1.0 + 2.0 / (1.0 + ((kf * f - 900.0) / 300.0).powi(2)));
            }
            let env = 1f32.min(i as f32 / 300.0).min((n - i) as f32 / 300.0);
            x[t0 + i] = (0.3 * s) as f32 * env;
        }
        t0 += (0.5 * sr as f64) as usize;
    }
    Audio { samples: x, sample_rate: sr }
}

fn load(app: &Rc<App>, auto_correct: bool) {
    app.load_audio(Arc::new(speech_clip()), "Test clip".into(), auto_correct, false, true);
    wait_until("the clip to load", 60.0, || app.clip().is_some() && app.busy().is_none());
}

#[test]
fn app_end_to_end() {
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("skipped: no display");
        return;
    }
    let dir = std::env::temp_dir().join(format!("glados-voice-test-{}", std::process::id()));
    std::env::set_var("GLADOS_VOICE_CONFIG_DIR", &dir);
    gtk4::init().expect("GTK");
    adw::init().expect("libadwaita");

    let app = App::new();

    // Every page builds.
    let window = adw::ApplicationWindow::builder().build();
    let _ = crate::ui::live_page::build(&app);
    let _ = crate::ui::speak_page::build(&app, &window, || {});
    let _ = crate::ui::editor_page::build(&app, &window, || {});
    let _ = crate::ui::voice_page::build(&app);

    // Loading finds the four notes.
    load(&app, false);
    let notes = app.with_edit(|e| e.notes.len()).unwrap();
    assert!((4..=6).contains(&notes), "expected about 4 notes, got {notes}");

    // Split and undo, and a refused split leaves the history alone.
    let (f0, f1) = app.with_edit(|e| (e.notes[1].f0, e.notes[1].f1)).unwrap();
    app.split_note(1, (f0 + f1) / 2);
    assert_eq!(notes + 1, app.with_edit(|e| e.notes.len()).unwrap());
    app.undo();
    assert_eq!(notes, app.with_edit(|e| e.notes.len()).unwrap());
    assert!(app.can_redo());
    app.split_note(0, app.with_edit(|e| e.notes[0].f0 + 1).unwrap());
    assert!(app.can_redo(), "a refused split wiped the redo history");
    app.messages.borrow_mut().clear();

    // A new preset re-tunes a clip until it is edited, and keeps the recording settings.
    load(&app, true);
    app.set_param(P::Denoise, 85.0);
    app.apply_preset("Monotone");
    let p = app.params();
    assert_eq!(85.0, p.get(P::Denoise), "presets keep the noise reduction setting");
    assert_eq!(Some("Monotone".to_string()), app.preset_name());
    let cs = app.clip().unwrap();
    let shift = NoteOps::global_shift(&cs.analysis, &p);
    let held = NoteOps::monotone_note(&cs.analysis, &p, shift, NoteOps::scale_mask(&p));
    app.with_edit(|e| {
        for n in &e.notes {
            assert!((n.center + n.offset + shift - held as f64).abs() < 1e-6, "Monotone holds one note");
        }
    });
    app.reset_notes(); // a hand edit: from now on presets leave the notes alone
    app.apply_preset("Portal 1");
    app.with_edit(|e| assert!(e.notes.iter().all(|n| n.offset == 0.0)));

    // At double speed the clip plays twice as fast, and the playhead still follows the timeline.
    let before = app.player.duration_seconds();
    app.set_param(P::Tempo, 200.0);
    wait_until("the faster render", 30.0, || app.player.duration_seconds() < before * 0.6);
    app.seek_timeline(1.0);
    assert!((app.playhead_seconds() - 1.0).abs() < 0.01);
    assert!((app.player.position_seconds() - 0.5).abs() < 0.02, "the clip plays twice as fast");
    app.set_param(P::Tempo, 100.0);

    // Simple-mode sliders move several settings at once and the preset name becomes "custom".
    app.apply_preset("Portal 1");
    let robotic = glados_dsp::model::MACROS.iter().find(|m| m.id == "robotic").unwrap();
    app.set_macro(robotic, 100.0);
    assert!(app.params().get(P::CombMix) > 90.0);
    assert_eq!(None, app.preset_name());

    // User presets cannot shadow built-in ones.
    assert!(!app.save_user_preset("portal 1"));
    assert!(app.save_user_preset("Mine"));
    assert!(app.user_preset_names().contains(&"Mine".to_string()));
    assert!(Preset::find("Mine").is_none());

    // Speech, if eSpeak NG is installed: generated speech is loaded without noise reduction.
    if tts::espeak_available() {
        let req = tts::Request {
            text: "Hello, and again, welcome.".into(),
            engine: tts::Engine::Espeak,
            voice: "en-us".into(),
            female: true,
            piper_model: String::new(),
            rate: 1.0,
            pitch: 1.0,
        };
        app.speak(req, true);
        wait_until("speech", 60.0, || app.clip().is_some_and(|c| c.title == "Speech") && app.busy().is_none());
        let cs = app.clip().unwrap();
        assert!(!cs.noisy);
        assert!(app.with_edit(|e| e.notes.len()).unwrap() >= 3, "speech should have several syllables");
    }

    // Settings were written.
    app.save_now();
    assert!(dir.join("settings.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

//! Live voice: microphone in, GLaDOS out, with a scrolling pitch view.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use adw::prelude::*;
use glados_dsp::model::P;
use glados_dsp::scales::{self, pitch};
use gtk4::{glib, Orientation};

use super::style::{self, palette, Rgba};
use super::widgets::{level_bar, page_box, param_control, preset_card, scrolled_page};
use crate::app::{App, Change};
use crate::audio::devices::{self, DeviceInfo};

pub fn build(app: &Rc<App>) -> gtk4::Widget {
    let page = page_box();

    let go = gtk4::Button::builder().label("Go live").css_classes(["suggested-action", "pill", "big-button"]).halign(gtk4::Align::Center).build();
    go.set_widget_name("go-live");
    page.append(&go);
    page.append(
        &gtk4::Label::builder()
            .label("Use headphones: speakers would feed the processed voice straight back into the microphone.")
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .css_classes(["dim-label", "caption"])
            .build(),
    );

    // Meters and status.
    let status = adw::PreferencesGroup::new();
    let meters = gtk4::Grid::builder().row_spacing(8).column_spacing(12).build();
    let in_bar = level_bar();
    let out_bar = level_bar();
    meters.attach(&gtk4::Label::builder().label("Microphone").xalign(0.0).build(), 0, 0, 1, 1);
    meters.attach(&in_bar, 1, 0, 1, 1);
    meters.attach(&gtk4::Label::builder().label("Output").xalign(0.0).build(), 0, 1, 1, 1);
    meters.attach(&out_bar, 1, 1, 1, 1);
    let detected = gtk4::Label::builder().xalign(0.0).wrap(true).build();
    let info = gtk4::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label", "caption"]).build();
    let box_ = gtk4::Box::new(Orientation::Vertical, 8);
    box_.append(&meters);
    box_.append(&detected);
    box_.append(&info);
    status.add(&super::widgets::plain_row(&box_));
    page.append(&status);

    // Pitch trace.
    let trace = gtk4::DrawingArea::builder().content_height(200).hexpand(true).build();
    let frame = gtk4::Frame::builder().child(&trace).build();
    page.append(&frame);
    page.append(
        &gtk4::Label::builder()
            .label("Grey dots are your voice; the bright line is the tuned pitch. Shaded rows are the notes of the scale.")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build(),
    );
    {
        let a = app.clone();
        trace.set_draw_func(move |area, cr, w, h| draw_trace(&a, area, cr, w as f64, h as f64));
    }

    // Devices and switches.
    let audio = adw::PreferencesGroup::builder().title("Sound devices").build();
    let inputs = Rc::new(RefCell::new(Vec::<DeviceInfo>::new()));
    let outputs = Rc::new(RefCell::new(Vec::<DeviceInfo>::new()));
    let in_row = adw::ComboRow::builder().title("Microphone").build();
    let out_row = adw::ComboRow::builder().title("Output").build();
    let rescan = gtk4::Button::builder().icon_name("view-refresh-symbolic").valign(gtk4::Align::Center).tooltip_text("Look for devices again").css_classes(["flat"]).build();
    out_row.add_suffix(&rescan);
    let fill = {
        let a = app.clone();
        let in_row = in_row.clone();
        let out_row = out_row.clone();
        let inputs = inputs.clone();
        let outputs = outputs.clone();
        move || {
            let s = a.settings.borrow().clone();
            *inputs.borrow_mut() = devices::inputs();
            *outputs.borrow_mut() = devices::outputs();
            let model = |list: &[DeviceInfo]| {
                let mut names = vec!["System default".to_string()];
                names.extend(list.iter().map(|d| d.name.clone()));
                gtk4::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>())
            };
            in_row.set_model(Some(&model(&inputs.borrow())));
            out_row.set_model(Some(&model(&outputs.borrow())));
            let pick = |list: &[DeviceInfo], id: &Option<String>| {
                id.as_ref().and_then(|id| list.iter().position(|d| &d.id == id)).map(|i| i as u32 + 1).unwrap_or(0)
            };
            in_row.set_selected(pick(&inputs.borrow(), &s.input_device));
            out_row.set_selected(pick(&outputs.borrow(), &s.output_device));
        }
    };
    fill();
    let chosen = {
        let a = app.clone();
        let in_row = in_row.clone();
        let out_row = out_row.clone();
        let inputs = inputs.clone();
        let outputs = outputs.clone();
        move || {
            let pick = |list: &[DeviceInfo], i: u32| if i == 0 { None } else { list.get(i as usize - 1).map(|d| d.id.clone()) };
            let input = pick(&inputs.borrow(), in_row.selected());
            let output = pick(&outputs.borrow(), out_row.selected());
            let s = a.settings.borrow().clone();
            if s.input_device != input || s.output_device != output {
                a.set_devices(input, output);
            }
        }
    };
    {
        let chosen = chosen.clone();
        in_row.connect_selected_notify(move |_| chosen());
    }
    {
        let chosen = chosen.clone();
        out_row.connect_selected_notify(move |_| chosen());
    }
    rescan.connect_clicked(move |_| fill());
    audio.add(&in_row);
    audio.add(&out_row);
    let bypass = adw::SwitchRow::builder().title("Bypass").subtitle("Hear your own voice, unprocessed.").build();
    let mute = adw::SwitchRow::builder().title("Mute output").build();
    {
        let a = app.clone();
        bypass.connect_active_notify(move |r| a.live.borrow().set_bypass(r.is_active()));
    }
    {
        let a = app.clone();
        mute.connect_active_notify(move |r| a.live.borrow().set_muted(r.is_active()));
    }
    audio.add(&bypass);
    audio.add(&mute);
    page.append(&audio);

    let presets = adw::PreferencesGroup::new();
    presets.add(&preset_card(app));
    page.append(&presets);

    let quick = adw::PreferencesGroup::builder()
        .title("Quick controls")
        .description("Everything else is on the Voice tab. Speed, random speed changes, harmony and noise reduction apply to recordings and speech only.")
        .build();
    for p in [P::Key, P::Scale, P::Retune, P::Strength, P::Hold, P::Shift, P::Formant, P::Gate] {
        quick.add(&param_control(app, p));
    }
    page.append(&quick);

    {
        let a = app.clone();
        go.connect_clicked(move |_| {
            if a.live.borrow().is_running() {
                a.stop_live();
            } else {
                a.start_live();
            }
        });
    }

    let refresh = {
        let a = app.clone();
        let go = go.clone();
        let in_bar = in_bar.clone();
        let out_bar = out_bar.clone();
        let detected = detected.clone();
        let info = info.clone();
        let trace = trace.clone();
        move || {
            let live = a.live.borrow();
            let running = live.is_running();
            go.set_label(if running { "Stop" } else { "Go live" });
            if running {
                go.remove_css_class("suggested-action");
                go.add_css_class("destructive-action");
            } else {
                go.remove_css_class("destructive-action");
                go.add_css_class("suggested-action");
            }
            let t = live.telemetry();
            in_bar.set_value(meter(t.input_level.load()));
            out_bar.set_value(meter(t.output_level.load()));
            if !running {
                detected.set_label("Not live.");
                info.set_label("");
            } else if t.no_voice_warning.load(Ordering::Relaxed) {
                detected.set_label("Hearing sound but no pitched voice. Speak up, get closer to the microphone, or raise Voicing sensitivity on the Voice tab.");
            } else if t.voice_detected.load(Ordering::Relaxed) {
                let target = t.target_midi.load(Ordering::Relaxed);
                let a4 = a.params().get(P::RefA4) as f64;
                detected.set_label(&if target != i32::MIN {
                    format!("Voice detected. Target note {} ({:.0} Hz)", scales::note_name(target), pitch::midi_to_hz(target as f64, a4))
                } else {
                    "Voice detected.".to_string()
                });
            } else {
                detected.set_label("Listening...");
            }
            if running {
                info.set_label(&format!(
                    "{} Hz. About {} ms of delay from this app, plus the sound system's own. {} dropouts.",
                    live.sample_rate(),
                    live.latency_ms(),
                    t.dropouts.load(Ordering::Relaxed)
                ));
            }
            trace.queue_draw();
        }
    };
    refresh();
    {
        let refresh = refresh.clone();
        app.connect(move |c| {
            if c == Change::Live {
                refresh();
            }
        });
    }
    // Meters and trace move at about 30 frames a second while live.
    let a = app.clone();
    let weak = page.downgrade();
    glib::timeout_add_local(Duration::from_millis(33), move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        if a.live.borrow().is_running() {
            refresh();
        }
        glib::ControlFlow::Continue
    });
    scrolled_page(&page).upcast()
}

/// Maps a peak level to a meter position on a decibel scale (-60 dB to 0 dB).
fn meter(peak: f32) -> f64 {
    if peak <= 0.0 { 0.0 } else { ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0) as f64 }
}

fn draw_trace(app: &App, area: &gtk4::DrawingArea, cr: &gtk4::cairo::Context, w: f64, h: f64) {
    let pal = palette(app.settings.borrow().aperture_accent);
    pal.background.set(cr);
    cr.paint().ok();
    let points: Vec<(f32, f32)> = app.live.borrow().telemetry().trace.lock().map(|t| t.iter().copied().collect()).unwrap_or_default();
    let p = app.params();
    let a4 = p.get(P::RefA4) as f64;
    let mask = scales::mask(p.int(P::Scale), p.int(P::Key), p.int(P::CustomMask));
    // Centre the view on the recent voice, two octaves tall.
    let mut voiced: Vec<f64> = points.iter().filter(|(i, _)| *i > 0.0).map(|(i, _)| pitch::hz_to_midi(*i as f64, a4)).collect();
    voiced.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let centre = voiced.get(voiced.len() / 2).copied().unwrap_or(57.0);
    let lo = centre - 12.0;
    let hi = centre + 12.0;
    let y = |m: f64| h - (m - lo) / (hi - lo) * h;
    for m in lo.floor() as i32..=hi.ceil() as i32 {
        // Notes of the scale shaded, black-key rows a little darker, as in the note editor.
        let black = [1, 3, 6, 8, 10].contains(&m.rem_euclid(12));
        let tint = match (scales::is_allowed(mask, m), black) {
            (true, true) => Some(pal.accent.with_alpha(0.16)),
            (true, false) => Some(pal.accent.with_alpha(0.08)),
            (false, true) => Some(pal.text.with_alpha(0.05)),
            _ => None,
        };
        if let Some(t) = tint {
            t.set(cr);
            cr.rectangle(0.0, y(m as f64 + 0.5), w, y(m as f64 - 0.5) - y(m as f64 + 0.5));
            cr.fill().ok();
        }
        if m.rem_euclid(12) == 0 {
            pal.dim.set(cr);
            style::draw_text(cr, area, &scales::note_name(m), 10.0, 4.0, y(m as f64) + 3.0);
        }
    }
    let n = crate::audio::live::TRACE_POINTS as f64;
    let x = |i: usize, len: usize| w - (len - i) as f64 * w / n;
    // Input as dots.
    pal.dim.with_alpha(0.8).set(cr);
    for (i, (inp, _)) in points.iter().enumerate() {
        if *inp > 0.0 {
            cr.arc(x(i, points.len()), y(pitch::hz_to_midi(*inp as f64, a4)), 1.6, 0.0, std::f64::consts::TAU);
            cr.fill().ok();
        }
    }
    // Output as a line, broken where unvoiced and at jumps of over half an octave: those are the pitch tracker
    // briefly locking onto an octave above or below, and a line to them would draw a spike that was never heard.
    let pen: Rgba = pal.accent;
    pen.set(cr);
    cr.set_line_width(2.5);
    let mut last: Option<f64> = None;
    for (i, (_, out)) in points.iter().enumerate() {
        let m = (*out > 0.0).then(|| pitch::hz_to_midi(*out as f64, a4));
        match (last, m) {
            (Some(l), Some(m)) if (m - l).abs() <= 6.0 => cr.line_to(x(i, points.len()), y(m)),
            (_, Some(m)) => {
                cr.stroke().ok();
                cr.move_to(x(i, points.len()), y(m));
            }
            _ => {}
        }
        last = m;
    }
    cr.stroke().ok();
}

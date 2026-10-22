//! Type text, have it spoken by eSpeak NG or Piper, then tuned and processed.

use std::rc::Rc;

use adw::prelude::*;
use gtk4::{gio, Orientation};

use super::widgets::{page_box, preset_card, scrolled_page};
use crate::app::{App, Change};
use crate::tts;

pub fn build(app: &Rc<App>, window: &adw::ApplicationWindow, open_editor: impl Fn() + 'static) -> gtk4::Widget {
    let page = page_box();
    let s = app.settings.borrow().clone();

    let text = gtk4::TextView::builder().wrap_mode(gtk4::WrapMode::WordChar).top_margin(10).bottom_margin(10).left_margin(10).right_margin(10).build();
    text.buffer().set_text(&s.tts_text);
    text.set_widget_name("speak-text");
    let frame = gtk4::Frame::builder().child(&gtk4::ScrolledWindow::builder().min_content_height(110).child(&text).build()).build();
    page.append(&gtk4::Label::builder().label("What should GLaDOS say?").xalign(0.0).css_classes(["heading"]).build());
    page.append(&frame);
    {
        let a = app.clone();
        text.buffer().connect_changed(move |b| {
            a.settings.borrow_mut().tts_text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
            a.save_soon();
        });
    }

    let espeak = tts::espeak_available();
    let piper = tts::piper_available();

    let actions = gtk4::Box::new(Orientation::Horizontal, 8);
    let speak = gtk4::Button::builder().label("Speak").css_classes(["suggested-action", "pill", "big-button"]).hexpand(true).build();
    speak.set_widget_name("speak-button");
    let stop = gtk4::Button::from_icon_name("media-playback-stop-symbolic");
    stop.set_tooltip_text(Some("Stop"));
    let save = gtk4::Button::from_icon_name("document-save-symbolic");
    save.set_tooltip_text(Some("Save as WAV"));
    actions.append(&speak);
    actions.append(&stop);
    actions.append(&save);
    page.append(&actions);

    let presets = adw::PreferencesGroup::new();
    presets.add(&preset_card(app));
    page.append(&presets);

    let group = adw::PreferencesGroup::builder().title("Speech").build();
    let engines = gtk4::StringList::new(&["eSpeak NG", "Piper"]);
    let engine = adw::ComboRow::builder().title("Engine").model(&engines).build();
    engine.set_selected(if s.tts_piper && piper { 1 } else { 0 });
    engine.set_sensitive(piper);
    engine.set_subtitle(if piper { "Piper sounds natural but needs a voice model." } else { "Install piper for natural-sounding voices." });
    group.add(&engine);

    let voices = tts::espeak_voices();
    let labels: Vec<&str> = voices.iter().map(|v| v.label.as_str()).collect();
    let voice = adw::ComboRow::builder().title("Voice").model(&gtk4::StringList::new(&labels)).enable_search(true).build();
    if let Some(i) = voices.iter().position(|v| v.id == s.tts_voice) {
        voice.set_selected(i as u32);
    }
    group.add(&voice);
    let female = adw::SwitchRow::builder().title("Female voice").subtitle("eSpeak NG's higher variant, closer to GLaDOS.").active(s.tts_female).build();
    group.add(&female);

    let model_row = adw::ActionRow::builder().title("Piper voice model").activatable(true).build();
    model_row.set_subtitle(if s.piper_model.is_empty() { "Choose a .onnx file" } else { &s.piper_model });
    model_row.add_suffix(&gtk4::Image::from_icon_name("document-open-symbolic"));
    group.add(&model_row);

    let rate = adw::SpinRow::with_range(0.5, 2.0, 0.05);
    rate.set_title("Speaking rate");
    rate.set_digits(2);
    rate.set_value(s.tts_rate as f64);
    group.add(&rate);
    let pitch = adw::SpinRow::with_range(0.5, 2.0, 0.05);
    pitch.set_title("Base pitch");
    pitch.set_digits(2);
    pitch.set_value(s.tts_pitch as f64);
    group.add(&pitch);
    let tune = adw::SwitchRow::builder()
        .title("Tune speech to the scale")
        .subtitle("Snaps each syllable to the key and scale from the Voice tab. Turn off to keep the engine's own melody.")
        .active(s.tts_tune)
        .build();
    group.add(&tune);
    page.append(&group);

    let sync_engine = {
        let voice = voice.clone();
        let female = female.clone();
        let model_row = model_row.clone();
        let pitch = pitch.clone();
        let engine = engine.clone();
        move || {
            let use_piper = engine.selected() == 1;
            voice.set_visible(!use_piper);
            female.set_visible(!use_piper);
            pitch.set_visible(!use_piper);
            model_row.set_visible(use_piper);
        }
    };
    sync_engine();
    {
        let a = app.clone();
        let sync_engine = sync_engine.clone();
        engine.connect_selected_notify(move |e| {
            a.settings.borrow_mut().tts_piper = e.selected() == 1;
            a.save_soon();
            sync_engine();
        });
    }
    {
        let a = app.clone();
        let voices = voices.clone();
        voice.connect_selected_notify(move |v| {
            if let Some(x) = voices.get(v.selected() as usize) {
                a.settings.borrow_mut().tts_voice = x.id.clone();
                a.save_soon();
            }
        });
    }
    {
        let a = app.clone();
        female.connect_active_notify(move |f| {
            a.settings.borrow_mut().tts_female = f.is_active();
            a.save_soon();
        });
    }
    {
        let a = app.clone();
        rate.connect_value_notify(move |r| {
            a.settings.borrow_mut().tts_rate = r.value() as f32;
            a.save_soon();
        });
    }
    {
        let a = app.clone();
        pitch.connect_value_notify(move |r| {
            a.settings.borrow_mut().tts_pitch = r.value() as f32;
            a.save_soon();
        });
    }
    {
        let a = app.clone();
        tune.connect_active_notify(move |t| {
            a.settings.borrow_mut().tts_tune = t.is_active();
            a.save_soon();
        });
    }
    {
        let a = app.clone();
        let window = window.clone();
        model_row.connect_activated(move |row| {
            let filter = gtk4::FileFilter::new();
            filter.set_name(Some("Piper voice models"));
            filter.add_pattern("*.onnx");
            let filters = gio::ListStore::new::<gtk4::FileFilter>();
            filters.append(&filter);
            let chooser = gtk4::FileDialog::builder().title("Choose a Piper voice model").filters(&filters).build();
            let a = a.clone();
            let row = row.clone();
            chooser.open(Some(&window), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let p = path.to_string_lossy().to_string();
                        row.set_subtitle(&p);
                        a.settings.borrow_mut().piper_model = p;
                        a.save_soon();
                    }
                }
            });
        });
    }

    {
        let a = app.clone();
        let text = text.clone();
        speak.connect_clicked(move |_| {
            let b = text.buffer();
            let s = a.settings.borrow().clone();
            let req = tts::Request {
                text: b.text(&b.start_iter(), &b.end_iter(), false).to_string(),
                engine: if s.tts_piper && tts::piper_available() { tts::Engine::Piper } else { tts::Engine::Espeak },
                voice: s.tts_voice.clone(),
                female: s.tts_female,
                piper_model: s.piper_model.clone(),
                rate: s.tts_rate,
                pitch: s.tts_pitch,
            };
            a.speak(req, s.tts_tune);
        });
    }
    {
        let a = app.clone();
        stop.connect_clicked(move |_| a.player.stop());
    }
    {
        let a = app.clone();
        let window = window.clone();
        save.connect_clicked(move |_| save_dialog(&a, &window));
    }

    let open = gtk4::Button::builder().label("Open in the note editor to sing it or fine-tune every syllable").css_classes(["flat"]).halign(gtk4::Align::Start).build();
    open.connect_clicked(move |_| open_editor());
    page.append(&open);
    let note = if espeak {
        "Speech comes from eSpeak NG (or Piper, if installed), then is pitch-corrected and run through the effects from the Voice tab."
    } else {
        "eSpeak NG is not installed. Install the espeak-ng package (or piper) to generate speech."
    };
    page.append(&gtk4::Label::builder().label(note).wrap(true).xalign(0.0).css_classes(["dim-label", "caption"]).build());

    let refresh = {
        let a = app.clone();
        let speak = speak.clone();
        let save = save.clone();
        let open = open.clone();
        move || {
            let idle = a.busy().is_none();
            speak.set_sensitive(idle && (espeak || piper));
            save.set_sensitive(idle && a.clip().is_some());
            open.set_visible(a.clip().is_some());
        }
    };
    refresh();
    app.connect(move |c| {
        if matches!(c, Change::Busy | Change::Clip) {
            refresh();
        }
    });
    scrolled_page(&page).upcast()
}

/// Asks where to save the processed clip, then exports it.
pub fn save_dialog(app: &Rc<App>, window: &adw::ApplicationWindow) {
    if app.clip().is_none() {
        return;
    }
    let chooser = gtk4::FileDialog::builder().title("Save as WAV").initial_name("glados.wav").build();
    let a = app.clone();
    chooser.save(Some(window), gio::Cancellable::NONE, move |res| {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                a.export_wav(path);
            }
        }
    });
}

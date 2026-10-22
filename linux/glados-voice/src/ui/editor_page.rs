//! The Melodyne-style note editor page.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use glados_dsp::model::P;
use glados_dsp::scales;
use glados_dsp::{Edit, NoteOps};
use gtk4::{gio, glib, Orientation};

use super::piano_roll::PianoRoll;
use super::widgets::{page_box, param_control, plain_row, preset_card, scrolled_page};
use crate::app::{App, Change, Tool};

fn icon_button(icon: &str, tip: &str) -> gtk4::Button {
    let b = gtk4::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("flat");
    b
}

/// A labelled slider with a formatted value, for the per-note and correction controls.
struct Labeled {
    widget: gtk4::Box,
    scale: gtk4::Scale,
    value: gtk4::Label,
    busy: Rc<Cell<bool>>,
    format: fn(f64) -> String,
}

impl Labeled {
    fn new(title: &str, min: f64, max: f64, step: f64, format: fn(f64) -> String) -> Labeled {
        let widget = gtk4::Box::new(Orientation::Vertical, 2);
        let header = gtk4::Box::new(Orientation::Horizontal, 8);
        header.append(&gtk4::Label::builder().label(title).xalign(0.0).hexpand(true).build());
        let value = gtk4::Label::builder().css_classes(["accent", "numeric"]).build();
        header.append(&value);
        let scale = gtk4::Scale::with_range(Orientation::Horizontal, min, max, step);
        scale.set_draw_value(false);
        widget.append(&header);
        widget.append(&scale);
        let l = Labeled { widget, scale, value, busy: Rc::new(Cell::new(false)), format };
        l.set(min.max(0.0).min(max));
        l
    }

    fn set(&self, v: f64) {
        self.busy.set(true);
        self.scale.set_value(v);
        self.busy.set(false);
        self.value.set_label(&(self.format)(v));
    }

    fn on_change(&self, f: impl Fn(f64) + 'static) {
        let busy = self.busy.clone();
        let value = self.value.clone();
        let format = self.format;
        self.scale.connect_value_changed(move |s| {
            value.set_label(&format(s.value()));
            if !busy.get() {
                f(s.value());
            }
        });
    }
}

fn format_clock(sec: f32) -> String {
    let m = (sec / 60.0) as i32;
    format!("{}:{:04.1}", m, sec - m as f32 * 60.0)
}

pub fn build(app: &Rc<App>, window: &adw::ApplicationWindow, go_speak: impl Fn() + 'static) -> gtk4::Widget {
    let page = page_box();
    page.set_spacing(12);
    let mic = gtk4::FileFilter::new();
    mic.set_name(Some("Audio files"));
    mic.add_mime_type("audio/*");

    // Transport and file actions.
    let transport = gtk4::Box::new(Orientation::Horizontal, 2);
    let record = icon_button("media-record-symbolic", "Record");
    record.set_widget_name("record");
    let import = icon_button("document-open-symbolic", "Import audio");
    let play = icon_button("media-playback-start-symbolic", "Play");
    let stop = icon_button("media-playback-stop-symbolic", "Stop");
    let looping = gtk4::ToggleButton::builder().icon_name("media-playlist-repeat-symbolic").tooltip_text("Loop").css_classes(["flat"]).build();
    let undo = icon_button("edit-undo-symbolic", "Undo");
    let redo = icon_button("edit-redo-symbolic", "Redo");
    let save = icon_button("document-save-symbolic", "Save as WAV");
    for w in [&record, &import, &play, &stop] {
        transport.append(w);
    }
    transport.append(&looping);
    for w in [&undo, &redo, &save] {
        transport.append(w);
    }
    let rec_level = super::widgets::level_bar();
    rec_level.set_width_request(120);
    rec_level.set_visible(false);
    rec_level.set_tooltip_text(Some("Microphone level"));
    transport.append(&rec_level);
    let time = gtk4::Label::builder().xalign(1.0).hexpand(true).css_classes(["dim-label", "numeric"]).build();
    transport.append(&time);
    page.append(&transport);

    // Empty state.
    let empty = adw::StatusPage::builder()
        .icon_name("document-edit-symbolic")
        .title("Note editor")
        .description("Every syllable becomes a note you can drag to any pitch, stretch, retune to a scale, or push toward GLaDOS. Start by recording your voice, importing an audio file, or generating speech.")
        .build();
    let empty_buttons = gtk4::Box::new(Orientation::Vertical, 8);
    empty_buttons.set_halign(gtk4::Align::Center);
    let e_record = gtk4::Button::builder().label("Record").css_classes(["suggested-action", "pill"]).build();
    let e_import = gtk4::Button::builder().label("Import audio file").css_classes(["pill"]).build();
    let e_speak = gtk4::Button::builder().label("Generate speech").css_classes(["pill"]).build();
    empty_buttons.append(&e_record);
    empty_buttons.append(&e_import);
    empty_buttons.append(&e_speak);
    empty.set_child(Some(&empty_buttons));
    page.append(&empty);

    // Editor proper.
    let editor = gtk4::Box::new(Orientation::Vertical, 12);
    let tools = gtk4::Box::new(Orientation::Horizontal, 6);
    let t_pitch = gtk4::ToggleButton::with_label("Pitch");
    let t_time = gtk4::ToggleButton::with_label("Timing");
    let t_split = gtk4::ToggleButton::with_label("Split");
    t_time.set_group(Some(&t_pitch));
    t_split.set_group(Some(&t_pitch));
    t_pitch.set_active(true);
    let tool_box = gtk4::Box::new(Orientation::Horizontal, 0);
    tool_box.add_css_class("linked");
    tool_box.append(&t_pitch);
    tool_box.append(&t_time);
    tool_box.append(&t_split);
    tools.append(&tool_box);
    let snap = gtk4::ToggleButton::builder().label("Snap to scale").active(true).build();
    let join = gtk4::Button::with_label("Join");
    let select_all = gtk4::Button::with_label("Select all");
    tools.append(&snap);
    tools.append(&join);
    tools.append(&select_all);
    let spacer = gtk4::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    tools.append(&spacer);
    let zoom_out = icon_button("zoom-out-symbolic", "Zoom out time");
    let zoom_in = icon_button("zoom-in-symbolic", "Zoom in time");
    let rows_less = gtk4::Button::builder().label("Rows -").css_classes(["flat"]).build();
    let rows_more = gtk4::Button::builder().label("Rows +").css_classes(["flat"]).build();
    let fit = gtk4::Button::builder().label("Fit").css_classes(["flat"]).build();
    for w in [&zoom_out, &zoom_in, &rows_less, &rows_more, &fit] {
        tools.append(w);
    }
    editor.append(&tools);
    let hint = gtk4::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label", "caption"]).build();
    editor.append(&hint);

    let roll = PianoRoll::new(app);
    let frame = gtk4::Frame::builder().child(&roll.widget).height_request(380).build();
    editor.append(&frame);

    let presets = adw::PreferencesGroup::new();
    presets.add(&preset_card(app));
    editor.append(&presets);

    // Selected notes.
    let sel_group = adw::PreferencesGroup::builder().title("Selected notes").build();
    let sel_title = gtk4::Label::builder().xalign(0.0).wrap(true).build();
    let offset = Labeled::new("Pitch offset", -12.0, 12.0, 0.01, |v| format!("{v:+.2} st"));
    let drift = Labeled::new("Pitch drift", 0.0, 200.0, 1.0, |v| format!("{v:.0} %"));
    let modulation = Labeled::new("Vibrato and modulation", 0.0, 300.0, 1.0, |v| format!("{v:.0} %"));
    let formant = Labeled::new("Formant", -12.0, 12.0, 0.1, |v| format!("{v:+.1} st"));
    let gain = Labeled::new("Level", -24.0, 12.0, 0.1, |v| format!("{v:+.1} dB"));
    let sel_box = gtk4::Box::new(Orientation::Vertical, 6);
    sel_box.append(&sel_title);
    let sliders = gtk4::Box::new(Orientation::Vertical, 6);
    for l in [&offset, &drift, &modulation, &formant, &gain] {
        sliders.append(&l.widget);
    }
    sel_box.append(&sliders);
    sel_group.add(&plain_row(&sel_box));
    editor.append(&sel_group);
    {
        let a = app.clone();
        offset.on_change(move |v| a.mutate(Some("offset"), |e, _| for_selected(&a, e, |n| n.offset = v)));
    }
    {
        let a = app.clone();
        drift.on_change(move |v| a.mutate(Some("drift"), |e, _| for_selected(&a, e, |n| n.drift = (v / 100.0) as f32)));
    }
    {
        let a = app.clone();
        modulation.on_change(move |v| a.mutate(Some("mod"), |e, _| for_selected(&a, e, |n| n.modulation = (v / 100.0) as f32)));
    }
    {
        let a = app.clone();
        formant.on_change(move |v| a.mutate(Some("formant"), |e, _| for_selected(&a, e, |n| n.formant = v as f32)));
    }
    {
        let a = app.clone();
        gain.on_change(move |v| a.mutate(Some("gain"), |e, _| for_selected(&a, e, |n| n.gain_db = v as f32)));
    }

    // Correction.
    let corr_group = adw::PreferencesGroup::builder().title("Correct pitch").build();
    let corr_note = gtk4::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label", "caption"]).build();
    let center = Labeled::new("Pitch center", 0.0, 100.0, 1.0, |v| format!("{v:.0} %"));
    let remove_drift = Labeled::new("Remove drift", 0.0, 100.0, 1.0, |v| format!("{v:.0} %"));
    let remove_mod = Labeled::new("Remove modulation", 0.0, 100.0, 1.0, |v| format!("{v:.0} %"));
    for l in [&center, &remove_drift, &remove_mod] {
        l.set(100.0);
    }
    let corr_box = gtk4::Box::new(Orientation::Vertical, 6);
    corr_box.append(&corr_note);
    corr_box.append(&center.widget);
    corr_box.append(&remove_drift.widget);
    corr_box.append(&remove_mod.widget);
    let corr_buttons = gtk4::FlowBox::builder().selection_mode(gtk4::SelectionMode::None).max_children_per_line(4).column_spacing(6).row_spacing(6).build();
    let apply = gtk4::Button::builder().label("Apply").css_classes(["suggested-action"]).build();
    let use_tab = gtk4::Button::with_label("Use Voice tab tuning");
    let reset_notes = gtk4::Button::with_label("Reset notes");
    let reset_timing = gtk4::Button::with_label("Reset timing");
    for b in [&apply, &use_tab, &reset_notes, &reset_timing] {
        corr_buttons.append(b);
    }
    corr_box.append(&corr_buttons);
    corr_group.add(&plain_row(&corr_box));
    editor.append(&corr_group);
    {
        let a = app.clone();
        let (c, d, m) = (center.scale.clone(), remove_drift.scale.clone(), remove_mod.scale.clone());
        apply.connect_clicked(move |_| a.apply_correction(c.value() / 100.0, d.value() / 100.0, m.value() / 100.0));
    }
    {
        let a = app.clone();
        use_tab.connect_clicked(move |_| a.apply_voice_tuning());
    }
    {
        let a = app.clone();
        reset_notes.connect_clicked(move |_| a.reset_notes());
    }
    {
        let a = app.clone();
        reset_timing.connect_clicked(move |_| a.reset_timing());
    }

    // Clean-up and detection.
    let det_group = adw::PreferencesGroup::builder().title("Clean-up and detection").build();
    let det_summary = gtk4::Label::builder().xalign(0.0).wrap(true).build();
    let det_warn = gtk4::Label::builder().xalign(0.0).wrap(true).css_classes(["error"]).build();
    let det_clean_note = gtk4::Label::builder()
        .label("Generated speech is already clean, so noise reduction is skipped for it.")
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label", "caption"])
        .build();
    let det_top = gtk4::Box::new(Orientation::Vertical, 6);
    det_top.append(&det_summary);
    det_top.append(&det_warn);
    det_top.append(&det_clean_note);
    det_group.add(&plain_row(&det_top));
    det_group.add(&param_control(app, P::Denoise));
    det_group.add(&param_control(app, P::Voicing));
    det_group.add(&param_control(app, P::Fmin));
    det_group.add(&param_control(app, P::Fmax));
    let seg_tol = Labeled::new("Pitch jump that starts a new note", 30.0, 300.0, 1.0, |v| format!("{v:.0} cents"));
    seg_tol.set(app.seg_tolerance.get());
    let seg_sens = Labeled::new("Syllable splitting", 0.0, 100.0, 1.0, |v| format!("{v:.0} %"));
    seg_sens.set(app.seg_sensitivity.get());
    {
        let a = app.clone();
        seg_tol.on_change(move |v| a.seg_tolerance.set(v));
    }
    {
        let a = app.clone();
        seg_sens.on_change(move |v| a.seg_sensitivity.set(v));
    }
    let seg_box = gtk4::Box::new(Orientation::Vertical, 6);
    seg_box.append(
        &gtk4::Label::builder()
            .label("Voicing and the pitch range re-analyse the clip automatically while you have not edited it yet.")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .build(),
    );
    seg_box.append(&seg_tol.widget);
    seg_box.append(&seg_sens.widget);
    let seg_buttons = gtk4::Box::new(Orientation::Horizontal, 6);
    let redetect = gtk4::Button::with_label("Re-detect notes");
    let reanalyse = gtk4::Button::with_label("Re-analyse");
    seg_buttons.append(&redetect);
    seg_buttons.append(&reanalyse);
    seg_box.append(&seg_buttons);
    det_group.add(&plain_row(&seg_box));
    editor.append(&det_group);
    {
        let a = app.clone();
        redetect.connect_clicked(move |_| a.resegment());
    }
    {
        let a = app.clone();
        reanalyse.connect_clicked(move |_| a.reanalyse());
    }
    page.append(&editor);

    // ---- wiring ----
    let open_file = {
        let a = app.clone();
        let window = window.clone();
        move || {
            let filters = gio::ListStore::new::<gtk4::FileFilter>();
            filters.append(&mic);
            let chooser = gtk4::FileDialog::builder().title("Import audio").filters(&filters).build();
            let a = a.clone();
            chooser.open(Some(&window), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        a.import_file(path);
                    }
                }
            });
        }
    };
    let open_file = Rc::new(open_file);
    {
        let a = app.clone();
        record.connect_clicked(move |_| a.toggle_record());
    }
    {
        let a = app.clone();
        e_record.connect_clicked(move |_| a.toggle_record());
    }
    {
        let f = open_file.clone();
        import.connect_clicked(move |_| f());
    }
    {
        let f = open_file.clone();
        e_import.connect_clicked(move |_| f());
    }
    e_speak.connect_clicked(move |_| go_speak());
    {
        let a = app.clone();
        play.connect_clicked(move |_| a.toggle_play());
    }
    {
        let a = app.clone();
        stop.connect_clicked(move |_| a.player.stop());
    }
    {
        let a = app.clone();
        looping.connect_toggled(move |b| a.player.set_loop(b.is_active()));
    }
    {
        let a = app.clone();
        undo.connect_clicked(move |_| a.undo());
    }
    {
        let a = app.clone();
        redo.connect_clicked(move |_| a.redo());
    }
    {
        let a = app.clone();
        let window = window.clone();
        save.connect_clicked(move |_| super::speak_page::save_dialog(&a, &window));
    }
    let set_tool = {
        let a = app.clone();
        let hint = hint.clone();
        let roll = roll.clone();
        move |t: Tool| {
            a.tool.set(t);
            hint.set_label(match t {
                Tool::Pitch => "Click notes to select or deselect them. Drag a note up or down to change its pitch. Drag empty space to scroll; Ctrl+wheel or a pinch zooms.",
                Tool::Time => "Drag a note's edge to stretch it, or its middle to slide it. Click the ruler to move the playhead.",
                Tool::Split => "Click inside a note to cut it in two.",
            });
            roll.widget.queue_draw();
        }
    };
    set_tool(Tool::Pitch);
    for (b, t) in [(&t_pitch, Tool::Pitch), (&t_time, Tool::Time), (&t_split, Tool::Split)] {
        let set_tool = set_tool.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                set_tool(t);
            }
        });
    }
    {
        let a = app.clone();
        snap.connect_toggled(move |b| a.snap_to_scale.set(b.is_active()));
    }
    {
        let a = app.clone();
        join.connect_clicked(move |_| a.merge_selected());
    }
    {
        let a = app.clone();
        select_all.connect_clicked(move |_| a.select_all());
    }
    {
        let r = roll.clone();
        zoom_out.connect_clicked(move |_| r.zoom_time(0.7));
    }
    {
        let r = roll.clone();
        zoom_in.connect_clicked(move |_| r.zoom_time(1.4));
    }
    {
        let r = roll.clone();
        rows_less.connect_clicked(move |_| r.zoom_pitch(0.8));
    }
    {
        let r = roll.clone();
        rows_more.connect_clicked(move |_| r.zoom_pitch(1.25));
    }
    {
        let r = roll.clone();
        fit.connect_clicked(move |_| r.fit());
    }

    let refresh = {
        let a = app.clone();
        let (empty, editor) = (empty.clone(), editor.clone());
        let (record, e_record, import, play, stop, undo, redo, save) =
            (record.clone(), e_record.clone(), import.clone(), play.clone(), stop.clone(), undo.clone(), redo.clone(), save.clone());
        let (sel_title, sliders) = (sel_title.clone(), sliders.clone());
        let (offset_s, drift_s, mod_s, formant_s, gain_s) =
            (Rc::new(offset), Rc::new(drift), Rc::new(modulation), Rc::new(formant), Rc::new(gain));
        let (corr_note, det_summary, det_warn, det_clean_note) = (corr_note.clone(), det_summary.clone(), det_warn.clone(), det_clean_note.clone());
        move |c: Change| {
            let clip = a.clip();
            let busy = a.busy().is_some();
            let recording = a.is_recording();
            empty.set_visible(clip.is_none());
            editor.set_visible(clip.is_some());
            record.set_icon_name(if recording { "media-playback-stop-symbolic" } else { "media-record-symbolic" });
            record.set_tooltip_text(Some(if recording { "Stop recording" } else { "Record" }));
            if recording {
                record.add_css_class("destructive-action");
            } else {
                record.remove_css_class("destructive-action");
            }
            e_record.set_label(if recording { "Stop recording" } else { "Record" });
            record.set_sensitive(recording || !busy);
            import.set_sensitive(!busy && !recording);
            play.set_sensitive(clip.is_some());
            stop.set_sensitive(clip.is_some());
            undo.set_sensitive(a.can_undo());
            redo.set_sensitive(a.can_redo());
            save.set_sensitive(clip.is_some() && !busy);
            if matches!(c, Change::Busy | Change::Recording) {
                return;
            }
            let Some(cs) = clip else { return };
            let an = &cs.analysis;
            let p = a.params();
            let shift = NoteOps::global_shift(an, &p);
            let sel = a.selection();
            corr_note.set_label(&format!(
                "Uses the key and scale from the Voice tab. Applies to {}.",
                if sel.is_empty() { "all notes".to_string() } else { format!("{} selected", sel.len()) }
            ));
            let summary = a.with_edit(|e: &Edit| {
                let first = sel.iter().next().and_then(|i| e.notes.get(*i)).cloned();
                (e.notes.len(), first)
            });
            let (count, first) = summary.unwrap_or((0, None));
            match first {
                None => {
                    sel_title.set_label("Select one or more notes to adjust them precisely. Corrections below apply to everything when nothing is selected.");
                    sliders.set_sensitive(false);
                }
                Some(n) => {
                    let fin = n.center + n.offset + shift;
                    let r = fin.round();
                    sel_title.set_label(&format!("{} selected: {} ({:+.0} cents)", sel.len(), scales::note_name(r as i32), (fin - r) * 100.0));
                    sliders.set_sensitive(true);
                    offset_s.set(n.offset);
                    drift_s.set(n.drift as f64 * 100.0);
                    mod_s.set(n.modulation as f64 * 100.0);
                    formant_s.set(n.formant as f64);
                    gain_s.set(n.gain_db as f64);
                }
            }
            let voiced = an.voiced.iter().filter(|v| **v).count();
            let pct = (100 * voiced / (an.frames().max(1) as usize)) as i32;
            det_summary.set_label(&format!("{}: found {count} notes. {pct}% of the clip is pitched voice.", cs.title));
            let mut warn = String::new();
            if pct < 20 {
                warn.push_str("Very little pitched voice was found. Raise Voicing sensitivity, widen the pitch range, or check that the recording is speech. ");
            }
            if a.analysis_stale() {
                warn.push_str("Detection settings changed after you made edits. Re-analyse to apply them (edits will be lost).");
            }
            det_warn.set_label(warn.trim());
            det_warn.set_visible(!warn.is_empty());
            det_clean_note.set_visible(!cs.noisy);
        }
    };
    refresh(Change::Clip);
    {
        let refresh = refresh.clone();
        app.connect(move |c| {
            if !matches!(c, Change::Live | Change::Appearance | Change::Presets) {
                refresh(c);
            }
        });
    }

    // Playhead, time and recording level move at about 30 frames a second.
    let a = app.clone();
    let weak = page.downgrade();
    let was_playing = Cell::new(false);
    glib::timeout_add_local(Duration::from_millis(33), move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        a.poll();
        let playing = a.player.is_playing();
        play.set_icon_name(if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
        rec_level.set_visible(a.is_recording());
        if a.is_recording() {
            let peak = a.recorder_level();
            rec_level.set_value(if peak <= 0.0 { 0.0 } else { ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0) as f64 });
            time.set_label(&format!("Recording {}", format_clock(a.recording_seconds())));
        } else if let Some(cs) = a.clip() {
            let n = a.with_edit(|e| e.notes.len()).unwrap_or(0);
            time.set_label(&format!(
                "{}: {} / {}   {n} notes",
                cs.title,
                format_clock(a.player.position_seconds()),
                format_clock(a.player.duration_seconds())
            ));
        } else {
            time.set_label("");
        }
        if playing || was_playing.get() {
            roll.follow_playhead();
        }
        was_playing.set(playing);
        glib::ControlFlow::Continue
    });
    scrolled_page(&page).upcast()
}

fn for_selected(app: &App, e: &mut Edit, f: impl Fn(&mut glados_dsp::Note)) {
    for i in app.selection() {
        if let Some(n) = e.notes.get_mut(i) {
            f(n);
        }
    }
}

//! The note editor canvas: every syllable as a blob you can drag in pitch, stretch in time, or cut in two.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use glados_dsp::model::P;
use glados_dsp::scales;
use glados_dsp::{Analysis, Edit, Note, NoteOps, TimeMap};
use gtk4::{gdk, glib};

use super::style::{self, palette, Palette};
use crate::app::{App, Change, Tool};

/// Visible window onto the clip: horizontal scale and scroll in seconds, vertical scale and top edge in MIDI notes.
#[derive(Clone, Copy, Debug)]
struct RollView {
    px_per_sec: f64,
    scroll_sec: f64,
    px_per_semi: f64,
    top_midi: f64,
}

const GUTTER: f64 = 44.0;
const RULER: f64 = 22.0;
const EDGE_ZONE: f64 = 10.0;
const SLOP: f64 = 4.0;

#[derive(Clone, Copy)]
struct Geo {
    view: RollView,
}

impl Geo {
    fn x(&self, sec: f64) -> f64 {
        GUTTER + (sec - self.view.scroll_sec) * self.view.px_per_sec
    }
    fn sec(&self, x: f64) -> f64 {
        self.view.scroll_sec + (x - GUTTER) / self.view.px_per_sec
    }
    fn y(&self, midi: f64) -> f64 {
        RULER + (self.view.top_midi - midi) * self.view.px_per_semi
    }
}

#[derive(Clone)]
struct NoteBox {
    index: usize,
    note: Note,
    x0: f64,
    x1: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drag {
    None,
    Pan,
    Seek,
    Pitch,
    TimeLeft,
    TimeRight,
    TimeBody,
}

struct DragState {
    mode: Drag,
    start: (f64, f64),
    last: (f64, f64),
    moved: bool,
    started: bool,
    hit: Option<NoteBox>,
    geo: Geo,
    start_offsets: Vec<(usize, f64)>,
    total_dy: f64,
}

struct State {
    app: Rc<App>,
    view: Cell<Option<RollView>>,
    hint: RefCell<Option<String>>,
    drag: RefCell<Option<DragState>>,
}

#[derive(Clone)]
pub struct PianoRoll {
    pub widget: gtk4::DrawingArea,
    state: Rc<State>,
}

fn shift_and_mask(app: &App, an: &Analysis) -> (f64, i32) {
    let p = app.params();
    (NoteOps::global_shift(an, &p), scales::mask(p.int(P::Scale), p.int(P::Key), p.int(P::CustomMask)))
}

fn layout(edit: &Edit, an: &Analysis, tm: &TimeMap, geo: &Geo) -> Vec<NoteBox> {
    let sr = an.sample_rate as f64;
    edit.notes
        .iter()
        .enumerate()
        .map(|(i, n)| NoteBox {
            index: i,
            note: n.clone(),
            x0: geo.x(tm.to_output(n.in_start(an) as f64) / sr),
            x1: geo.x(tm.to_output(n.in_end(an) as f64) / sr),
        })
        .collect()
}

/// The note under (x, y), allowing a generous margin around the drawn blob.
fn hit_test(boxes: &[NoteBox], x: f64, y: f64, geo: &Geo, an: &Analysis, tm: &TimeMap, shift: f64) -> Option<NoteBox> {
    if y < RULER || x < GUTTER {
        return None;
    }
    let slop = 12.0;
    let mut best: Option<&NoteBox> = None;
    let mut best_d = f64::MAX;
    for b in boxes {
        if x < b.x0 - slop / 2.0 || x > b.x1 + slop / 2.0 {
            continue;
        }
        let frame = (tm.to_input(geo.sec(x) * an.sample_rate as f64) / an.hop as f64).clamp(b.note.f0 as f64, b.note.f1 as f64);
        let d = (y - geo.y(b.note.pitch_at(frame) + shift)).abs();
        if d < slop + 8.0 && d < best_d {
            best = Some(b);
            best_d = d;
        }
    }
    best.cloned()
}

fn pitch_label(midi: f64) -> String {
    let n = midi.round() as i32;
    let cents = ((midi - n as f64) * 100.0).round() as i32;
    format!("{} {}{} cents", scales::note_name(n), if cents >= 0 { "+" } else { "" }, cents)
}

fn format_time(t: f64) -> String {
    let m = (t / 60.0) as i32;
    let s = t - m as f64 * 60.0;
    if m > 0 { format!("{m}:{s:04.1}") } else { format!("{s:.2}s") }
}

impl PianoRoll {
    pub fn new(app: &Rc<App>) -> PianoRoll {
        let widget = gtk4::DrawingArea::builder().hexpand(true).vexpand(true).focusable(true).build();
        widget.set_widget_name("piano-roll");
        let state = Rc::new(State { app: app.clone(), view: Cell::new(None), hint: RefCell::new(None), drag: RefCell::new(None) });
        let roll = PianoRoll { widget, state };

        {
            let st = roll.state.clone();
            roll.widget.set_draw_func(move |area, cr, w, h| {
                let (w, h) = (w as f64, h as f64);
                if st.view.get().is_none() {
                    if let Some(v) = fit_view(&st.app, w, h) {
                        st.view.set(Some(v));
                    }
                }
                draw(&st, area, cr, w, h);
            });
        }

        let drag = gtk4::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        {
            let st = roll.state.clone();
            drag.connect_drag_begin(move |_, x, y| begin(&st, x, y));
        }
        {
            let st = roll.state.clone();
            let area = roll.widget.clone();
            drag.connect_drag_update(move |_, dx, dy| {
                update(&st, dx, dy);
                area.queue_draw();
            });
        }
        {
            let st = roll.state.clone();
            let area = roll.widget.clone();
            drag.connect_drag_end(move |_, _, _| {
                end(&st);
                area.queue_draw();
            });
        }
        roll.widget.add_controller(drag);

        // Wheel: up and down scrolls the notes, sideways (or with Shift) scrolls time, Ctrl zooms time.
        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::BOTH_AXES);
        {
            let st = roll.state.clone();
            let area = roll.widget.clone();
            scroll.connect_scroll(move |c, dx, dy| {
                let Some(mut v) = st.view.get() else { return glib::Propagation::Proceed };
                let mods = c.current_event_state();
                if mods.contains(gdk::ModifierType::CONTROL_MASK) {
                    let f = if dy < 0.0 { 1.15 } else { 1.0 / 1.15 };
                    let w = area.width() as f64;
                    let centre = v.scroll_sec + (w - GUTTER) / 2.0 / v.px_per_sec;
                    v.px_per_sec = (v.px_per_sec * f).clamp(8.0, 4000.0);
                    v.scroll_sec = centre - (w - GUTTER) / 2.0 / v.px_per_sec;
                } else if mods.contains(gdk::ModifierType::SHIFT_MASK) || dx != 0.0 {
                    let d = if dx != 0.0 { dx } else { dy };
                    v.scroll_sec = (v.scroll_sec + d * 40.0 / v.px_per_sec).max(-1.0);
                } else {
                    v.top_midi = (v.top_midi - dy * 2.0).clamp(30.0, 110.0);
                }
                st.view.set(Some(v));
                area.queue_draw();
                glib::Propagation::Stop
            });
        }
        roll.widget.add_controller(scroll);

        // Touchpad or touchscreen pinch zooms time.
        let zoom = gtk4::GestureZoom::new();
        let zoom_start = Rc::new(Cell::new(None::<RollView>));
        {
            let st = roll.state.clone();
            let zs = zoom_start.clone();
            zoom.connect_begin(move |_, _| zs.set(st.view.get()));
        }
        {
            let st = roll.state.clone();
            let area = roll.widget.clone();
            zoom.connect_scale_changed(move |_, scale| {
                if let Some(mut v) = zoom_start.get() {
                    let w = area.width() as f64;
                    let centre = v.scroll_sec + (w - GUTTER) / 2.0 / v.px_per_sec;
                    v.px_per_sec = (v.px_per_sec * scale).clamp(8.0, 4000.0);
                    v.scroll_sec = centre - (w - GUTTER) / 2.0 / v.px_per_sec;
                    st.view.set(Some(v));
                    area.queue_draw();
                }
            });
        }
        roll.widget.add_controller(zoom);

        {
            let area = roll.widget.clone();
            let st = roll.state.clone();
            app.connect(move |c| match c {
                Change::Clip => {
                    st.view.set(None);
                    area.queue_draw();
                }
                Change::Edit | Change::Selection | Change::Params | Change::Appearance => area.queue_draw(),
                _ => {}
            });
        }
        roll
    }

    pub fn zoom_time(&self, f: f64) {
        let Some(mut v) = self.state.view.get() else { return };
        let w = self.widget.width() as f64;
        let centre = v.scroll_sec + (w - GUTTER) / 2.0 / v.px_per_sec;
        v.px_per_sec = (v.px_per_sec * f).clamp(8.0, 4000.0);
        v.scroll_sec = centre - (w - GUTTER) / 2.0 / v.px_per_sec;
        self.state.view.set(Some(v));
        self.widget.queue_draw();
    }

    pub fn zoom_pitch(&self, f: f64) {
        let Some(mut v) = self.state.view.get() else { return };
        let h = self.widget.height() as f64;
        let centre = v.top_midi - (h - RULER) / 2.0 / v.px_per_semi;
        v.px_per_semi = (v.px_per_semi * f).clamp(6.0, 60.0);
        v.top_midi = centre + (h - RULER) / 2.0 / v.px_per_semi;
        self.state.view.set(Some(v));
        self.widget.queue_draw();
    }

    pub fn fit(&self) {
        let (w, h) = (self.widget.width() as f64, self.widget.height() as f64);
        if let Some(v) = fit_view(&self.state.app, w, h) {
            self.state.view.set(Some(v));
            self.widget.queue_draw();
        }
    }

    /// Keeps the playhead on screen while playing. Call from a regular tick.
    pub fn follow_playhead(&self) {
        let Some(mut v) = self.state.view.get() else { return };
        let pos = self.state.app.playhead_seconds() as f64;
        let x = GUTTER + (pos - v.scroll_sec) * v.px_per_sec;
        if x > self.widget.width() as f64 - 24.0 || x < GUTTER {
            v.scroll_sec = (pos - 0.5).max(0.0);
            self.state.view.set(Some(v));
        }
        self.widget.queue_draw();
    }
}

fn fit_view(app: &App, w: f64, h: f64) -> Option<RollView> {
    if w <= GUTTER || h <= RULER {
        return None;
    }
    let cs = app.clip()?;
    let an = &cs.analysis;
    let (shift, _) = shift_and_mask(app, an);
    let duration = (an.length as f64 / an.sample_rate as f64).max(0.5);
    let pps = ((w - GUTTER) / duration).clamp(30.0, 1500.0);
    let (lo, hi) = app
        .with_edit(|e| {
            if e.notes.is_empty() {
                (48.0, 72.0)
            } else {
                let lo = e.notes.iter().map(|n| n.center + n.offset).fold(f64::MAX, f64::min) + shift - 3.0;
                let hi = e.notes.iter().map(|n| n.center + n.offset).fold(f64::MIN, f64::max) + shift + 3.0;
                (lo, hi)
            }
        })
        .unwrap_or((48.0, 72.0));
    let semis = (hi - lo).max(14.0);
    let ps = ((h - RULER) / semis).clamp(10.0, 36.0);
    let visible = (h - RULER) / ps;
    Some(RollView { px_per_sec: pps, scroll_sec: 0.0, px_per_semi: ps, top_midi: (lo + hi) / 2.0 + visible / 2.0 })
}

fn begin(st: &State, x: f64, y: f64) {
    let app = &st.app;
    let Some(v) = st.view.get() else { return };
    let Some(cs) = app.clip() else { return };
    let an = &cs.analysis;
    let geo = Geo { view: v };
    let (shift, _) = shift_and_mask(app, an);
    let hit = app
        .with_edit(|e| {
            let tm = e.time_map(an.length);
            let boxes = layout(e, an, &tm, &geo);
            hit_test(&boxes, x, y, &geo, an, &tm, shift)
        })
        .flatten();
    let tool = app.tool.get();
    let mode = match (&hit, tool) {
        _ if y < RULER => Drag::Seek,
        (Some(_), Tool::Split) => Drag::None,
        (Some(b), Tool::Time) => {
            let dl = (x - b.x0).abs();
            let dr = (x - b.x1).abs();
            if dl < EDGE_ZONE && dl <= dr {
                Drag::TimeLeft
            } else if dr < EDGE_ZONE {
                Drag::TimeRight
            } else {
                Drag::TimeBody
            }
        }
        (Some(_), _) => Drag::Pitch,
        (None, _) => Drag::Pan,
    };
    if mode == Drag::Seek {
        app.seek_timeline(geo.sec(x).max(0.0) as f32);
    }
    st.drag.replace(Some(DragState {
        mode,
        start: (x, y),
        last: (x, y),
        moved: false,
        started: false,
        hit,
        geo,
        start_offsets: Vec::new(),
        total_dy: 0.0,
    }));
}

fn update(st: &State, ox: f64, oy: f64) {
    let app = st.app.clone();
    let mut guard = st.drag.borrow_mut();
    let Some(d) = guard.as_mut() else { return };
    let pos = (d.start.0 + ox, d.start.1 + oy);
    let delta = (pos.0 - d.last.0, pos.1 - d.last.1);
    d.last = pos;
    if !d.moved && (ox * ox + oy * oy).sqrt() > SLOP {
        d.moved = true;
    }
    if !d.moved {
        return;
    }
    let Some(cs) = app.clip() else { return };
    let an = cs.analysis.clone();
    let sr = an.sample_rate as f64;
    match d.mode {
        Drag::Seek => app.seek_timeline(d.geo.sec(pos.0).max(0.0) as f32),
        Drag::Pan => {
            if let Some(mut v) = st.view.get() {
                v.scroll_sec = (v.scroll_sec - delta.0 / v.px_per_sec).max(-1.0);
                v.top_midi = (v.top_midi + delta.1 / v.px_per_semi).clamp(30.0, 110.0);
                st.view.set(Some(v));
            }
        }
        Drag::Pitch => {
            let Some(b) = d.hit.clone() else { return };
            if !d.started {
                d.started = true;
                let mut sel = app.selection();
                if !sel.contains(&b.index) {
                    app.select_only(b.index);
                    sel = [b.index].into_iter().collect();
                }
                app.begin_gesture();
                d.start_offsets = app
                    .with_edit(|e| sel.iter().filter_map(|i| e.notes.get(*i).map(|n| (*i, n.offset))).collect())
                    .unwrap_or_default();
                d.total_dy = 0.0;
            }
            d.total_dy += delta.1;
            let d_semi = -d.total_dy / d.geo.view.px_per_semi;
            let (shift, mask) = shift_and_mask(&app, &an);
            let snap = app.snap_to_scale.get();
            let offsets = d.start_offsets.clone();
            let mut label = None;
            drop(guard);
            app.modify(|e, _| {
                for (idx, start) in &offsets {
                    let Some(n) = e.notes.get_mut(*idx) else { continue };
                    let mut target = n.center + shift + start + d_semi;
                    if snap {
                        target = scales::nearest_allowed(target, mask) as f64;
                    }
                    n.offset = target - shift - n.center;
                    if *idx == b.index {
                        label = Some(pitch_label(n.center + n.offset + shift));
                    }
                }
            });
            st.hint.replace(label);
        }
        Drag::TimeLeft | Drag::TimeRight | Drag::TimeBody => {
            let Some(b) = d.hit.clone() else { return };
            if !d.started {
                d.started = true;
                app.begin_gesture();
            }
            let d_samples = delta.0 / d.geo.view.px_per_sec * sr;
            let mode = d.mode;
            drop(guard);
            app.modify(|e, a| {
                let s = b.note.in_start(a);
                let en = b.note.in_end(a);
                let keys: Vec<i32> = match mode {
                    Drag::TimeLeft => vec![s],
                    Drag::TimeRight => vec![en],
                    _ => e.anchors.range(s..=en).map(|(k, _)| *k).collect(),
                };
                e.move_anchors(&keys, d_samples, 0.01 * sr, a.length);
            });
        }
        Drag::None => {}
    }
}

fn end(st: &State) {
    let Some(d) = st.drag.replace(None) else { return };
    st.hint.replace(None);
    if d.moved {
        return;
    }
    let app = &st.app;
    match (&d.hit, d.mode) {
        (_, Drag::Seek) => {}
        (Some(b), _) if app.tool.get() == Tool::Split => {
            let Some(cs) = app.clip() else { return };
            let an = &cs.analysis;
            let frame = app
                .with_edit(|e| {
                    let tm = e.time_map(an.length);
                    (tm.to_input(d.geo.sec(d.start.0) * an.sample_rate as f64) / an.hop as f64).round() as i32
                })
                .unwrap_or(0);
            app.split_note(b.index, frame);
        }
        (Some(b), _) => app.toggle_select(b.index),
        (None, _) => app.clear_selection(),
    }
}

fn draw(st: &State, area: &gtk4::DrawingArea, cr: &gtk4::cairo::Context, w: f64, h: f64) {
    let app = &st.app;
    let pal: Palette = palette(app.settings.borrow().aperture_accent);
    pal.background.set(cr);
    cr.paint().ok();
    let (Some(v), Some(cs)) = (st.view.get(), app.clip()) else { return };
    let an = cs.analysis.clone();
    let geo = Geo { view: v };
    let (shift, mask) = shift_and_mask(app, &an);
    let sr = an.sample_rate as f64;

    // Pitch lanes: notes of the scale shaded, octaves ruled.
    let bot = v.top_midi - (h - RULER) / v.px_per_semi;
    for m in (bot.floor() as i32 - 1)..=(v.top_midi.ceil() as i32 + 1) {
        let top = geo.y(m as f64 + 0.5);
        let bottom = geo.y(m as f64 - 0.5);
        let allowed = scales::is_allowed(mask, m);
        let black = [1, 3, 6, 8, 10].contains(&m.rem_euclid(12));
        let tint = match (allowed, black) {
            (true, true) => Some(pal.accent.with_alpha(0.16)),
            (true, false) => Some(pal.accent.with_alpha(0.09)),
            (false, true) => Some(pal.text.with_alpha(0.05)),
            _ => None,
        };
        if let Some(t) = tint {
            t.set(cr);
            cr.rectangle(GUTTER, top, w - GUTTER, bottom - top);
            cr.fill().ok();
        }
        if m.rem_euclid(12) == 0 {
            pal.line.set(cr);
            cr.set_line_width(1.0);
            cr.move_to(GUTTER, bottom);
            cr.line_to(w, bottom);
            cr.stroke().ok();
        }
    }

    cr.save().ok();
    cr.rectangle(GUTTER, RULER, w - GUTTER, h - RULER);
    cr.clip();
    let selection = app.selection();
    let tool = app.tool.get();
    app.with_edit(|e| {
        let tm = e.time_map(an.length);
        let boxes = layout(e, &an, &tm, &geo);
        let loud = an.loud_level().max(1e-6);
        let px_per_frame = v.px_per_sec * an.hop as f64 / sr;
        let step = ((2.5 / px_per_frame.max(0.01)).ceil() as i32).max(1);
        let frames_n = an.frames();
        for b in &boxes {
            if b.x1 < GUTTER - 20.0 || b.x0 > w + 20.0 {
                continue;
            }
            let n = &b.note;
            let selected = selection.contains(&b.index);
            let edited = n.offset != 0.0 || n.drift != 1.0 || n.modulation != 1.0 || shift != 0.0;
            let mut frames = Vec::new();
            let mut f = n.f0;
            while f < n.f1 {
                frames.push(f);
                f += step;
            }
            frames.push(n.f1);

            // Ghost of the original contour.
            if edited {
                pal.dim.with_alpha(0.55).set(cr);
                cr.set_line_width(1.2);
                let mut first = true;
                for &fr in &frames {
                    if !an.voiced[fr as usize] {
                        continue;
                    }
                    let x = geo.x(tm.to_output((fr * an.hop) as f64) / sr);
                    let y = geo.y(an.midi(fr));
                    if first {
                        cr.move_to(x, y);
                        first = false;
                    } else {
                        cr.line_to(x, y);
                    }
                }
                cr.stroke().ok();
            }

            let mut top = Vec::with_capacity(frames.len());
            let mut bottom = Vec::with_capacity(frames.len());
            for &fr in &frames {
                let x = geo.x(tm.to_output((fr * an.hop) as f64) / sr);
                let y = geo.y(n.pitch_at(fr as f64) + shift);
                let i = fr as usize;
                let lv = (an.level[i.saturating_sub(1)] + an.level[i] + an.level[(i + 1).min(frames_n as usize - 1)]) / 3.0;
                let db = 20.0 * (lv / loud + 1e-6).log10();
                let thick = 4.0 + 10.0 * ((db + 36.0) / 36.0).clamp(0.0, 1.0) as f64;
                top.push((x, y - thick / 2.0));
                bottom.push((x, y + thick / 2.0));
            }
            cr.move_to(top[0].0, top[0].1);
            for p in &top {
                cr.line_to(p.0, p.1);
            }
            for p in bottom.iter().rev() {
                cr.line_to(p.0, p.1);
            }
            cr.close_path();
            if selected { pal.accent.set(cr) } else { pal.note.set(cr) };
            cr.fill_preserve().ok();
            if selected { pal.text.set(cr) } else { pal.note_edge.set(cr) };
            cr.set_line_width(1.2);
            cr.stroke().ok();

            if tool == Tool::Time {
                let yc = geo.y(n.center + n.offset + shift);
                pal.handle.set(cr);
                cr.set_line_width(3.0);
                for x in [b.x0, b.x1] {
                    cr.move_to(x, yc - 14.0);
                    cr.line_to(x, yc + 14.0);
                }
                cr.stroke().ok();
            }
        }
    });
    // Playhead.
    let px = geo.x(app.playhead_seconds() as f64);
    if px >= GUTTER && px <= w {
        pal.playhead.set(cr);
        cr.set_line_width(2.0);
        cr.move_to(px, RULER);
        cr.line_to(px, h);
        cr.stroke().ok();
    }
    cr.restore().ok();

    // Ruler.
    pal.panel.set(cr);
    cr.rectangle(0.0, 0.0, w, RULER);
    cr.fill().ok();
    let steps = [0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0];
    let tick = steps.iter().copied().find(|s| s * v.px_per_sec >= 70.0).unwrap_or(60.0);
    let mut t = (geo.sec(GUTTER) / tick).ceil() * tick;
    while geo.x(t) < w {
        let x = geo.x(t);
        pal.dim.set(cr);
        cr.set_line_width(1.0);
        cr.move_to(x, RULER - 6.0);
        cr.line_to(x, RULER);
        cr.stroke().ok();
        style::draw_text(cr, area, &format_time(t), 10.0, x + 3.0, 13.0);
        t += tick;
    }

    // Key gutter last, so notes scrolled off the left edge are hidden.
    pal.panel.set(cr);
    cr.rectangle(0.0, RULER, GUTTER, h - RULER);
    cr.fill().ok();
    for m in bot.floor() as i32..=v.top_midi.ceil() as i32 {
        let is_c = m.rem_euclid(12) == 0;
        if (v.px_per_semi > 11.0 && scales::is_allowed(mask, m)) || is_c {
            if is_c { pal.text.set(cr) } else { pal.dim.set(cr) };
            style::draw_text(cr, area, &scales::note_name(m), 10.0, 4.0, geo.y(m as f64) + 4.0);
        }
    }

    if let Some(hint) = st.hint.borrow().as_ref() {
        pal.accent.set(cr);
        style::draw_text(cr, area, hint, 16.0, GUTTER + 8.0, RULER + 22.0);
    }
}

//! The app's state and everything that changes it: settings and presets, the editor (clip, notes, undo), rendering,
//! playback, recording, speech and live voice. UI pages read from it and call into it; it tells them what changed.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glados_dsp::model::{Macro, Preset, VoiceParams, P};
use glados_dsp::{Analysis, Analyzer, Audio, Denoiser, Edit, FxChain, Note, NoteOps, OfflineRenderer, RenderSettings, SpeedWarp, Wav};
use gtk4::{gio, glib};

use crate::audio::decode;
use crate::audio::live::LiveController;
use crate::audio::player::ClipPlayer;
use crate::audio::recorder::Recorder;
use crate::settings::{pairs_to_params, ColorScheme, Settings};
use crate::tts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Params,
    Presets,
    /// A new clip (or none).
    Clip,
    /// The notes changed (pitch, timing, split, undo...).
    Edit,
    Selection,
    History,
    Busy,
    Recording,
    Live,
    Appearance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pitch,
    Time,
    Split,
}

pub struct ClipState {
    pub audio: Arc<Audio>,
    pub analysis: Arc<Analysis>,
    pub title: String,
    /// Apply the Tuning settings to every note now, and again whenever they change until a note is edited.
    pub auto_correct: bool,
    /// Recordings and imports get noise reduction; generated speech is clean already.
    pub noisy: bool,
}

struct Rendered {
    samples: Arc<Vec<f32>>,
    warp: Arc<SpeedWarp>,
    sample_rate: i32,
}

/// Noise-reduced samples for one clip and setting, so moving other sliders does not redo the noise reduction.
struct Cleaned {
    audio: Arc<Audio>,
    amount: f32,
    samples: Arc<Vec<f32>>,
}

/// The settings each kind of follow-up depends on; when they differ, that follow-up runs.
fn render_key(p: &VoiceParams) -> Vec<f32> {
    [
        P::Shift, P::Formant, P::FormantFollow, P::AutoReg, P::RegHz, P::Breath, P::RefA4, P::VibDepth, P::VibRate, P::Dry,
        P::Denoise, P::Tempo, P::SpeedRandom, P::HarmonyMix, P::HarmonySt,
    ]
    .iter()
    .map(|k| p.get(*k))
    .collect()
}

fn tuning_key(p: &VoiceParams) -> Vec<f32> {
    [
        P::Key, P::Scale, P::CustomMask, P::Retune, P::Strength, P::Monotone, P::MonoNote, P::MonoAuto, P::Shift, P::AutoReg,
        P::RegHz,
    ]
    .iter()
    .map(|k| p.get(*k))
    .collect()
}

fn detection_key(p: &VoiceParams) -> Vec<f32> {
    [P::Voicing, P::Fmin, P::Fmax].iter().map(|k| p.get(*k)).collect()
}

type Listener = Rc<dyn Fn(Change)>;
type WarpCache = Mutex<Option<(Vec<f32>, Arc<SpeedWarp>)>>;

pub struct App {
    pub settings: RefCell<Settings>,
    params: Cell<VoiceParams>,
    listeners: RefCell<Vec<Listener>>,
    save_pending: Cell<bool>,
    /// Messages for the user (shown as toasts), oldest first.
    pub messages: RefCell<VecDeque<String>>,

    pub player: ClipPlayer,
    pub live: RefCell<LiveController>,
    recorder: RefCell<Recorder>,

    // ---- editor ----
    clip: RefCell<Option<Rc<ClipState>>>,
    edit: RefCell<Option<Edit>>,
    edit_version: Cell<u64>,
    selection: RefCell<BTreeSet<usize>>,
    pub tool: Cell<Tool>,
    pub snap_to_scale: Cell<bool>,
    busy: RefCell<Option<String>>,
    pub seg_tolerance: Cell<f64>,
    pub seg_sensitivity: Cell<f64>,
    analysis_stale: Cell<bool>,
    undo: RefCell<Vec<Edit>>,
    redo: RefCell<Vec<Edit>>,
    last_coalesce: RefCell<Option<String>>,
    last_mutation: Cell<Option<Instant>>,
    /// Bumped by every load and render; a finished job only publishes if it is still the latest.
    load_gen: Cell<u64>,
    render_gen: Cell<u64>,
    load_cancel: RefCell<Arc<AtomicBool>>,
    render_cancel: RefCell<Arc<AtomicBool>>,
    reanalyse_gen: Cell<u64>,
    rendered: RefCell<Option<Rendered>>,
    warp_cache: Arc<WarpCache>,
    cleaned: Arc<Mutex<Option<Cleaned>>>,
}

impl App {
    pub fn new() -> Rc<App> {
        let settings = Settings::load();
        let params = settings.voice_params();
        let app = Rc::new(App {
            player: ClipPlayer::new(params),
            live: RefCell::new(LiveController::new(params)),
            recorder: RefCell::new(Recorder::default()),
            params: Cell::new(params),
            settings: RefCell::new(settings),
            listeners: RefCell::new(Vec::new()),
            save_pending: Cell::new(false),
            messages: RefCell::new(VecDeque::new()),
            clip: RefCell::new(None),
            edit: RefCell::new(None),
            edit_version: Cell::new(0),
            selection: RefCell::new(BTreeSet::new()),
            tool: Cell::new(Tool::Pitch),
            snap_to_scale: Cell::new(true),
            busy: RefCell::new(None),
            seg_tolerance: Cell::new(80.0),
            seg_sensitivity: Cell::new(50.0),
            analysis_stale: Cell::new(false),
            undo: RefCell::new(Vec::new()),
            redo: RefCell::new(Vec::new()),
            last_coalesce: RefCell::new(None),
            last_mutation: Cell::new(None),
            load_gen: Cell::new(0),
            render_gen: Cell::new(0),
            load_cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
            render_cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
            reanalyse_gen: Cell::new(0),
            rendered: RefCell::new(None),
            warp_cache: Arc::new(Mutex::new(None)),
            cleaned: Arc::new(Mutex::new(None)),
        });
        let out = app.settings.borrow().output_device.clone();
        app.player.set_device(out);
        app
    }

    pub fn connect(&self, f: impl Fn(Change) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    fn notify(&self, c: Change) {
        let ls: Vec<Listener> = self.listeners.borrow().clone();
        for l in ls {
            l(c);
        }
    }

    pub fn message(&self, text: impl Into<String>) {
        self.messages.borrow_mut().push_back(text.into());
        self.notify(Change::Busy);
    }

    /// Saves the settings shortly after the last change, so dragging a slider does not write the file 60 times.
    pub fn save_soon(self: &Rc<Self>) {
        if self.save_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            if let Some(app) = weak.upgrade() {
                app.save_pending.set(false);
                app.settings.borrow().save();
            }
        });
    }

    pub fn save_now(&self) {
        self.settings.borrow().save();
    }

    // ------------------------------------------------------------------ parameters and presets

    pub fn params(&self) -> VoiceParams {
        self.params.get()
    }

    pub fn set_param(self: &Rc<Self>, p: P, value: f32) {
        let next = self.params().with(p, value);
        self.set_params(next);
    }

    pub fn set_params(self: &Rc<Self>, next: VoiceParams) {
        let prev = self.params.replace(next);
        if prev == next {
            return;
        }
        self.settings.borrow_mut().set_voice_params(&next);
        self.save_soon();
        self.player.set_params(next);
        self.live.borrow().set_params(next);
        self.notify(Change::Params);
        if tuning_key(&prev) != tuning_key(&next) {
            self.on_tuning_changed();
        }
        if render_key(&prev) != render_key(&next) {
            self.schedule_render(80);
        }
        if detection_key(&prev) != detection_key(&next) {
            self.on_detection_settings_changed();
        }
    }

    pub fn reset_params(self: &Rc<Self>, ps: &[P]) {
        let next = ps.iter().fold(self.params(), |acc, p| acc.with(*p, p.spec().default));
        self.set_params(next);
    }

    pub fn set_macro(self: &Rc<Self>, m: &Macro, value: f32) {
        self.set_params(m.write(&self.params(), value));
    }

    /// Applies a built-in or saved preset. Recording settings (noise reduction, pitch range, voicing, gate) are kept.
    pub fn apply_preset(self: &Rc<Self>, name: &str) {
        let current = self.params();
        if let Some(p) = Preset::find(name) {
            self.set_params(p.apply_to(&current));
            return;
        }
        let saved = self.settings.borrow().user_presets.get(name).map(pairs_to_params);
        if let Some(p) = saved {
            self.set_params(p.with_input_from(&current));
        }
    }

    pub fn user_preset_names(&self) -> Vec<String> {
        self.settings.borrow().user_presets.keys().cloned().collect()
    }

    pub fn save_user_preset(self: &Rc<Self>, name: &str) -> bool {
        let clean = name.trim();
        if clean.is_empty() {
            return false;
        }
        if glados_dsp::model::PRESETS.iter().any(|p| p.name.eq_ignore_ascii_case(clean)) {
            self.message(format!("\"{clean}\" is a built-in preset name. Choose another."));
            return false;
        }
        let map: BTreeMap<String, f32> = self.params().to_pairs().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        self.settings.borrow_mut().user_presets.insert(clean.to_string(), map);
        self.save_now();
        self.notify(Change::Presets);
        true
    }

    pub fn delete_user_preset(self: &Rc<Self>, name: &str) {
        self.settings.borrow_mut().user_presets.remove(name);
        self.save_now();
        self.notify(Change::Presets);
    }

    /// Name of the built-in or saved preset that exactly matches the current settings, or None once anything changed.
    pub fn preset_name(&self) -> Option<String> {
        let p = self.params();
        if let Some(m) = Preset::matching(&p) {
            return Some(m.name.to_string());
        }
        let s = self.settings.borrow();
        s.user_presets.iter().find(|(_, m)| pairs_to_params(m).with_input_from(&p) == p).map(|(k, _)| k.clone())
    }

    pub fn set_simple_mode(self: &Rc<Self>, on: bool) {
        self.settings.borrow_mut().simple_mode = on;
        self.save_soon();
        self.notify(Change::Appearance);
    }

    pub fn set_color_scheme(self: &Rc<Self>, c: ColorScheme, aperture_accent: bool) {
        {
            let mut s = self.settings.borrow_mut();
            s.color_scheme = c;
            s.aperture_accent = aperture_accent;
        }
        self.save_soon();
        self.notify(Change::Appearance);
    }

    // ------------------------------------------------------------------ editor state

    pub fn clip(&self) -> Option<Rc<ClipState>> {
        self.clip.borrow().clone()
    }

    /// Runs `f` with the current notes, if there is a clip.
    pub fn with_edit<R>(&self, f: impl FnOnce(&Edit) -> R) -> Option<R> {
        self.edit.borrow().as_ref().map(f)
    }

    pub fn selection(&self) -> BTreeSet<usize> {
        self.selection.borrow().clone()
    }

    pub fn busy(&self) -> Option<String> {
        self.busy.borrow().clone()
    }

    fn set_busy(&self, b: Option<String>) {
        self.busy.replace(b);
        self.notify(Change::Busy);
    }

    pub fn analysis_stale(&self) -> bool {
        self.analysis_stale.get()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.borrow().is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.borrow().is_empty()
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.borrow().is_recording()
    }

    pub fn recording_seconds(&self) -> f32 {
        self.recorder.borrow().seconds()
    }

    pub fn select_only(&self, i: usize) {
        self.selection.replace([i].into_iter().collect());
        self.notify(Change::Selection);
    }

    pub fn toggle_select(&self, i: usize) {
        {
            let mut s = self.selection.borrow_mut();
            if !s.remove(&i) {
                s.insert(i);
            }
        }
        self.notify(Change::Selection);
    }

    pub fn select_all(&self) {
        let n = self.with_edit(|e| e.notes.len()).unwrap_or(0);
        self.selection.replace((0..n).collect());
        self.notify(Change::Selection);
    }

    pub fn clear_selection(&self) {
        self.selection.replace(BTreeSet::new());
        self.notify(Change::Selection);
    }

    fn push_undo(&self) {
        let Some(e) = self.edit.borrow().clone() else { return };
        let mut u = self.undo.borrow_mut();
        u.push(e);
        if u.len() > 100 {
            u.remove(0);
        }
        self.redo.borrow_mut().clear();
    }

    /// Call once when a drag starts, so the whole drag is a single undo step.
    pub fn begin_gesture(&self) {
        self.push_undo();
        self.last_coalesce.replace(None);
        self.notify(Change::History);
    }

    /// Applies `block` to the notes in place and re-renders. Use between [`begin_gesture`](Self::begin_gesture) and
    /// the end of a drag.
    pub fn modify(self: &Rc<Self>, block: impl FnOnce(&mut Edit, &Analysis)) {
        let Some(cs) = self.clip() else { return };
        {
            let mut e = self.edit.borrow_mut();
            let Some(e) = e.as_mut() else { return };
            block(e, &cs.analysis);
        }
        self.bump();
    }

    /// A discrete change (a correction, a slider move); consecutive moves with the same key share one undo step.
    pub fn mutate(self: &Rc<Self>, coalesce: Option<&str>, block: impl FnOnce(&mut Edit, &Analysis)) {
        let now = Instant::now();
        let same = coalesce.is_some()
            && self.last_coalesce.borrow().as_deref() == coalesce
            && self.last_mutation.get().is_some_and(|t| now - t < Duration::from_millis(800));
        if !same {
            self.push_undo();
        }
        self.last_coalesce.replace(coalesce.map(str::to_string));
        self.last_mutation.set(Some(now));
        self.modify(block);
        self.notify(Change::History);
    }

    pub fn undo(self: &Rc<Self>) {
        let Some(prev) = self.undo.borrow_mut().pop() else { return };
        if let Some(cur) = self.edit.replace(Some(prev)) {
            self.redo.borrow_mut().push(cur);
        }
        self.after_history_change();
    }

    pub fn redo(self: &Rc<Self>) {
        let Some(next) = self.redo.borrow_mut().pop() else { return };
        if let Some(cur) = self.edit.replace(Some(next)) {
            self.undo.borrow_mut().push(cur);
        }
        self.after_history_change();
    }

    fn after_history_change(self: &Rc<Self>) {
        let n = self.with_edit(|e| e.notes.len()).unwrap_or(0);
        self.selection.borrow_mut().retain(|i| *i < n);
        self.last_coalesce.replace(None);
        self.bump();
        self.notify(Change::History);
        self.notify(Change::Selection);
    }

    fn bump(self: &Rc<Self>) {
        self.edit_version.set(self.edit_version.get() + 1);
        self.notify(Change::Edit);
        self.schedule_render(40);
    }

    /// The notes an operation applies to: the selection, or every note when nothing is selected.
    fn target_indices(&self, e: &Edit) -> Vec<usize> {
        let sel = self.selection.borrow();
        if sel.is_empty() { (0..e.notes.len()).collect() } else { sel.iter().copied().filter(|i| *i < e.notes.len()).collect() }
    }

    fn targets<'a>(e: &'a mut Edit, idx: &'a [usize]) -> impl Iterator<Item = &'a mut Note> + 'a {
        e.notes.iter_mut().enumerate().filter(move |(i, _)| idx.contains(i)).map(|(_, n)| n)
    }

    pub fn apply_correction(self: &Rc<Self>, center: f64, drift: f64, modulation: f64) {
        let p = self.params();
        let idx = match self.with_edit(|e| self.target_indices(e)) {
            Some(i) => i,
            None => return,
        };
        self.mutate(None, |e, a| {
            let shift = NoteOps::global_shift(a, &p);
            let mask = NoteOps::scale_mask(&p);
            let mono = NoteOps::monotone_note(a, &p, shift, mask);
            NoteOps::correct(Self::targets(e, &idx), mask, center, drift, modulation, shift, p.bool(P::Monotone), mono);
        });
    }

    pub fn apply_voice_tuning(self: &Rc<Self>) {
        let p = self.params();
        let Some(idx) = self.with_edit(|e| self.target_indices(e)) else { return };
        self.mutate(None, |e, a| NoteOps::correct_to_settings(Self::targets(e, &idx), a, &p));
    }

    pub fn reset_notes(self: &Rc<Self>) {
        let Some(idx) = self.with_edit(|e| self.target_indices(e)) else { return };
        self.mutate(None, |e, _| {
            for n in Self::targets(e, &idx) {
                n.offset = 0.0;
                n.drift = 1.0;
                n.modulation = 1.0;
                n.formant = 0.0;
                n.gain_db = 0.0;
            }
        });
    }

    pub fn reset_timing(self: &Rc<Self>) {
        self.mutate(None, |e, _| {
            for (k, v) in e.anchors.iter_mut() {
                *v = *k as f64;
            }
        });
    }

    /// Runs `block` on a throwaway copy first; only if it succeeds is the real edit changed and an undo step
    /// recorded, so a refused operation leaves the undo and redo history untouched.
    fn try_mutate(self: &Rc<Self>, failure: &str, block: impl Fn(&mut Edit, &Analysis) -> bool) -> bool {
        let Some(cs) = self.clip() else { return false };
        let Some(mut probe) = self.edit.borrow().clone() else { return false };
        if !block(&mut probe, &cs.analysis) {
            self.message(failure);
            return false;
        }
        self.push_undo();
        self.last_coalesce.replace(None);
        if let Some(e) = self.edit.borrow_mut().as_mut() {
            block(e, &cs.analysis);
        }
        self.bump();
        self.notify(Change::History);
        true
    }

    pub fn split_note(self: &Rc<Self>, index: usize, frame: i32) {
        if self.try_mutate("That cut is too close to the edge of the note.", |e, a| NoteOps::split(e, a, index as i32, frame)) {
            self.selection.replace([index, index + 1].into_iter().collect());
            self.notify(Change::Selection);
        }
    }

    pub fn merge_selected(self: &Rc<Self>) {
        let idx: Vec<usize> = self.selection.borrow().iter().copied().collect();
        let Some(&first) = idx.first() else { return };
        let last = if idx.len() == 1 { first + 1 } else { *idx.last().unwrap() };
        let ok = self.try_mutate("Those notes are not next to each other.", |e, a| {
            let mut merged = false;
            for i in (first..last).rev() {
                merged = NoteOps::merge(e, a, i as i32) || merged;
            }
            merged
        });
        if ok {
            self.selection.replace([first].into_iter().collect());
            self.notify(Change::Selection);
        }
    }

    /// Re-runs pitch analysis, for when the voice range or sensitivity changed. Edits are lost.
    pub fn reanalyse(self: &Rc<Self>) {
        if let Some(cs) = self.clip() {
            self.load_audio(cs.audio.clone(), cs.title.clone(), cs.auto_correct, false, cs.noisy);
        }
    }

    pub fn resegment(self: &Rc<Self>) {
        let Some(cs) = self.clip() else { return };
        let e = NoteOps::segment(&cs.analysis, self.seg_tolerance.get(), self.seg_sensitivity.get() / 100.0);
        self.push_undo();
        self.edit.replace(Some(e));
        self.selection.replace(BTreeSet::new());
        self.bump();
        self.notify(Change::History);
        self.notify(Change::Selection);
    }

    /// A new preset or Tuning setting re-tunes the clip, as long as its notes have not been edited by hand.
    fn on_tuning_changed(self: &Rc<Self>) {
        let Some(cs) = self.clip() else { return };
        if !cs.auto_correct || self.can_undo() {
            return;
        }
        let p = self.params();
        self.modify(|e, a| NoteOps::correct_to_settings(e.notes.iter_mut(), a, &p));
    }

    fn on_detection_settings_changed(self: &Rc<Self>) {
        let Some(cs) = self.clip() else { return };
        if !self.can_undo() {
            // Nothing has been edited yet, so re-analysing loses nothing: do it automatically, once the slider rests.
            let gen = self.reanalyse_gen.get() + 1;
            self.reanalyse_gen.set(gen);
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Some(app) = weak.upgrade() {
                    if app.reanalyse_gen.get() == gen && app.clip().is_some_and(|c| Rc::ptr_eq(&c, &cs)) {
                        app.load_audio(cs.audio.clone(), cs.title.clone(), cs.auto_correct, false, cs.noisy);
                    }
                }
            });
        } else {
            self.analysis_stale.set(true);
            self.notify(Change::Clip);
        }
    }

    // ------------------------------------------------------------------ loading and rendering

    /// Analyses `audio`, splits it into notes and makes it the editor clip. A newer call cancels an older one still
    /// running, so overlapping loads can never interleave their results.
    pub fn load_audio(self: &Rc<Self>, audio: Arc<Audio>, title: String, auto_correct: bool, autoplay: bool, noisy: bool) {
        self.load_cancel.borrow().store(true, Ordering::Relaxed);
        self.render_cancel.borrow().store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.load_cancel.replace(cancel.clone());
        let gen = self.load_gen.get() + 1;
        self.load_gen.set(gen);
        self.player.stop();
        self.set_busy(Some("Analysing...".into()));

        let p = self.params();
        let tolerance = self.seg_tolerance.get();
        let sensitivity = self.seg_sensitivity.get() / 100.0;
        let cleaned = self.cleaned.clone();
        let warp_cache = self.warp_cache.clone();
        let weak = Rc::downgrade(self);
        let (tx, rx) = async_channel::unbounded::<f32>();
        glib::spawn_future_local(async move {
            let job_cancel = cancel.clone();
            let job_audio = audio.clone();
            let handle = gio::spawn_blocking(move || {
                let cancelled = || job_cancel.load(Ordering::Relaxed);
                let progress = |f: f32| {
                    let _ = tx.try_send(f);
                };
                let an = Analyzer::analyze(&job_audio.samples, job_audio.sample_rate, &p, Some(&progress), &cancelled, None);
                let mut e = NoteOps::segment(&an, tolerance, sensitivity);
                if auto_correct {
                    NoteOps::correct_to_settings(e.notes.iter_mut(), &an, &p);
                }
                let out = render(&job_audio, &an, &e, &p, noisy, &cleaned, &warp_cache, &cancelled);
                (an, e, out)
            });
            // Show analysis progress while the job runs.
            let progress_weak = weak.clone();
            let progress_gen = gen;
            glib::spawn_future_local(async move {
                while let Ok(f) = rx.recv().await {
                    let Some(app) = progress_weak.upgrade() else { break };
                    if app.load_gen.get() != progress_gen {
                        break;
                    }
                    app.busy.replace(Some(format!("Analysing {}%", (f * 100.0) as i32)));
                    app.notify(Change::Busy);
                }
            });
            let result = handle.await;
            let Some(app) = weak.upgrade() else { return };
            if app.load_gen.get() != gen || cancel.load(Ordering::Relaxed) {
                return;
            }
            app.set_busy(None);
            let Ok((an, e, out)) = result else {
                app.message("Could not analyse that audio.");
                return;
            };
            if e.notes.is_empty() {
                app.message("No pitched voice found. It will play back unchanged.");
            }
            app.clip.replace(Some(Rc::new(ClipState { audio, analysis: Arc::new(an), title, auto_correct, noisy })));
            app.analysis_stale.set(false);
            app.edit.replace(Some(e));
            app.selection.replace(BTreeSet::new());
            app.undo.borrow_mut().clear();
            app.redo.borrow_mut().clear();
            app.edit_version.set(app.edit_version.get() + 1);
            app.rendered.replace(None); // a new clip: nothing to carry the playhead over from
            app.publish(out);
            app.notify(Change::Clip);
            app.notify(Change::Edit);
            app.notify(Change::History);
            app.notify(Change::Selection);
            if autoplay {
                if let Err(err) = app.player.play(true) {
                    app.message(err);
                }
            }
        });
    }

    fn schedule_render(self: &Rc<Self>, delay_ms: u64) {
        let Some(cs) = self.clip() else { return };
        let Some(e) = self.edit.borrow().clone() else { return };
        let p = self.params();
        self.render_cancel.borrow().store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.render_cancel.replace(cancel.clone());
        let gen = self.render_gen.get() + 1;
        self.render_gen.set(gen);
        let cleaned = self.cleaned.clone();
        let warp_cache = self.warp_cache.clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(Duration::from_millis(delay_ms)).await;
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let (audio, analysis, noisy) = (cs.audio.clone(), cs.analysis.clone(), cs.noisy);
            let job_cancel = cancel.clone();
            let out = gio::spawn_blocking(move || {
                let cancelled = || job_cancel.load(Ordering::Relaxed);
                render(&audio, &analysis, &e, &p, noisy, &cleaned, &warp_cache, &cancelled)
            })
            .await;
            let Some(app) = weak.upgrade() else { return };
            // Only publish if this render is still the latest and for the clip on screen.
            if app.render_gen.get() != gen || cancel.load(Ordering::Relaxed) {
                return;
            }
            if !app.clip().is_some_and(|c| Rc::ptr_eq(&c, &cs)) {
                return;
            }
            if let Ok(out) = out {
                app.publish(out);
            }
        });
    }

    /// Hands a render to the player, keeping the playhead on the same spot of the timeline if the speed changed.
    fn publish(&self, r: Rendered) {
        let old_warp = self.rendered.borrow().as_ref().map(|o| o.warp.clone());
        let timeline = old_warp.as_ref().map(|w| w.to_timeline(self.player.position_seconds() as f64 * r.sample_rate as f64));
        self.player.set_clip(r.samples.clone(), r.sample_rate);
        let moved = match &old_warp {
            Some(old) => !Arc::ptr_eq(old, &r.warp) && !(old.is_identity() && r.warp.is_identity()),
            None => false,
        };
        if let (true, Some(t)) = (moved, timeline) {
            self.player.seek((r.warp.to_rendered(t) / r.sample_rate as f64) as f32);
        }
        self.rendered.replace(Some(r));
    }

    /// Playhead position on the editor's timeline, in seconds.
    pub fn playhead_seconds(&self) -> f32 {
        let pos = self.player.position_seconds();
        match (self.clip(), self.rendered.borrow().as_ref()) {
            (Some(cs), Some(r)) => {
                let sr = cs.audio.sample_rate as f64;
                (r.warp.to_timeline(pos as f64 * sr) / sr) as f32
            }
            _ => pos,
        }
    }

    /// Moves the playhead to `seconds` on the editor's timeline.
    pub fn seek_timeline(&self, seconds: f32) {
        match (self.clip(), self.rendered.borrow().as_ref()) {
            (Some(cs), Some(r)) => {
                let sr = cs.audio.sample_rate as f64;
                self.player.seek((r.warp.to_rendered(seconds as f64 * sr) / sr) as f32);
            }
            _ => self.player.seek(seconds),
        }
    }

    pub fn toggle_play(&self) {
        if let Err(e) = self.player.toggle() {
            self.message(e);
        }
    }

    // ------------------------------------------------------------------ recording, files, speech

    pub fn toggle_record(self: &Rc<Self>) {
        if self.is_recording() {
            self.finish_recording(None);
            return;
        }
        if self.busy().is_some() {
            return;
        }
        if self.live.borrow().is_running() {
            self.message("Stop live mode before recording.");
            return;
        }
        self.player.stop();
        let device = self.settings.borrow().input_device.clone();
        let result = self.recorder.borrow_mut().start(device.as_deref());
        match result {
            Ok(()) => self.notify(Change::Recording),
            Err(e) => self.message(e),
        }
    }

    /// Called by the UI's tick: stops by itself at the length cap.
    pub fn poll(self: &Rc<Self>) {
        self.player.poll();
        if self.is_recording() && self.recorder.borrow().is_capped() {
            self.finish_recording(Some(format!("Recording stopped at {} seconds.", crate::audio::recorder::MAX_SECONDS)));
        }
        let err = self.live.borrow().take_error();
        if let Some(err) = err {
            self.live.borrow_mut().stop();
            self.message(err);
            self.notify(Change::Live);
        }
    }

    pub fn recorder_level(&self) -> f32 {
        self.recorder.borrow().level()
    }

    fn finish_recording(self: &Rc<Self>, note: Option<String>) {
        let audio = self.recorder.borrow_mut().stop();
        self.notify(Change::Recording);
        match audio {
            None => self.message(note.unwrap_or_else(|| "Nothing was recorded.".into())),
            Some(a) => {
                if let Some(n) = note {
                    self.message(n);
                }
                self.load_audio(Arc::new(a), "Recording".into(), true, true, true);
            }
        }
    }

    pub fn import_file(self: &Rc<Self>, path: PathBuf) {
        if self.busy().is_some() {
            return;
        }
        self.set_busy(Some("Decoding...".into()));
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let job = path.clone();
            let result = gio::spawn_blocking(move || decode::decode(&job)).await;
            let Some(app) = weak.upgrade() else { return };
            app.set_busy(None);
            match result {
                Ok(Ok(d)) => {
                    if d.truncated {
                        app.message(format!("Only the first {} minutes were imported.", decode::MAX_SECONDS / 60));
                    }
                    let title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Imported audio".into());
                    app.load_audio(Arc::new(d.audio), title, true, true, true);
                }
                Ok(Err(e)) => app.message(format!("Could not read that file: {e}")),
                Err(_) => app.message("Could not read that file."),
            }
        });
    }

    pub fn export_wav(self: &Rc<Self>, path: PathBuf) {
        let Some(cs) = self.clip() else { return };
        let Some(samples) = self.rendered.borrow().as_ref().map(|r| r.samples.clone()) else { return };
        let p = self.params();
        self.set_busy(Some("Rendering...".into()));
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let sr = cs.audio.sample_rate;
            let result = gio::spawn_blocking(move || -> Result<(), String> {
                let mut out = samples.to_vec();
                out.resize(out.len() + (FxChain::tail_seconds(&p) * sr as f64) as usize, 0.0);
                let mut fx = FxChain::new(sr);
                fx.configure(&p);
                for chunk in out.chunks_mut(4096) {
                    fx.process(chunk);
                }
                let mut file = std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e| e.to_string())?);
                Wav::write(&mut file, &Audio { samples: out, sample_rate: sr }).map_err(|e| e.to_string())
            })
            .await;
            let Some(app) = weak.upgrade() else { return };
            app.set_busy(None);
            match result {
                Ok(Ok(())) => app.message("Saved."),
                Ok(Err(e)) => app.message(format!("Could not save: {e}")),
                Err(_) => app.message("Could not save."),
            }
        });
    }

    pub fn speak(self: &Rc<Self>, req: tts::Request, auto_tune: bool) {
        if req.text.trim().is_empty() || self.busy().is_some() {
            return;
        }
        self.set_busy(Some("Speaking...".into()));
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || tts::synthesize(&req)).await;
            let Some(app) = weak.upgrade() else { return };
            app.set_busy(None);
            match result {
                Ok(Ok(mut audio)) => {
                    let peak = audio.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    if peak > 1e-4 {
                        let g = 0.8 / peak;
                        audio.samples.iter_mut().for_each(|v| *v *= g);
                    }
                    app.load_audio(Arc::new(audio), "Speech".into(), auto_tune, true, false);
                }
                Ok(Err(e)) => app.message(e),
                Err(_) => app.message("Speech failed."),
            }
        });
    }

    // ------------------------------------------------------------------ live

    pub fn start_live(self: &Rc<Self>) {
        if self.is_recording() {
            self.finish_recording(None);
        }
        self.player.pause();
        let (input, output) = {
            let s = self.settings.borrow();
            (s.input_device.clone(), s.output_device.clone())
        };
        let result = self.live.borrow_mut().start(input.as_deref(), output.as_deref());
        if let Err(e) = result {
            self.message(e);
        }
        self.notify(Change::Live);
    }

    pub fn stop_live(&self) {
        self.live.borrow_mut().stop();
        self.notify(Change::Live);
    }

    pub fn set_devices(self: &Rc<Self>, input: Option<String>, output: Option<String>) {
        {
            let mut s = self.settings.borrow_mut();
            s.input_device = input;
            s.output_device = output.clone();
        }
        self.save_soon();
        self.player.set_device(output);
        if self.live.borrow().is_running() {
            self.start_live();
        }
    }
}

/// Noise reduction, speed map and PSOLA rendering for one clip. Runs off the UI thread.
#[allow(clippy::too_many_arguments)]
fn render(
    audio: &Arc<Audio>, an: &Analysis, e: &Edit, p: &VoiceParams, noisy: bool, cleaned: &Mutex<Option<Cleaned>>,
    warp_cache: &WarpCache, cancelled: &dyn Fn() -> bool,
) -> Rendered {
    let source = if noisy { clean(audio, p.get(P::Denoise), cleaned, cancelled) } else { Arc::new(audio.samples.clone()) };
    let shift = NoteOps::global_shift(an, p);
    let warp = speed_warp(audio, p, warp_cache);
    let settings = RenderSettings {
        pitch_shift: shift,
        formant_shift: NoteOps::formant_shift(p, shift),
        breath: p.get(P::Breath) / 100.0,
        vibrato_cents: p.get(P::VibDepth) as f64,
        vibrato_hz: p.get(P::VibRate) as f64,
        dry: p.get(P::Dry) / 100.0,
        harmony_mix: p.get(P::HarmonyMix) / 100.0,
        harmony_st: p.get(P::HarmonySt) as f64,
        warp: Some(warp.clone()),
    };
    let samples = OfflineRenderer::render(&source, an, e, &settings, cancelled);
    Rendered { samples: Arc::new(samples), warp, sample_rate: audio.sample_rate }
}

/// Noise-reduced samples, computed once per clip and setting. Pitch analysis keeps using the original audio.
fn clean(audio: &Arc<Audio>, amount: f32, cache: &Mutex<Option<Cleaned>>, cancelled: &dyn Fn() -> bool) -> Arc<Vec<f32>> {
    if let Some(c) = cache.lock().unwrap().as_ref() {
        if Arc::ptr_eq(&c.audio, audio) && c.amount == amount {
            return c.samples.clone();
        }
    }
    let y = Arc::new(Denoiser::process(&audio.samples, audio.sample_rate, amount / 100.0, cancelled));
    if !cancelled() {
        *cache.lock().unwrap() = Some(Cleaned { audio: audio.clone(), amount, samples: y.clone() });
    }
    y
}

/// The speed map for `audio` at the current Speed settings. The same settings give back the same instance, so a
/// re-render after a note edit does not look like a speed change to `publish` (which would seek and reset effects).
fn speed_warp(audio: &Audio, p: &VoiceParams, cache: &WarpCache) -> Arc<SpeedWarp> {
    let key = vec![audio.samples.len() as f32, audio.sample_rate as f32, p.get(P::Tempo), p.get(P::SpeedRandom)];
    let mut c = cache.lock().unwrap();
    if let Some((k, w)) = c.as_ref() {
        if *k == key {
            return w.clone();
        }
    }
    let n = audio.samples.len() as i32;
    let w = Arc::new(SpeedWarp::build(n, audio.sample_rate, p.get(P::Tempo) as f64 / 100.0, p.get(P::SpeedRandom) as f64 / 100.0, n as i64));
    *c = Some((key, w.clone()));
    w
}

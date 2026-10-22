use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

use crate::analysis::Analysis;
use crate::kmath::round_f32;
use crate::model::{VoiceParams, P};
use crate::scales::{self, pitch};

/// One blob in the editor: a stretch of voiced frames treated as a single sung or spoken note.
///
/// The original pitch is split into a centre, a slow drift and a fast modulation (vibrato and
/// jitter). Each part can be scaled independently, which is what the macros and tools do.
#[derive(Clone, Debug)]
pub struct Note {
    pub f0: i32,
    pub f1: i32,
    /// Semitones added to [`Note::center`].
    pub offset: f64,
    pub drift: f32,
    /// Scale of the fast modulation (Kotlin `mod`).
    pub modulation: f32,

    /// Formant shift in semitones, on top of the global one.
    pub formant: f32,
    pub gain_db: f32,

    /// Average pitch (MIDI) of the note's voiced frames, set by [`Note::refresh`].
    pub center: f64,
    slow: Vec<f64>,
    fast: Vec<f64>,
}

impl Note {
    pub fn new(f0: i32, f1: i32) -> Self {
        Note {
            f0, f1, offset: 0.0, drift: 1.0, modulation: 1.0, formant: 0.0, gain_db: 0.0, center: 0.0,
            slow: Vec::new(), fast: Vec::new(),
        }
    }

    pub fn frames(&self) -> i32 {
        self.f1 - self.f0 + 1
    }

    /// Recomputes centre, drift and modulation from the analysis. Call after changing the frame range.
    pub fn refresh(&mut self, an: &Analysis) {
        let n = self.frames().max(0) as usize;
        let mut m = vec![f64::NAN; n];
        let mut sum = 0.0;
        let mut cnt = 0;
        for (i, mi) in m.iter_mut().enumerate() {
            let fr = self.f0 + i as i32;
            if fr >= 0 && fr < an.frames() && an.voiced[fr as usize] {
                *mi = an.midi(fr);
                sum += *mi;
                cnt += 1;
            }
        }
        self.center = if cnt > 0 { sum / cnt as f64 } else { 60.0 };
        let dev: Vec<f64> = m.iter().map(|v| if v.is_nan() { 0.0 } else { v - self.center }).collect();
        let mut slow = vec![0.0; n];
        let radius = 10;
        for (i, si) in slow.iter_mut().enumerate() {
            let mut s = 0.0;
            let mut c = 0;
            for d in &dev[i.saturating_sub(radius)..=(n - 1).min(i + radius)] {
                s += d;
                c += 1;
            }
            *si = s / c as f64;
        }
        self.fast = dev.iter().zip(&slow).map(|(d, s)| d - s).collect();
        self.slow = slow;
    }

    /// Output pitch in MIDI at a fractional frame, before the global shift.
    pub fn pitch_at(&self, frame: f64) -> f64 {
        let frames = self.frames();
        let k = (frame - self.f0 as f64).clamp(0.0, (frames - 1) as f64);
        let i = k.floor() as i32;
        let j = (i + 1).min(frames - 1);
        let t = k - i as f64;
        let (i, j) = (i as usize, j as usize);
        let s = self.slow[i] * (1.0 - t) + self.slow[j] * t;
        let f = self.fast[i] * (1.0 - t) + self.fast[j] * t;
        self.center + self.offset + self.drift as f64 * s + self.modulation as f64 * f
    }

    /// Input span in samples. A frame covers [i * hop - hop/2, (i + 1) * hop - hop/2). The end is written as the
    /// start of the *next* frame so that touching notes share exactly one boundary for any hop, including the odd
    /// hop that a 44.1 kHz clip gets.
    pub fn in_start(&self, an: &Analysis) -> i32 {
        0.max(self.f0 * an.hop - an.hop / 2)
    }

    pub fn in_end(&self, an: &Analysis) -> i32 {
        an.length.min((self.f1 + 1) * an.hop - an.hop / 2)
    }
}

/// Piecewise-linear mapping between input and output time, defined by anchor points.
#[derive(Clone, Debug)]
pub struct TimeMap {
    in_x: Vec<f64>,
    out_x: Vec<f64>,
}

impl TimeMap {
    pub fn new(in_x: Vec<f64>, out_x: Vec<f64>) -> Self {
        TimeMap { in_x, out_x }
    }

    pub fn to_input(&self, out: f64) -> f64 {
        interp(&self.out_x, &self.in_x, out)
    }

    pub fn to_output(&self, inp: f64) -> f64 {
        interp(&self.in_x, &self.out_x, inp)
    }

    pub fn from(anchors: &BTreeMap<i32, f64>, length: i32) -> TimeMap {
        let mut pts = anchors.clone();
        pts.entry(0).or_insert(0.0);
        pts.entry(length).or_insert(length as f64);
        let mut xs = Vec::with_capacity(pts.len());
        let mut ys: Vec<f64> = Vec::with_capacity(pts.len());
        for (i, (k, v)) in pts.into_iter().enumerate() {
            xs.push(k as f64);
            ys.push(if i > 0 { v.max(ys[i - 1] + 1e-3) } else { v });
        }
        TimeMap::new(xs, ys)
    }
}

fn interp(xs: &[f64], ys: &[f64], v: f64) -> f64 {
    if v <= xs[0] {
        return ys[0] + (v - xs[0]);
    }
    let last = xs.len() - 1;
    if v >= xs[last] {
        return ys[last] + (v - xs[last]);
    }
    let mut lo = 0;
    let mut hi = last;
    while hi - lo > 1 {
        let mid = (lo + hi) >> 1;
        if xs[mid] <= v {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let span = xs[hi] - xs[lo];
    if span <= 0.0 { ys[lo] } else { ys[lo] + (ys[hi] - ys[lo]) * (v - xs[lo]) / span }
}

/// Everything the user can change in the editor. Clone it for undo.
#[derive(Clone, Debug)]
pub struct Edit {
    pub notes: Vec<Note>,
    pub anchors: BTreeMap<i32, f64>,
}

impl Edit {
    pub fn time_map(&self, length: i32) -> TimeMap {
        TimeMap::from(&self.anchors, length)
    }

    /// True if any note has been changed from its original state.
    pub fn is_edited(&self, _an: &Analysis) -> bool {
        self.notes
            .iter()
            .any(|n| n.offset != 0.0 || n.drift != 1.0 || n.modulation != 1.0 || n.formant != 0.0 || n.gain_db != 0.0)
            || self.anchors.iter().any(|(k, v)| (*k as f64 - v).abs() > 0.5)
    }

    /// Moves the anchors at `keys` (which should be contiguous) by `delta` output samples, clamped so
    /// order is kept, nothing gets shorter than `min_gap`, and no neighbouring segment is stretched
    /// beyond 4x or squeezed below 1/4. The first and last sample of the clip never move.
    /// Returns the delta actually applied.
    pub fn move_anchors(&mut self, keys: impl IntoIterator<Item = impl Borrow<i32>>, delta: f64, min_gap: f64, length: i32) -> f64 {
        let mut moved: Vec<i32> = keys
            .into_iter()
            .map(|k| *k.borrow())
            .filter(|k| *k >= 1 && *k < length && self.anchors.contains_key(k))
            .collect();
        moved.sort();
        if moved.is_empty() {
            return 0.0;
        }
        let anchors = &mut self.anchors;

        let first = moved[0];
        let below_key = anchors.range(..first).next_back().map(|(k, _)| *k).unwrap_or(0);
        let below_out = anchors.get(&below_key).copied().unwrap_or(0.0);
        let span_below = (first - below_key) as f64;
        let mut lo = below_out + min_gap.max(0.25 * span_below) - anchors[&first];
        let mut hi = below_out + 4.0 * span_below - anchors[&first];

        let last = moved[moved.len() - 1];
        let above_key = anchors.range((Excluded(last), Unbounded)).next().map(|(k, _)| *k).unwrap_or(length);
        let above_out = anchors.get(&above_key).copied().unwrap_or(length as f64);
        let span_above = (above_key - last) as f64;
        lo = lo.max(above_out - 4.0 * span_above - anchors[&last]);
        hi = hi.min(above_out - min_gap.max(0.25 * span_above) - anchors[&last]);

        let applied = if lo > hi { 0.0 } else { delta.clamp(lo, hi) };
        for k in moved {
            let v = anchors[&k] + applied;
            anchors.insert(k, v);
        }
        applied
    }
}

pub struct NoteOps;

impl NoteOps {
    /// Splits each voiced stretch into notes at pitch steps and at loudness valleys (syllables).
    ///
    /// * `pitch_tol_cents` – how far pitch may wander from a note's average before a new note starts (Kotlin default 80)
    /// * `sensitivity` – 0..1; higher splits at shallower loudness dips (Kotlin default 0.5)
    pub fn segment(an: &Analysis, pitch_tol_cents: f64, sensitivity: f64) -> Edit {
        let frames = an.frames();
        let mut notes: Vec<Note> = Vec::new();
        let lvl: Vec<f64> = an.level.iter().map(|l| 20.0 * (*l as f64 + 1e-6).log10()).collect();
        let mut smooth = lvl.clone();
        for _ in 0..2 {
            let src = smooth.clone();
            for i in 0..smooth.len() {
                let mut s = 0.0;
                let mut c = 0;
                for v in &src[i.saturating_sub(2)..=(smooth.len() - 1).min(i + 2)] {
                    s += v;
                    c += 1;
                }
                smooth[i] = s / c as f64;
            }
        }
        let depth_db = 12.0 - 8.0 * sensitivity.clamp(0.0, 1.0);

        let mut i = 0;
        while i < frames {
            if !an.voiced[i as usize] {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < frames && an.voiced[j as usize] {
                j += 1;
            }
            let i0 = i;
            let i1 = j - 1;
            let mut starts = BTreeSet::new();
            starts.insert(i0);

            // Pitch jumps. A smooth glide (a rising question, a falling phrase end) is one note, so compare each
            // frame with where the recent trend predicts it, not with the note's average. A frame that lands far
            // from that prediction, twice in a row, is a new note.
            let mut off = 0;
            let mut off_start = -1;
            for k in i0 + 4..=i1 {
                let predicted = an.midi(k - 1) + (an.midi(k - 1) - an.midi(k - 4)) / 3.0;
                if (an.midi(k) - predicted).abs() * 100.0 > pitch_tol_cents {
                    if off == 0 {
                        off_start = k;
                    }
                    off += 1;
                    if off >= 2 {
                        starts.insert(off_start);
                        off = 0;
                    }
                } else {
                    off = 0;
                }
            }

            // Loudness valleys.
            let sm = |q: i32| smooth[q as usize];
            let mut k = i0 + 3;
            while k <= i1 - 3 {
                let mut left = sm(k);
                for q in i0.max(k - 12)..=k {
                    left = left.max(sm(q));
                }
                let mut right = sm(k);
                for q in k..=i1.min(k + 12) {
                    right = right.max(sm(q));
                }
                let is_min = sm(k) <= sm(k - 1) && sm(k) < sm(k + 1);
                if is_min && left.min(right) - sm(k) >= depth_db {
                    starts.insert(k);
                    k += 8;
                } else {
                    k += 1;
                }
            }

            // Build notes, folding anything under four frames into its predecessor.
            let list: Vec<i32> = starts.into_iter().collect();
            let mut bounds: Vec<[i32; 2]> = Vec::new();
            for n in 0..list.len() {
                let a = list[n];
                let b = if n + 1 < list.len() { list[n + 1] - 1 } else { i1 };
                if !bounds.is_empty() && b - a + 1 < 4 {
                    bounds.last_mut().unwrap()[1] = b;
                } else {
                    bounds.push([a, b]);
                }
            }
            if bounds.len() > 1 && bounds[0][1] - bounds[0][0] + 1 < 4 {
                bounds[1][0] = bounds[0][0];
                bounds.remove(0);
            }
            for b in bounds {
                let mut note = Note::new(b[0], b[1]);
                note.refresh(an);
                notes.push(note);
            }
            i = j;
        }
        let anchors = NoteOps::anchors_for(an, &notes);
        Edit { notes, anchors }
    }

    pub fn anchors_for(an: &Analysis, notes: &[Note]) -> BTreeMap<i32, f64> {
        let mut anchors = BTreeMap::new();
        for n in notes {
            anchors.insert(n.in_start(an), n.in_start(an) as f64);
            anchors.insert(n.in_end(an), n.in_end(an) as f64);
        }
        anchors
    }

    /// Splits `index` so that a new note starts at `frame`. Returns false if the cut is too close to an edge.
    pub fn split(edit: &mut Edit, an: &Analysis, index: i32, frame: i32) -> bool {
        let idx = index as usize;
        let n = &mut edit.notes[idx];
        if frame - n.f0 < 3 || n.f1 - frame < 2 {
            return false;
        }
        let target = n.center + n.offset;
        let mut right = n.clone();
        right.f0 = frame;
        n.f1 = frame - 1;
        n.refresh(an);
        right.refresh(an);
        // refresh recomputes each half's centre; keep both halves on the pitch the note was edited to.
        n.offset = target - n.center;
        right.offset = target - right.center;
        let key = right.in_start(an);
        edit.notes.insert(idx + 1, right);
        if !edit.anchors.contains_key(&key) {
            let v = edit.time_map(an.length).to_output(key as f64);
            edit.anchors.insert(key, v);
        }
        true
    }

    /// Joins `index` with the following note if they touch (or nearly).
    pub fn merge(edit: &mut Edit, an: &Analysis, index: i32) -> bool {
        if index < 0 || index + 1 >= edit.notes.len() as i32 {
            return false;
        }
        let idx = index as usize;
        let b = edit.notes[idx + 1].clone();
        let a = &mut edit.notes[idx];
        if b.f0 - a.f1 > 3 {
            return false;
        }
        let touching = a.in_end(an) == b.in_start(an);
        let boundary = b.in_start(an);
        let target = a.center + a.offset;
        a.f1 = b.f1;
        a.refresh(an);
        a.offset = target - a.center; // the joined note keeps the first note's edited pitch
        edit.notes.remove(idx + 1);
        // Notes that shared a boundary move as one once joined, so that anchor goes away.
        if touching {
            edit.anchors.remove(&boundary);
        }
        true
    }

    /// Melodyne-style correction. For each note: `center` 0..1 pulls it toward the nearest allowed note,
    /// `drift` and `modulation` 0..1 remove that fraction of slow drift and fast modulation.
    #[allow(clippy::too_many_arguments)]
    pub fn correct<'a>(
        notes: impl IntoIterator<Item = &'a mut Note>, mask: i32, center: f64, drift: f64, modulation: f64,
        global_shift: f64, monotone: bool, monotone_note: i32,
    ) {
        for n in notes {
            let shifted = n.center + global_shift;
            let target = if monotone { monotone_note } else { scales::nearest_allowed(shifted, mask) };
            n.offset = (target as f64 - shifted) * center;
            n.drift = (1.0 - drift) as f32;
            n.modulation = (1.0 - modulation) as f32;
        }
    }

    /// Applies the Tuning settings from the Voice tab (key, scale, strength, retune speed, monotone) to `notes`.
    pub fn correct_to_settings<'a>(notes: impl IntoIterator<Item = &'a mut Note>, an: &Analysis, p: &VoiceParams) {
        let shift = NoteOps::global_shift(an, p);
        let strength = p.get(P::Strength) as f64 / 100.0;
        let smoothing = (p.get(P::Retune) as f64 / 300.0).clamp(0.0, 1.0);
        let flatten = strength * (1.0 - smoothing);
        let mask = NoteOps::scale_mask(p);
        let mono = NoteOps::monotone_note(an, p, shift, mask);
        NoteOps::correct(notes, mask, strength, flatten, flatten, shift, p.bool(P::Monotone), mono);
    }

    pub fn scale_mask(p: &VoiceParams) -> i32 {
        scales::mask(p.int(P::Scale), p.int(P::Key), round_f32(p.get(P::CustomMask)))
    }

    /// The note Monotone holds: the chosen one, or the allowed note nearest the clip's average pitch after
    /// `global_shift`. Pass [`NoteOps::scale_mask`] as `mask` for the Kotlin default.
    pub fn monotone_note(an: &Analysis, p: &VoiceParams, global_shift: f64, mask: i32) -> i32 {
        let med = an.median_midi();
        if !p.bool(P::MonoAuto) || med.is_nan() {
            p.int(P::MonoNote)
        } else {
            scales::nearest_allowed(med + global_shift, mask)
        }
    }

    /// Global shift in semitones: manual shift plus register matching.
    pub fn global_shift(an: &Analysis, p: &VoiceParams) -> f64 {
        let mut shift = p.get(P::Shift) as f64;
        if p.bool(P::AutoReg) {
            let med = an.median_midi();
            if !med.is_nan() {
                shift += (pitch::hz_to_midi(p.get(P::RegHz) as f64, an.a4) - med).clamp(-14.0, 14.0);
            }
        }
        shift
    }

    /// Global formant shift in semitones: the setting plus its share of the pitch shift.
    pub fn formant_shift(p: &VoiceParams, global_shift: f64) -> f64 {
        (p.get(P::Formant) as f64 + p.get(P::FormantFollow) as f64 / 100.0 * global_shift).clamp(-12.0, 12.0)
    }
}

package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import java.util.TreeMap
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * One blob in the editor: a stretch of voiced frames treated as a single sung or spoken note.
 *
 * The original pitch is split into a centre, a slow drift and a fast modulation (vibrato and
 * jitter). Each part can be scaled independently, which is what the macros and tools do.
 */
class Note(var f0: Int, var f1: Int) {
    /** Semitones added to [center]. */
    var offset = 0.0
    var drift = 1f
    var mod = 1f

    /** Formant shift in semitones, on top of the global one. */
    var formant = 0f
    var gainDb = 0f

    var center = 0.0
        private set
    private var slow = DoubleArray(0)
    private var fast = DoubleArray(0)

    val frames: Int get() = f1 - f0 + 1

    /** Recomputes centre, drift and modulation from the analysis. Call after changing the frame range. */
    fun refresh(an: Analysis) {
        val n = frames
        val m = DoubleArray(n) { Double.NaN }
        var sum = 0.0
        var cnt = 0
        for (i in 0 until n) {
            val fr = f0 + i
            if (fr in 0 until an.frames && an.voiced[fr]) {
                m[i] = an.midi(fr)
                sum += m[i]
                cnt++
            }
        }
        center = if (cnt > 0) sum / cnt else 60.0
        val dev = DoubleArray(n) { if (m[it].isNaN()) 0.0 else m[it] - center }
        slow = DoubleArray(n)
        val radius = 10
        for (i in 0 until n) {
            var s = 0.0
            var c = 0
            for (k in max(0, i - radius)..min(n - 1, i + radius)) {
                s += dev[k]
                c++
            }
            slow[i] = s / c
        }
        fast = DoubleArray(n) { dev[it] - slow[it] }
    }

    /** Output pitch in MIDI at a fractional frame, before the global shift. */
    fun pitchAt(frame: Double): Double {
        val k = (frame - f0).coerceIn(0.0, (frames - 1).toDouble())
        val i = floor(k).toInt()
        val j = min(i + 1, frames - 1)
        val t = k - i
        val s = slow[i] * (1 - t) + slow[j] * t
        val f = fast[i] * (1 - t) + fast[j] * t
        return center + offset + drift * s + mod * f
    }

    fun copy(): Note = Note(f0, f1).also {
        it.offset = offset; it.drift = drift; it.mod = mod; it.formant = formant; it.gainDb = gainDb
        it.center = center; it.slow = slow; it.fast = fast
    }

    /**
     * Input span in samples. A frame covers [i * hop - hop/2, (i + 1) * hop - hop/2). The end is written as the
     * start of the *next* frame so that touching notes share exactly one boundary for any hop, including the odd
     * hop that a 44.1 kHz clip gets.
     */
    fun inStart(an: Analysis): Int = max(0, f0 * an.hop - an.hop / 2)
    fun inEnd(an: Analysis): Int = min(an.length, (f1 + 1) * an.hop - an.hop / 2)
}

/** Piecewise-linear mapping between input and output time, defined by anchor points. */
class TimeMap(private val inX: DoubleArray, private val outX: DoubleArray) {
    fun toInput(out: Double): Double = interp(outX, inX, out)
    fun toOutput(inp: Double): Double = interp(inX, outX, inp)

    private fun interp(xs: DoubleArray, ys: DoubleArray, v: Double): Double {
        if (v <= xs[0]) return ys[0] + (v - xs[0])
        val last = xs.size - 1
        if (v >= xs[last]) return ys[last] + (v - xs[last])
        var lo = 0
        var hi = last
        while (hi - lo > 1) {
            val mid = (lo + hi) ushr 1
            if (xs[mid] <= v) lo = mid else hi = mid
        }
        val span = xs[hi] - xs[lo]
        return if (span <= 0.0) ys[lo] else ys[lo] + (ys[hi] - ys[lo]) * (v - xs[lo]) / span
    }

    companion object {
        fun from(anchors: Map<Int, Double>, length: Int): TimeMap {
            val pts = TreeMap<Int, Double>(anchors)
            pts.putIfAbsent(0, 0.0)
            pts.putIfAbsent(length, length.toDouble())
            val xs = DoubleArray(pts.size)
            val ys = DoubleArray(pts.size)
            var i = 0
            for ((k, v) in pts) {
                xs[i] = k.toDouble()
                ys[i] = if (i > 0) max(v, ys[i - 1] + 1e-3) else v
                i++
            }
            return TimeMap(xs, ys)
        }
    }
}

/** Everything the user can change in the editor. Deep-copyable for undo. */
class Edit(val notes: MutableList<Note>, val anchors: TreeMap<Int, Double>) {
    fun copy(): Edit = Edit(notes.mapTo(ArrayList()) { it.copy() }, TreeMap(anchors))

    fun timeMap(length: Int): TimeMap = TimeMap.from(anchors, length)

    /** True if any note has been changed from its original state. */
    fun isEdited(an: Analysis): Boolean =
        notes.any { it.offset != 0.0 || it.drift != 1f || it.mod != 1f || it.formant != 0f || it.gainDb != 0f } ||
            anchors.any { (k, v) -> abs(k - v) > 0.5 }

    /**
     * Moves the anchors at [keys] (which should be contiguous) by [delta] output samples, clamped so
     * order is kept, nothing gets shorter than [minGap], and no neighbouring segment is stretched
     * beyond 4x or squeezed below 1/4. The first and last sample of the clip never move.
     * @return the delta actually applied
     */
    fun moveAnchors(keys: Collection<Int>, delta: Double, minGap: Double, length: Int): Double {
        val moved = keys.filter { it in 1 until length && anchors.containsKey(it) }.sorted()
        if (moved.isEmpty()) return 0.0

        val first = moved.first()
        val belowKey = anchors.lowerKey(first) ?: 0
        val belowOut = anchors[belowKey] ?: 0.0
        val spanBelow = (first - belowKey).toDouble()
        var lo = belowOut + max(minGap, 0.25 * spanBelow) - anchors.getValue(first)
        var hi = belowOut + 4.0 * spanBelow - anchors.getValue(first)

        val last = moved.last()
        val aboveKey = anchors.higherKey(last) ?: length
        val aboveOut = anchors[aboveKey] ?: length.toDouble()
        val spanAbove = (aboveKey - last).toDouble()
        lo = max(lo, aboveOut - 4.0 * spanAbove - anchors.getValue(last))
        hi = min(hi, aboveOut - max(minGap, 0.25 * spanAbove) - anchors.getValue(last))

        val applied = if (lo > hi) 0.0 else delta.coerceIn(lo, hi)
        for (k in moved) anchors[k] = anchors.getValue(k) + applied
        return applied
    }
}

object NoteOps {
    /**
     * Splits each voiced stretch into notes at pitch steps and at loudness valleys (syllables).
     * @param pitchTolCents how far pitch may wander from a note's average before a new note starts
     * @param sensitivity 0..1; higher splits at shallower loudness dips
     */
    fun segment(an: Analysis, pitchTolCents: Double = 80.0, sensitivity: Double = 0.5): Edit {
        val notes = ArrayList<Note>()
        val lvl = DoubleArray(an.frames) { 20 * log10(an.level[it] + 1e-6) }
        val smooth = lvl.copyOf()
        repeat(2) {
            val src = smooth.copyOf()
            for (i in smooth.indices) {
                var s = 0.0
                var c = 0
                for (k in max(0, i - 2)..min(smooth.size - 1, i + 2)) { s += src[k]; c++ }
                smooth[i] = s / c
            }
        }
        val depthDb = 12.0 - 8.0 * sensitivity.coerceIn(0.0, 1.0)

        var i = 0
        while (i < an.frames) {
            if (!an.voiced[i]) { i++; continue }
            var j = i
            while (j < an.frames && an.voiced[j]) j++
            val i0 = i
            val i1 = j - 1
            val starts = sortedSetOf(i0)

            // Pitch jumps. A smooth glide (a rising question, a falling phrase end) is one note, so compare each
            // frame with where the recent trend predicts it, not with the note's average. A frame that lands far
            // from that prediction, twice in a row, is a new note.
            var off = 0
            var offStart = -1
            for (k in i0 + 4..i1) {
                val predicted = an.midi(k - 1) + (an.midi(k - 1) - an.midi(k - 4)) / 3.0
                if (abs(an.midi(k) - predicted) * 100 > pitchTolCents) {
                    if (off == 0) offStart = k
                    off++
                    if (off >= 2) {
                        starts += offStart
                        off = 0
                    }
                } else {
                    off = 0
                }
            }

            // Loudness valleys.
            var k = i0 + 3
            while (k <= i1 - 3) {
                var left = smooth[k]
                for (q in max(i0, k - 12)..k) left = max(left, smooth[q])
                var right = smooth[k]
                for (q in k..min(i1, k + 12)) right = max(right, smooth[q])
                val isMin = smooth[k] <= smooth[k - 1] && smooth[k] < smooth[k + 1]
                if (isMin && min(left, right) - smooth[k] >= depthDb) {
                    starts += k
                    k += 8
                } else {
                    k++
                }
            }

            // Build notes, folding anything under four frames into its predecessor.
            val list = starts.toList()
            val bounds = ArrayList<IntArray>()
            for (n in list.indices) {
                val a = list[n]
                val b = if (n + 1 < list.size) list[n + 1] - 1 else i1
                if (bounds.isNotEmpty() && b - a + 1 < 4) bounds.last()[1] = b else bounds += intArrayOf(a, b)
            }
            if (bounds.size > 1 && bounds.first()[1] - bounds.first()[0] + 1 < 4) {
                bounds[1][0] = bounds[0][0]
                bounds.removeAt(0)
            }
            for (b in bounds) notes += Note(b[0], b[1]).also { it.refresh(an) }
            i = j
        }
        return Edit(notes, anchorsFor(an, notes))
    }

    fun anchorsFor(an: Analysis, notes: List<Note>): TreeMap<Int, Double> {
        val anchors = TreeMap<Int, Double>()
        for (n in notes) {
            anchors[n.inStart(an)] = n.inStart(an).toDouble()
            anchors[n.inEnd(an)] = n.inEnd(an).toDouble()
        }
        return anchors
    }

    /** Splits [index] so that a new note starts at [frame]. Returns false if the cut is too close to an edge. */
    fun split(edit: Edit, an: Analysis, index: Int, frame: Int): Boolean {
        val n = edit.notes[index]
        if (frame - n.f0 < 3 || n.f1 - frame < 2) return false
        val target = n.center + n.offset
        val right = n.copy()
        right.f0 = frame
        n.f1 = frame - 1
        n.refresh(an)
        right.refresh(an)
        // refresh recomputes each half's centre; keep both halves on the pitch the note was edited to.
        n.offset = target - n.center
        right.offset = target - right.center
        edit.notes.add(index + 1, right)
        val key = right.inStart(an)
        if (!edit.anchors.containsKey(key)) {
            edit.anchors[key] = edit.timeMap(an.length).toOutput(key.toDouble())
        }
        return true
    }

    /** Joins [index] with the following note if they touch (or nearly). */
    fun merge(edit: Edit, an: Analysis, index: Int): Boolean {
        if (index < 0 || index + 1 >= edit.notes.size) return false
        val a = edit.notes[index]
        val b = edit.notes[index + 1]
        if (b.f0 - a.f1 > 3) return false
        val touching = a.inEnd(an) == b.inStart(an)
        val boundary = b.inStart(an)
        val target = a.center + a.offset
        a.f1 = b.f1
        a.refresh(an)
        a.offset = target - a.center // the joined note keeps the first note's edited pitch
        edit.notes.removeAt(index + 1)
        // Notes that shared a boundary move as one once joined, so that anchor goes away.
        if (touching) edit.anchors.remove(boundary)
        return true
    }

    /**
     * Melodyne-style correction. For each note: [center] 0..1 pulls it toward the nearest allowed note,
     * [drift] and [mod] 0..1 remove that fraction of slow drift and fast modulation.
     */
    fun correct(
        notes: Collection<Note>, mask: Int, center: Double, drift: Double, mod: Double, globalShift: Double,
        monotone: Boolean = false, monotoneNote: Int = 57,
    ) {
        for (n in notes) {
            val shifted = n.center + globalShift
            val target = if (monotone) monotoneNote else Scales.nearestAllowed(shifted, mask)
            n.offset = (target - shifted) * center
            n.drift = (1.0 - drift).toFloat()
            n.mod = (1.0 - mod).toFloat()
        }
    }

    /** Applies the Tuning settings from the Voice tab (key, scale, strength, retune speed, monotone) to [notes]. */
    fun correctToSettings(notes: Collection<Note>, p: VoiceParams, globalShift: Double) {
        val strength = p[Specs.STRENGTH] / 100.0
        val smoothing = (p[Specs.RETUNE] / 300.0).coerceIn(0.0, 1.0)
        val flatten = strength * (1.0 - smoothing)
        val mask = Scales.mask(p.int(Specs.SCALE), p.int(Specs.KEY), p[Specs.CUSTOM_MASK].roundToInt())
        correct(notes, mask, strength, flatten, flatten, globalShift, p.bool(Specs.MONOTONE), p.int(Specs.MONO_NOTE))
    }

    /** Global shift in semitones: manual shift plus register matching. */
    fun globalShift(an: Analysis, p: VoiceParams): Double {
        var shift = p[Specs.SHIFT].toDouble()
        if (p.bool(Specs.AUTO_REG)) {
            val med = an.medianMidi()
            if (!med.isNaN()) shift += (Pitch.hzToMidi(p[Specs.REG_HZ].toDouble(), an.a4) - med).coerceIn(-14.0, 14.0)
        }
        return shift
    }

    /** Global formant shift in semitones: the setting plus its share of the pitch shift. */
    fun formantShift(p: VoiceParams, globalShift: Double): Double =
        (p[Specs.FORMANT] + p[Specs.FORMANT_FOLLOW] / 100.0 * globalShift).coerceIn(-12.0, 12.0)
}

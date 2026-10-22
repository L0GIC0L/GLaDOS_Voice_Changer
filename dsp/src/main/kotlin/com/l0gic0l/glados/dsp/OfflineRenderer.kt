package com.l0gic0l.glados.dsp

import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.PI
import kotlin.math.pow
import kotlin.math.sin

/** Global settings applied on top of the per-note edits. */
class RenderSettings(
    val pitchShift: Double = 0.0,
    val formantShift: Double = 0.0,
    val breath: Float = 1f,
    val vibratoCents: Double = 0.0,
    val vibratoHz: Double = 5.5,
    /** 0..1 share of the original, unedited voice blended into the result. */
    val dry: Float = 0f,
    /** 0..1 level of a second voice [harmonySt] semitones away. */
    val harmonyMix: Float = 0f,
    val harmonySt: Double = -12.0,
    /** Tempo and random speed changes; null keeps the timeline's own speed. Must be built for the clip's length. */
    val warp: SpeedWarp? = null,
)

/**
 * Non-real-time PSOLA: rebuilds the clip from the edited notes, honouring pitch, formant, gain and
 * timing edits. Marks and the pitch contour come from a full-clip [Analysis], so it can look ahead and stretch time.
 * The result is [SpeedWarp.renderedLength] samples long when [RenderSettings.warp] is set, otherwise as long as [x].
 */
object OfflineRenderer {
    private const val PAD = 8192

    fun render(
        x: FloatArray, an: Analysis, edit: Edit, settings: RenderSettings, cancelled: () -> Boolean = { false },
    ): FloatArray {
        val n = x.size
        val warp = settings.warp ?: SpeedWarp.identity(n)
        val timeMap = edit.timeMap(n)
        val out = renderVoice(x, an, edit, timeMap, warp, settings, settings.pitchShift, cancelled)
        val h = settings.harmonyMix.coerceIn(0f, 1f)
        if (h > 0f) {
            val harmony = renderVoice(x, an, edit, timeMap, warp, settings, settings.pitchShift + settings.harmonySt, cancelled)
            val level = 1f / (1f + 0.5f * h)
            for (i in out.indices) out[i] = (out[i] + harmony[i] * h) * level
        }
        val d = settings.dry.coerceIn(0f, 1f)
        if (d > 0f) {
            // The original voice, following any timing and speed changes so it stays in step with the tuned one.
            for (i in out.indices) {
                val a = timeMap.toInput(warp.toTimeline(i.toDouble())).coerceIn(0.0, (n - 1).toDouble())
                val j = a.toInt()
                val f = (a - j).toFloat()
                val dryV = x[j] + (x[min(j + 1, n - 1)] - x[j]) * f
                out[i] = out[i] * (1f - d) + dryV * d
            }
        }
        return out
    }

    private fun renderVoice(
        x: FloatArray, an: Analysis, edit: Edit, timeMap: TimeMap, warp: SpeedWarp, settings: RenderSettings,
        pitchShift: Double, cancelled: () -> Boolean,
    ): FloatArray {
        val n = x.size
        val outLen = warp.renderedLength
        val sr = an.sampleRate
        val src = FloatArray(n + 2 * PAD)
        System.arraycopy(x, 0, src, PAD, n)
        val acc = FloatArray(outLen + 2 * PAD)
        val norm = FloatArray(outLen + 2 * PAD)

        val noteOfFrame = IntArray(an.frames) { -1 }
        edit.notes.forEachIndexed { idx, note ->
            for (f in max(0, note.f0)..min(an.frames - 1, note.f1)) noteOfFrame[f] = idx
        }
        val marks = an.marks
        val unvoicedHalf = max(16.0, sr * 0.005)
        val baseRho = 2.0.pow(settings.formantShift / 12.0)

        var s = 0.0
        var grains = 0
        while (s < outLen) {
            if (++grains % 512 == 0 && cancelled()) break
            val a = timeMap.toInput(warp.toTimeline(s)).coerceIn(0.0, (n - 1).toDouble())
            val frame = a / an.hop
            val fi = frame.toInt().coerceIn(0, an.frames - 1)
            var markIdx = -1
            var tin = 0.0
            if (an.voiced[fi] && marks.isNotEmpty()) {
                markIdx = nearestMark(marks, a)
                tin = periodAt(an, marks[markIdx])
                if (abs(marks[markIdx] - a) > 0.75 * tin) markIdx = -1
            }

            if (markIdx >= 0) {
                val note = edit.notes.getOrNull(noteOfFrame[fi])
                val inMidi = an.midiAt(frame)
                val vibrato = settings.vibratoCents / 100.0 * sin(2 * PI * settings.vibratoHz * s / sr)
                val outMidi = (if (note != null) note.pitchAt(frame) else inMidi) + pitchShift + vibrato
                val ts = sr / Pitch.midiToHz(outMidi, an.a4).coerceIn(40.0, 1600.0)
                val rho = (baseRho * 2.0.pow((note?.formant ?: 0f) / 12.0)).coerceIn(0.4, 2.5)
                val gain = if (note == null || note.gainDb == 0f) 1f else 10f.pow(note.gainDb / 20f)
                val half = min(max(tin, 0.6 * ts * rho), 2.0 * tin)
                Grain.add(src, -1, (marks[markIdx] + PAD).toDouble(), half, acc, norm, -1, s + PAD, rho, gain)
                s += ts
            } else {
                val note = edit.notes.getOrNull(noteOfFrame[fi])
                val rho = (baseRho * 2.0.pow((note?.formant ?: 0f) / 12.0)).coerceIn(0.4, 2.5)
                val gain = settings.breath * (if (note == null || note.gainDb == 0f) 1f else 10f.pow(note.gainDb / 20f))
                Grain.add(src, -1, a + PAD, unvoicedHalf, acc, norm, -1, s + PAD, rho, gain)
                s += unvoicedHalf / rho
            }
        }

        return FloatArray(outLen) { acc[it + PAD] / max(norm[it + PAD], Grain.NORM_FLOOR) }
    }

    private fun nearestMark(marks: IntArray, t: Double): Int {
        var lo = 0
        var hi = marks.size - 1
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (marks[mid] < t) lo = mid + 1 else hi = mid
        }
        return if (lo > 0 && abs(marks[lo - 1] - t) <= abs(marks[lo] - t)) lo - 1 else lo
    }

    private fun periodAt(an: Analysis, t: Int): Double {
        val i = (t.toDouble() / an.hop).toInt().coerceIn(0, an.frames - 1)
        var k = i
        var d = 0
        while (!an.voiced[k] && d < 6) {
            d++
            k = (i + if (d % 2 == 1) (d + 1) / 2 else -(d / 2)).coerceIn(0, an.frames - 1)
        }
        return an.sampleRate / an.f0[k].toDouble()
    }
}

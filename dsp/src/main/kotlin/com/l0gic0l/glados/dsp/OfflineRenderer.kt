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
)

/**
 * Non-real-time PSOLA: rebuilds the clip from the edited notes, honouring pitch, formant, gain and
 * timing edits. Marks and the pitch contour come from a full-clip [Analysis], so it can look ahead and stretch time.
 */
object OfflineRenderer {
    private const val PAD = 8192

    fun render(
        x: FloatArray, an: Analysis, edit: Edit, settings: RenderSettings, cancelled: () -> Boolean = { false },
    ): FloatArray {
        val n = x.size
        val sr = an.sampleRate
        val src = FloatArray(n + 2 * PAD)
        System.arraycopy(x, 0, src, PAD, n)
        val acc = FloatArray(n + 2 * PAD)
        val norm = FloatArray(n + 2 * PAD)

        val timeMap = edit.timeMap(n)
        val noteOfFrame = IntArray(an.frames) { -1 }
        edit.notes.forEachIndexed { idx, note ->
            for (f in max(0, note.f0)..min(an.frames - 1, note.f1)) noteOfFrame[f] = idx
        }
        val marks = an.marks
        val unvoicedHalf = max(16.0, sr * 0.005)
        val baseRho = 2.0.pow(settings.formantShift / 12.0)

        var s = 0.0
        var grains = 0
        while (s < n) {
            if (++grains % 512 == 0 && cancelled()) break
            val a = timeMap.toInput(s).coerceIn(0.0, (n - 1).toDouble())
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
                val outMidi = (if (note != null) note.pitchAt(frame) else inMidi) + settings.pitchShift + vibrato
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

        val out = FloatArray(n) { acc[it + PAD] / max(norm[it + PAD], Grain.NORM_FLOOR) }
        val d = settings.dry.coerceIn(0f, 1f)
        if (d > 0f) {
            // The original voice, following any timing edits so it stays in step with the tuned one.
            for (i in 0 until n) {
                val a = timeMap.toInput(i.toDouble()).coerceIn(0.0, (n - 1).toDouble())
                val j = a.toInt()
                val f = (a - j).toFloat()
                val dryV = x[j] + (x[min(j + 1, n - 1)] - x[j]) * f
                out[i] = out[i] * (1f - d) + dryV * d
            }
        }
        return out
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

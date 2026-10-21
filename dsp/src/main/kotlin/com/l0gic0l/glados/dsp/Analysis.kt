package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sqrt

/** Pitch, voicing, loudness and pitch marks for a whole clip. Frame i is centred on sample i * hop. */
class Analysis(
    val sampleRate: Int,
    val hop: Int,
    val length: Int,
    val f0: FloatArray,
    val voiced: BooleanArray,
    val level: FloatArray,
    val marks: IntArray,
    val a4: Double,
    val aperiodicity: FloatArray = FloatArray(f0.size),
) {
    val frames: Int get() = f0.size

    fun midi(frame: Int): Double = Pitch.hzToMidi(f0[frame].toDouble(), a4)

    /** Interpolated pitch (MIDI) at a fractional frame, using the nearest voiced frames. */
    fun midiAt(frame: Double): Double {
        val i = floor(frame).toInt().coerceIn(0, frames - 1)
        val j = (i + 1).coerceAtMost(frames - 1)
        val f = (frame - i).coerceIn(0.0, 1.0)
        return when {
            voiced[i] && voiced[j] -> midi(i) * (1 - f) + midi(j) * f
            voiced[i] -> midi(i)
            voiced[j] -> midi(j)
            else -> Double.NaN
        }
    }

    /** Loudness near the top of the clip (95th percentile frame level), used to scale note thickness. */
    val loudLevel: Float by lazy {
        val sorted = level.sorted()
        if (sorted.isEmpty()) 1e-4f else sorted[(sorted.size * 0.95).toInt().coerceAtMost(sorted.size - 1)].coerceAtLeast(1e-4f)
    }

    private val median: Double by lazy {
        val v = (0 until frames).filter { voiced[it] }.map { midi(it) }.sorted()
        if (v.isEmpty()) Double.NaN else v[v.size / 2]
    }

    /** Median pitch of voiced frames (MIDI), or NaN for a clip with no voiced sound. */
    fun medianMidi(): Double = median
}

object Analyzer {
    fun analyze(
        x: FloatArray,
        sampleRate: Int,
        params: VoiceParams,
        progress: ((Float) -> Unit)? = null,
        cancelled: () -> Boolean = { false },
        thresholdOverride: Double? = null,
    ): Analysis {
        val fmin = params[Specs.FMIN].toDouble()
        val fmax = max(params[Specs.FMAX].toDouble(), fmin * 2)
        val a4 = params[Specs.REF_A4].toDouble()
        val threshold = thresholdOverride ?: Voicing.threshold(params[Specs.VOICING])

        val hop = max(32, (sampleRate * 0.005).roundToInt())
        val frames = x.size / hop + 1
        val yin = Yin(sampleRate, fmin, fmax)
        val w = yin.windowLength
        val pad = w + 64
        // Pitch is tracked on a band-passed copy (rumble and hiss removed); the audio itself is untouched.
        val band = PitchBandpass.zeroPhase(x, sampleRate, fmin, fmax)
        val xp = FloatArray(x.size + 2 * pad)
        System.arraycopy(band, 0, xp, pad, x.size)

        val f0 = FloatArray(frames)
        val periodic = BooleanArray(frames)
        val aperiodicity = FloatArray(frames)
        val level = FloatArray(frames)
        for (i in 0 until frames) {
            if (cancelled()) break
            val center = i * hop
            periodic[i] = yin.detect(xp, -1, center - w / 2 + pad, threshold)
            f0[i] = yin.f0.toFloat()
            aperiodicity[i] = yin.aperiodicity.toFloat()
            var s = 0.0
            val a = max(0, center - hop)
            val b = min(x.size, center + hop)
            for (k in a until b) s += band[k].toDouble() * band[k]
            level[i] = sqrt(s / max(1, b - a)).toFloat()
            if (i % 256 == 0) progress?.invoke(i.toFloat() / frames * 0.9f)
        }

        val sorted = level.sorted()
        val loud = sorted[(sorted.size * 0.95).toInt().coerceAtMost(sorted.size - 1)]
        val silence = max(2e-5f, loud * 0.01f)
        val voiced = BooleanArray(frames) {
            periodic[it] && level[it] > silence && f0[it] in fmin.toFloat()..fmax.toFloat()
        }

        growVoiced(f0, voiced, aperiodicity, level, silence, threshold)
        fixOctaves(f0, voiced)
        fillGaps(f0, voiced)
        dropShortRuns(voiced, 4)
        medianSmooth(f0, voiced)

        val marks = trackMarks(xp, pad, x.size, sampleRate, hop, f0, voiced, cancelled)
        progress?.invoke(1f)
        return Analysis(sampleRate, hop, x.size, f0, voiced, level, marks, a4, aperiodicity)
    }

    /**
     * Hysteresis: a frame next to confirmed voice is kept if it is nearly periodic and its pitch is
     * continuous with the neighbour, so onsets, offsets and brief dips in a vowel are not lost.
     */
    private fun growVoiced(f0: FloatArray, voiced: BooleanArray, ap: FloatArray, level: FloatArray, silence: Float, threshold: Double) {
        val loose = threshold * 1.5
        fun ok(i: Int, from: Int): Boolean =
            !voiced[i] && ap[i] < loose && level[i] > silence && abs(ln(f0[i].toDouble() / f0[from])) < 0.12
        for (i in 1 until f0.size) if (voiced[i - 1] && ok(i, i - 1)) voiced[i] = true
        for (i in f0.size - 2 downTo 0) if (voiced[i + 1] && ok(i, i + 1)) voiced[i] = true
    }

    /** Pulls isolated octave jumps back to the pitch of their neighbours. */
    private fun fixOctaves(f0: FloatArray, voiced: BooleanArray) {
        val l2 = DoubleArray(f0.size) { if (voiced[it]) ln(f0[it].toDouble()) / ln(2.0) else 0.0 }
        val tmp = ArrayList<Double>(9)
        for (i in f0.indices) {
            if (!voiced[i]) continue
            tmp.clear()
            for (k in -4..4) {
                val j = i + k
                if (j in f0.indices && voiced[j]) tmp += l2[j]
            }
            tmp.sort()
            val med = tmp[tmp.size / 2]
            val d = l2[i] - med
            val k = d.roundToInt()
            if (k != 0 && abs(d - k) < 0.12) f0[i] = (2.0.pow(l2[i] - k)).toFloat()
        }
    }

    /** Bridges one or two unvoiced frames inside a voiced stretch. */
    private fun fillGaps(f0: FloatArray, voiced: BooleanArray) {
        var i = 1
        while (i < f0.size - 1) {
            if (!voiced[i] && voiced[i - 1]) {
                var j = i
                while (j < f0.size && !voiced[j]) j++
                if (j < f0.size && j - i <= 4 && abs(ln(f0[j].toDouble() / f0[i - 1])) < 0.12) {
                    for (k in i until j) {
                        val t = (k - i + 1).toFloat() / (j - i + 1)
                        f0[k] = f0[i - 1] * (1 - t) + f0[j] * t
                        voiced[k] = true
                    }
                }
                i = j
            } else {
                i++
            }
        }
    }

    private fun dropShortRuns(voiced: BooleanArray, minFrames: Int) {
        var i = 0
        while (i < voiced.size) {
            if (voiced[i]) {
                var j = i
                while (j < voiced.size && voiced[j]) j++
                if (j - i < minFrames) for (k in i until j) voiced[k] = false
                i = j
            } else {
                i++
            }
        }
    }

    private fun medianSmooth(f0: FloatArray, voiced: BooleanArray) {
        val src = f0.copyOf()
        for (i in 1 until f0.size - 1) {
            if (voiced[i - 1] && voiced[i] && voiced[i + 1]) {
                val a = src[i - 1]; val b = src[i]; val c = src[i + 1]
                f0[i] = max(min(a, b), min(max(a, b), c))
            }
        }
    }

    private fun trackMarks(
        xp: FloatArray, pad: Int, n: Int, sr: Int, hop: Int, f0: FloatArray, voiced: BooleanArray, cancelled: () -> Boolean,
    ): IntArray {
        val marks = ArrayList<Int>()
        fun periodAt(t: Int): Double {
            val i = (t.toDouble() / hop).roundToInt().coerceIn(0, f0.size - 1)
            var k = i
            var d = 0
            while (!voiced[k] && d < 6) {
                d++
                k = (i + if (d % 2 == 1) (d + 1) / 2 else -(d / 2)).coerceIn(0, f0.size - 1)
            }
            return sr / f0[k].toDouble()
        }

        var i = 0
        while (i < f0.size) {
            if (!voiced[i]) {
                i++
                continue
            }
            var j = i
            while (j < f0.size && voiced[j]) j++
            val s0 = max(0, i * hop - hop / 2)
            val s1 = min(n - 1, (j - 1) * hop + hop / 2)
            var m = (Marks.peak(xp, -1, (s0 + pad).toLong(), periodAt(s0).toInt()) - pad).toInt()
            marks += m
            while (!cancelled()) {
                val t = periodAt(m)
                val next = (Marks.next(xp, -1, (m + pad).toLong(), t) - pad).toInt()
                if (next > s1 || next <= m) break
                marks += next
                m = next
            }
            i = j
        }
        return marks.toIntArray()
    }
}

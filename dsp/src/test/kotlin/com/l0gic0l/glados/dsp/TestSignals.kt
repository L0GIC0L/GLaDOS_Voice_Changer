package com.l0gic0l.glados.dsp

import java.util.Random
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.log2
import kotlin.math.max
import kotlin.math.sin
import kotlin.math.sqrt

object TestSignals {
    val VOWEL_A = listOf(700.0 to 110.0, 1200.0 to 120.0, 2600.0 to 160.0)

    /**
     * Additive vowel-like signal: harmonics of a time-varying f0 shaped by formant peaks and a
     * 12 dB/octave tilt. Light jitter keeps it from being unrealistically perfect.
     */
    fun voice(
        sr: Int, seconds: Double, f0: (Double) -> Double,
        formants: List<Pair<Double, Double>> = VOWEL_A, amp: Float = 0.3f, jitter: Double = 0.002, seed: Long = 1,
    ): FloatArray {
        val n = (seconds * sr).toInt()
        val out = FloatArray(n)
        val rnd = Random(seed)
        var phase = 0.0
        val amps = DoubleArray(160)
        var harmonics = 0
        for (i in 0 until n) {
            val t = i.toDouble() / sr
            val f = f0(t) * (1.0 + jitter * rnd.nextGaussian() * 0.3)
            if (i % 64 == 0) {
                harmonics = max(1, minOf(amps.size, (0.45 * sr / f).toInt()))
                for (k in 1..harmonics) {
                    val hz = k * f
                    var env = 0.02
                    for ((fc, bw) in formants) env += 1.0 / (1.0 + ((hz - fc) / bw).let { it * it })
                    amps[k - 1] = env / (k.toDouble() * k)
                }
            }
            phase += 2 * PI * f / sr
            if (phase > 2 * PI) phase -= 2 * PI
            var s = 0.0
            for (k in 1..harmonics) s += amps[k - 1] * sin(k * phase)
            out[i] = s.toFloat()
        }
        normalise(out, amp)
        return out
    }

    fun normalise(x: FloatArray, peak: Float) {
        var m = 0f
        for (v in x) m = max(m, abs(v))
        if (m > 0) for (i in x.indices) x[i] = x[i] / m * peak
    }

    /** f0 per hop (Hz), 0 where unvoiced. Frames start at [from] samples. */
    fun pitchTrack(x: FloatArray, sr: Int, from: Int = 0, to: Int = x.size, hop: Int = 480, fmin: Double = 60.0, fmax: Double = 900.0): DoubleArray {
        val yin = Yin(sr, fmin, fmax)
        val w = yin.windowLength
        val out = ArrayList<Double>()
        var s = from
        while (s + w <= to) {
            out += if (yin.detect(x, -1, s, 0.15)) yin.f0 else 0.0
            s += hop
        }
        return out.toDoubleArray()
    }

    fun median(v: DoubleArray): Double {
        val s = v.filter { it > 0 }.sorted()
        return if (s.isEmpty()) 0.0 else s[s.size / 2]
    }

    fun cents(a: Double, b: Double): Double = 1200.0 * log2(a / b)

    fun rms(x: FloatArray, from: Int = 0, to: Int = x.size): Double {
        var s = 0.0
        for (i in from until to) s += x[i] * x[i].toDouble()
        return sqrt(s / max(1, to - from))
    }

    /** Power-weighted mean frequency of [x] in [from, to) using a plain DFT over the audible range. */
    fun spectralCentroid(x: FloatArray, sr: Int, from: Int, to: Int): Double {
        val n = to - from
        var num = 0.0
        var den = 0.0
        var f = 100.0
        while (f < 6000.0) {
            var re = 0.0
            var im = 0.0
            val w = 2 * PI * f / sr
            for (i in 0 until n) {
                val win = 0.5 - 0.5 * kotlin.math.cos(2 * PI * i / n)
                re += x[from + i] * win * kotlin.math.cos(w * i)
                im -= x[from + i] * win * sin(w * i)
            }
            val p = re * re + im * im
            num += f * p
            den += p
            f += 25.0
        }
        return num / den
    }
}

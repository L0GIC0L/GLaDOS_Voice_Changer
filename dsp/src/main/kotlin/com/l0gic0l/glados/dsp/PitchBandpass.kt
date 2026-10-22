package com.l0gic0l.glados.dsp

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

/**
 * Band-pass used only to help the pitch tracker; it is never heard. Phone and room recordings carry
 * rumble below the voice and hiss above it, and either one drowns the periodicity the tracker looks
 * for. Four-pole Butterworth high-pass and low-pass, each as two biquads.
 *
 * Use [process] for a stream (it adds a small, frequency-dependent delay) or [zeroPhase] for a whole
 * clip (no delay, so positions line up with the original audio).
 */
class PitchBandpass(sampleRate: Int, fmin: Double, fmax: Double) {
    private class Section(val b0: Double, val b1: Double, val b2: Double, val a1: Double, val a2: Double) {
        var z1 = 0.0
        var z2 = 0.0
        fun step(x: Double): Double {
            val y = b0 * x + z1
            z1 = b1 * x - a1 * y + z2
            z2 = b2 * x - a2 * y
            return y
        }
        fun reset() { z1 = 0.0; z2 = 0.0 }
    }

    private val sections: List<Section>

    init {
        val hp = max(40.0, 0.7 * fmin)
        val lp = min(sampleRate * 0.45, max(1200.0, 2.5 * fmax))
        sections = buildList {
            for (q in Q4) add(design(sampleRate, hp, q, high = true))
            for (q in Q4) add(design(sampleRate, lp, q, high = false))
        }
    }

    fun reset() = sections.forEach { it.reset() }

    fun process(x: Float): Float {
        var v = x.toDouble()
        for (s in sections) v = s.step(v)
        return v.toFloat()
    }

    companion object {
        private val Q4 = doubleArrayOf(0.5411961, 1.3065630)

        private fun design(sr: Int, fc: Double, q: Double, high: Boolean): Section {
            val w = 2 * PI * fc / sr
            val c = cos(w)
            val alpha = sin(w) / (2 * q)
            val a0 = 1 + alpha
            return if (high) {
                Section((1 + c) / 2 / a0, -(1 + c) / a0, (1 + c) / 2 / a0, -2 * c / a0, (1 - alpha) / a0)
            } else {
                Section((1 - c) / 2 / a0, (1 - c) / a0, (1 - c) / 2 / a0, -2 * c / a0, (1 - alpha) / a0)
            }
        }

        /** Filters the whole clip forwards and then backwards, so the result has no phase shift. */
        fun zeroPhase(x: FloatArray, sampleRate: Int, fmin: Double, fmax: Double): FloatArray {
            val f = PitchBandpass(sampleRate, fmin, fmax)
            val y = FloatArray(x.size)
            for (i in x.indices) y[i] = f.process(x[i])
            f.reset()
            for (i in x.indices.reversed()) y[i] = f.process(y[i])
            return y
        }
    }
}

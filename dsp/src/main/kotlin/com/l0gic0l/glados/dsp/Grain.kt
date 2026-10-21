package com.l0gic0l.glados.dsp

import kotlin.math.PI
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.floor

/** Windowed-grain overlap-add shared by the live and offline PSOLA engines. */
object Grain {
    /**
     * Lower bound for the window-sum divisor. Dividing by the raw sum would turn the faint tails of
     * widely spaced grains (large downward shifts) back into full-level copies of the neighbouring
     * pitch pulses, undoing the shift. The floor keeps amplitude flat for moderate shifts and
     * leaves those tails quiet for large ones.
     */
    const val NORM_FLOOR = 0.5f

    private const val HANN_N = 1024
    private val hann = FloatArray(HANN_N + 2).also {
        for (i in 0..HANN_N) it[i] = (0.5 - 0.5 * cos(2.0 * PI * i / HANN_N)).toFloat()
        it[HANN_N + 1] = 0f
    }

    /** Hann window at u in [0, 1]: zero at both edges, one at the middle. */
    private fun window(u: Double): Float {
        val pos = u * HANN_N
        val i = pos.toInt()
        val f = (pos - i).toFloat()
        return hann[i] + (hann[i + 1] - hann[i]) * f
    }

    /** Catmull-Rom interpolation at fractional index [pos]. */
    fun cubic(x: FloatArray, mask: Int, pos: Double): Float {
        val i = floor(pos).toInt()
        val f = (pos - i).toFloat()
        val p0 = x[(i - 1) and mask]
        val p1 = x[i and mask]
        val p2 = x[(i + 1) and mask]
        val p3 = x[(i + 2) and mask]
        val a = -0.5f * p0 + 1.5f * p1 - 1.5f * p2 + 0.5f * p3
        val b = p0 - 2.5f * p1 + 2f * p2 - 0.5f * p3
        val c = -0.5f * p0 + 0.5f * p2
        return ((a * f + b) * f + c) * f + p1
    }

    /**
     * Adds one Hann-windowed grain of the source, centred on [center], into the accumulator at
     * output position [s].
     *
     * @param half grain half-length in source samples (the grain spans center - half .. center + half)
     * @param rho read-speed ratio. Above 1 the grain is shortened, which raises the formants
     *   without touching the spacing of grains (and so the pitch).
     * @param gain applied to the signal only. The window sum in [norm] is left unscaled so that
     *   neighbouring grains with different gains crossfade rather than step.
     */
    fun add(
        src: FloatArray, smask: Int, center: Double, half: Double,
        acc: FloatArray, norm: FloatArray, amask: Int,
        s: Double, rho: Double, gain: Float,
    ) {
        val hs = half / rho
        val n0 = ceil(s - hs).toInt()
        val n1 = floor(s + hs).toInt()
        for (n in n0..n1) {
            val p = (n - s) * rho
            val w = window(0.5 + 0.5 * p / half)
            val v = cubic(src, smask, center + p) * w
            val idx = n and amask
            acc[idx] += v * gain
            norm[idx] += w
        }
    }
}

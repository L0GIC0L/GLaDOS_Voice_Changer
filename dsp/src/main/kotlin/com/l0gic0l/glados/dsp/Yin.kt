package com.l0gic0l.glados.dsp

import kotlin.math.PI
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

/**
 * YIN fundamental-frequency detector (de Cheveigné and Kawahara, 2002).
 *
 * The search runs on a copy decimated to roughly 12 kHz, then the winning lag is refined on the
 * original samples. Input is read as `x[(start + i) and mask]`, so it works on a ring buffer
 * (mask = size - 1) or a plain array (mask = -1).
 */
class Yin(private val sampleRate: Int, fmin: Double, fmax: Double) {
    private val decim = max(1, (sampleRate / 12000.0).toInt())
    private val srd = sampleRate.toDouble() / decim
    private val tauMin = max(2, floor(srd / fmax).toInt())
    private val tauMax = ceil(srd / fmin).toInt()
    private val w = tauMax
    private val decimated = FloatArray(w + tauMax + 1)
    private val diff = DoubleArray(tauMax + 2)
    private val cmnd = DoubleArray(tauMax + 2)
    private val fir: FloatArray = designLowpass()
    private val refineCost = DoubleArray(2 * decim + 4)

    /** Number of input samples read by [detect]. */
    val windowLength: Int = decim * (w + tauMax)

    /** Estimated fundamental in Hz. Meaningful when [detect] returned true; best guess otherwise. */
    var f0: Double = 0.0
        private set

    /** Normalised difference at the chosen lag: 0 is perfectly periodic, 1 is noise. */
    var aperiodicity: Double = 1.0
        private set

    private fun designLowpass(): FloatArray {
        if (decim == 1) return floatArrayOf(1f)
        val half = 2 * decim
        val cutoff = 0.45 / decim
        val h = FloatArray(2 * half + 1)
        var sum = 0.0
        for (k in -half..half) {
            val sinc = if (k == 0) 2 * cutoff else sin(2 * PI * cutoff * k) / (PI * k)
            val win = 0.5 + 0.5 * cos(PI * k / (half + 1))
            h[k + half] = (sinc * win).toFloat()
            sum += h[k + half]
        }
        for (i in h.indices) h[i] = (h[i] / sum).toFloat()
        return h
    }

    /**
     * @param threshold absolute YIN threshold; lower is stricter (typical 0.10 to 0.25)
     * @return true if the window is judged periodic
     */
    fun detect(x: FloatArray, mask: Int, start: Int, threshold: Double): Boolean {
        val n = w + tauMax
        decimate(x, mask, start, n)

        for (tau in 1..tauMax) {
            var s = 0.0
            for (j in 0 until w) {
                val d = (decimated[j] - decimated[j + tau]).toDouble()
                s += d * d
            }
            diff[tau] = s
        }
        cmnd[0] = 1.0
        var running = 0.0
        for (tau in 1..tauMax) {
            running += diff[tau]
            cmnd[tau] = if (running > 1e-12) diff[tau] * tau / running else 1.0
        }

        // First dip below the threshold, followed down to its local minimum.
        var tau = -1
        var t = tauMin
        while (t < tauMax) {
            if (cmnd[t] < threshold) {
                while (t + 1 < tauMax && cmnd[t + 1] < cmnd[t]) t++
                tau = t
                break
            }
            t++
        }
        if (tau < 0) {
            var best = tauMin
            for (i in tauMin..tauMax) if (cmnd[i] < cmnd[best]) best = i
            tau = best
        }
        aperiodicity = cmnd[tau]

        var lag = tau.toDouble()
        if (tau > 1 && tau < tauMax) {
            val a = cmnd[tau - 1]
            val b = cmnd[tau]
            val c = cmnd[tau + 1]
            val denom = a - 2 * b + c
            if (denom > 1e-12) lag += (0.5 * (a - c) / denom).coerceIn(-1.0, 1.0)
        }

        var lagFull = lag * decim
        if (decim > 1) lagFull = refine(x, mask, start, lagFull)
        f0 = sampleRate / lagFull
        return aperiodicity < threshold
    }

    private fun decimate(x: FloatArray, mask: Int, start: Int, n: Int) {
        if (decim == 1) {
            for (i in 0 until n) decimated[i] = x[(start + i) and mask]
            return
        }
        val half = fir.size / 2
        val last = windowLength - 1
        for (i in 0 until n) {
            val center = i * decim + decim / 2
            var acc = 0f
            for (k in fir.indices) {
                val idx = min(max(center + k - half, 0), last)
                acc += fir[k] * x[(start + idx) and mask]
            }
            decimated[i] = acc
        }
    }

    /** Re-search the lag on the original samples around the decimated estimate. */
    private fun refine(x: FloatArray, mask: Int, start: Int, lagGuess: Double): Double {
        val lo = max(2, floor(lagGuess).toInt() - decim)
        val hi = ceil(lagGuess).toInt() + decim
        val len = windowLength - hi
        if (len < lo) return lagGuess
        val count = hi - lo + 1
        val d = refineCost
        var bestI = 0
        for (i in 0 until count) {
            val lag = lo + i
            var s = 0.0
            for (j in 0 until len) {
                val e = (x[(start + j) and mask] - x[(start + j + lag) and mask]).toDouble()
                s += e * e
            }
            d[i] = s
            if (s < d[bestI]) bestI = i
        }
        var lag = (lo + bestI).toDouble()
        if (bestI in 1 until count - 1) {
            val a = d[bestI - 1]
            val b = d[bestI]
            val c = d[bestI + 1]
            val denom = a - 2 * b + c
            if (denom > 1e-18) lag += (0.5 * (a - c) / denom).coerceIn(-1.0, 1.0)
        }
        return lag
    }
}

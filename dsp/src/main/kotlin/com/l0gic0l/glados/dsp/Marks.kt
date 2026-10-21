package com.l0gic0l.glados.dsp

import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt
import kotlin.math.sqrt

/**
 * Pitch marks: one position per glottal cycle, all at the same phase of the waveform.
 * PSOLA cuts a grain around each mark, so consistency matters more than exact placement.
 */
object Marks {
    /**
     * Next mark after [prev], expected about [period] samples later. Picks the offset whose
     * surrounding waveform best matches the waveform around [prev] (normalised cross-correlation),
     * which keeps the chain locked to one phase even as the pitch drifts.
     *
     * Reads `x[(i) and mask]`, so [x] may be a ring buffer or a plain array.
     */
    fun next(x: FloatArray, mask: Int, prev: Long, period: Double): Long {
        val p = prev.toInt()
        val t = max(period, 8.0)
        val h = max(4, (t * 0.5).toInt())
        val delta = max(2, (t * 0.2).toInt())
        val predicted = p + t.roundToInt()

        var er = 0.0
        for (k in -h..h) {
            val v = x[(p + k) and mask]
            er += v * v
        }
        if (er < 1e-10) return prev + t.roundToInt()

        var best = -2.0
        var bestC = predicted
        for (c in predicted - delta..predicted + delta) {
            var cross = 0.0
            var ec = 0.0
            for (k in -h..h) {
                val a = x[(p + k) and mask]
                val b = x[(c + k) and mask]
                cross += a * b
                ec += b * b
            }
            val score = cross / sqrt(er * ec + 1e-20) - 0.002 * abs(c - predicted) / delta
            if (score > best) {
                best = score
                bestC = c
            }
        }
        return prev + (bestC - p)
    }

    /** Position of the largest absolute sample in `[from, from + length)`. */
    fun peak(x: FloatArray, mask: Int, from: Long, length: Int): Long {
        val f = from.toInt()
        var bestK = 0
        var bestV = -1f
        for (k in 0 until length) {
            val v = abs(x[(f + k) and mask])
            if (v > bestV) {
                bestV = v
                bestK = k
            }
        }
        return from + bestK
    }
}

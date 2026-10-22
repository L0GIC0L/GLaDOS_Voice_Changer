package com.l0gic0l.glados.dsp

import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.pow

object Pitch {
    private val LN2 = ln(2.0)

    fun hzToMidi(hz: Double, a4: Double = 440.0): Double = 69.0 + 12.0 * ln(hz / a4) / LN2
    fun midiToHz(midi: Double, a4: Double = 440.0): Double = a4 * 2.0.pow((midi - 69.0) / 12.0)
}

object Scales {
    val noteNames = arrayOf("C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B")

    class Def(val name: String, val intervals: IntArray)

    val all: List<Def> = listOf(
        Def("Chromatic", IntArray(12) { it }),
        Def("Major", intArrayOf(0, 2, 4, 5, 7, 9, 11)),
        Def("Natural minor", intArrayOf(0, 2, 3, 5, 7, 8, 10)),
        Def("Harmonic minor", intArrayOf(0, 2, 3, 5, 7, 8, 11)),
        Def("Pentatonic major", intArrayOf(0, 2, 4, 7, 9)),
        Def("Pentatonic minor", intArrayOf(0, 3, 5, 7, 10)),
        Def("Blues", intArrayOf(0, 3, 5, 6, 7, 10)),
        Def("Whole tone", intArrayOf(0, 2, 4, 6, 8, 10)),
        Def("Dorian", intArrayOf(0, 2, 3, 5, 7, 9, 10)),
        Def("Mixolydian", intArrayOf(0, 2, 4, 5, 7, 9, 10)),
        Def("Custom", IntArray(0)),
    )

    const val CUSTOM = 10
    const val CHROMATIC_MASK = 0xFFF

    /**
     * 12-bit mask of allowed pitch classes (bit 0 = C). For [CUSTOM] the mask is used as given,
     * otherwise the scale is transposed to [root].
     */
    fun mask(scale: Int, root: Int, customMask: Int): Int {
        if (scale == CUSTOM) return customMask and CHROMATIC_MASK
        var m = 0
        for (i in all[scale.coerceIn(0, all.size - 1)].intervals) m = m or (1 shl ((i + root) % 12))
        return m
    }

    fun isAllowed(mask: Int, midi: Int): Boolean = (mask shr Math.floorMod(midi, 12)) and 1 == 1

    /** Nearest allowed MIDI note to [midi]; plain rounding if the mask is empty. */
    fun nearestAllowed(midi: Double, mask: Int): Int {
        if (mask and CHROMATIC_MASK == 0) return Math.round(midi).toInt()
        val lo = floor(midi).toInt() - 6
        val hi = ceil(midi).toInt() + 6
        var best = Int.MIN_VALUE
        var bestDist = Double.MAX_VALUE
        for (n in lo..hi) {
            if (!isAllowed(mask, n)) continue
            val d = abs(n - midi)
            if (d < bestDist) {
                bestDist = d
                best = n
            }
        }
        return best
    }

    fun noteName(midi: Int): String = noteNames[Math.floorMod(midi, 12)] + (Math.floorDiv(midi, 12) - 1)
}

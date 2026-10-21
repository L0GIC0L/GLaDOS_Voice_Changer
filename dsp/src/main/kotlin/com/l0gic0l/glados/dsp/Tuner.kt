package com.l0gic0l.glados.dsp

import kotlin.math.abs
import kotlin.math.exp

/**
 * Chooses the target note for a detected pitch and smooths the correction over time.
 *
 * With [retuneMs] = 0 the output sits exactly on the target note (hard steps). Larger values
 * smooth the *correction* rather than the pitch, so natural glides survive at slow speeds.
 */
class Tuner(private val frameSeconds: Double) {
    var mask: Int = Scales.CHROMATIC_MASK
    var retuneMs: Double = 20.0
    var strength: Double = 1.0
    var holdSemitones: Double = 0.25
    var monotone: Boolean = false
    var monotoneNote: Int = 57

    private var currentTarget = Int.MIN_VALUE
    private var smoothed = 0.0
    private var wasVoiced = false

    fun reset() {
        currentTarget = Int.MIN_VALUE
        wasVoiced = false
    }

    /** Target note (MIDI) most recently chosen, or [Int.MIN_VALUE] if none. */
    val target: Int get() = currentTarget

    /**
     * @param midiIn detected pitch as MIDI, after any transposition
     * @return output pitch as MIDI
     */
    fun update(midiIn: Double, voiced: Boolean): Double {
        if (!voiced) {
            wasVoiced = false
            return midiIn
        }
        val nearest = if (monotone) monotoneNote else Scales.nearestAllowed(midiIn, mask)
        var tgt = nearest
        if (!monotone && wasVoiced && currentTarget != Int.MIN_VALUE && Scales.isAllowed(mask, currentTarget) && nearest != currentTarget) {
            // Hysteresis: stay on the current note until the new one is clearly closer.
            if (abs(midiIn - currentTarget) < abs(midiIn - nearest) + holdSemitones) tgt = currentTarget
        }
        currentTarget = tgt

        val correction = (tgt - midiIn) * strength
        if (!wasVoiced || retuneMs <= 0.0) {
            smoothed = correction
        } else {
            val alpha = 1.0 - exp(-frameSeconds * 1000.0 / retuneMs)
            smoothed += alpha * (correction - smoothed)
        }
        wasVoiced = true
        return midiIn + smoothed
    }
}

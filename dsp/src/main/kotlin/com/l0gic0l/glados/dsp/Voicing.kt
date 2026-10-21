package com.l0gic0l.glados.dsp

/** Maps the "Voicing sensitivity" setting to the YIN aperiodicity threshold. */
object Voicing {
    /**
     * A frame counts as pitched when its aperiodicity is below this value, so a higher sensitivity
     * means a higher threshold and more of the sound is treated as voice. Runs from a strict 0.08
     * (only very clean vowels) to a permissive 0.40 (noisy, breathy or reverberant speech).
     */
    fun threshold(sensitivityPercent: Float): Double = 0.08 + 0.32 * (sensitivityPercent / 100.0).coerceIn(0.0, 1.0)
}

package com.l0gic0l.glados.dsp

import java.util.Random
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/**
 * Where each moment of the edited timeline lands in the rendered output once the speed is changed: a steady tempo
 * plus, optionally, random stretches that rush ahead or drag. The same inputs always give the same warp, so a clip
 * keeps its rhythm when it is re-rendered, and the editor can map the playhead back onto the notes.
 */
class SpeedWarp private constructor(private val map: TimeMap?, val renderedLength: Int) {
    val isIdentity: Boolean get() = map == null

    /** Timeline sample at rendered sample [s]. */
    fun toTimeline(s: Double): Double = map?.toInput(s) ?: s

    /** Rendered sample at timeline sample [u]. */
    fun toRendered(u: Double): Double = map?.toOutput(u) ?: u

    companion object {
        fun identity(length: Int) = SpeedWarp(null, length)

        /**
         * @param tempo overall speed, 0.5 to 2 (2 talks twice as fast)
         * @param random 0..1: how often, and how far, the speed jumps away from [tempo] for a moment
         */
        fun build(length: Int, sampleRate: Int, tempo: Double, random: Double, seed: Long = length.toLong()): SpeedWarp {
            if (abs(tempo - 1.0) < 1e-6 && random <= 0.0) return identity(length)
            val rng = Random(seed xor 0x5DEECE66DL)
            val block = max(1, sampleRate / 200)
            val glide = 1.0 - exp(-block / (0.04 * sampleRate))
            val us = ArrayList<Double>()
            val ss = ArrayList<Double>()
            var u = 0
            var s = 0.0
            var speed = tempo
            var target = tempo
            var segmentLeft = 0
            while (u < length) {
                if (segmentLeft <= 0) {
                    segmentLeft = ((0.35 + 0.85 * rng.nextDouble()) * sampleRate).toInt()
                    target = tempo
                    if (rng.nextDouble() < random) {
                        val octaves = (0.2 + 0.8 * random) * (0.5 + 0.5 * rng.nextDouble())
                        target = (tempo * 2.0.pow(if (rng.nextBoolean()) octaves else -octaves)).coerceIn(0.35, 3.0)
                    }
                }
                speed += (target - speed) * glide
                us += u.toDouble()
                ss += s
                val step = min(block, length - u)
                u += step
                segmentLeft -= step
                s += step / speed
            }
            us += length.toDouble()
            ss += s
            return SpeedWarp(TimeMap(us.toDoubleArray(), ss.toDoubleArray()), max(1, ceil(s).toInt()))
        }
    }
}

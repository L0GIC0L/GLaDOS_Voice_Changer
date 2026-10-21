package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tanh

/** One-pole parameter smoother to avoid zipper noise when a slider moves. */
private class Smooth(private var v: Float, sampleRate: Int, seconds: Float = 0.015f) {
    private val coef = (1.0 - exp(-1.0 / (seconds * sampleRate))).toFloat()
    fun next(target: Float): Float {
        v += (target - v) * coef
        return v
    }
}

/** RBJ cookbook biquad. Coefficients in double; state in double so low cutoffs stay stable. */
private class Biquad {
    private var b0 = 1.0
    private var b1 = 0.0
    private var b2 = 0.0
    private var a1 = 0.0
    private var a2 = 0.0
    private var z1 = 0.0
    private var z2 = 0.0
    var bypass = true
        private set

    fun reset() {
        z1 = 0.0
        z2 = 0.0
    }

    private fun set(nb0: Double, nb1: Double, nb2: Double, na0: Double, na1: Double, na2: Double) {
        b0 = nb0 / na0; b1 = nb1 / na0; b2 = nb2 / na0
        a1 = na1 / na0; a2 = na2 / na0
        bypass = false
    }

    fun off() { bypass = true }

    fun highpass(f: Double, q: Double, sr: Int) {
        val w = 2 * PI * min(f, sr * 0.45) / sr
        val c = cos(w); val alpha = sin(w) / (2 * q)
        set((1 + c) / 2, -(1 + c), (1 + c) / 2, 1 + alpha, -2 * c, 1 - alpha)
    }

    fun lowpass(f: Double, q: Double, sr: Int) {
        val w = 2 * PI * min(f, sr * 0.45) / sr
        val c = cos(w); val alpha = sin(w) / (2 * q)
        set((1 - c) / 2, 1 - c, (1 - c) / 2, 1 + alpha, -2 * c, 1 - alpha)
    }

    fun peaking(f: Double, q: Double, gainDb: Double, sr: Int) {
        if (abs(gainDb) < 0.05) return off()
        val a = 10.0.pow(gainDb / 40)
        val w = 2 * PI * min(f, sr * 0.45) / sr
        val c = cos(w); val alpha = sin(w) / (2 * q)
        set(1 + alpha * a, -2 * c, 1 - alpha * a, 1 + alpha / a, -2 * c, 1 - alpha / a)
    }

    fun lowShelf(f: Double, gainDb: Double, sr: Int) {
        if (abs(gainDb) < 0.05) return off()
        val a = 10.0.pow(gainDb / 40)
        val w = 2 * PI * min(f, sr * 0.45) / sr
        val c = cos(w); val s = sin(w)
        val beta = 2 * sqrt(a) * (s / 2 * sqrt(2.0))
        set(
            a * ((a + 1) - (a - 1) * c + beta), 2 * a * ((a - 1) - (a + 1) * c), a * ((a + 1) - (a - 1) * c - beta),
            (a + 1) + (a - 1) * c + beta, -2 * ((a - 1) + (a + 1) * c), (a + 1) + (a - 1) * c - beta,
        )
    }

    fun highShelf(f: Double, gainDb: Double, sr: Int) {
        if (abs(gainDb) < 0.05) return off()
        val a = 10.0.pow(gainDb / 40)
        val w = 2 * PI * min(f, sr * 0.45) / sr
        val c = cos(w); val s = sin(w)
        val beta = 2 * sqrt(a) * (s / 2 * sqrt(2.0))
        set(
            a * ((a + 1) + (a - 1) * c + beta), -2 * a * ((a - 1) + (a + 1) * c), a * ((a + 1) + (a - 1) * c - beta),
            (a + 1) - (a - 1) * c + beta, 2 * ((a - 1) - (a + 1) * c), (a + 1) - (a - 1) * c - beta,
        )
    }

    fun process(x: Float): Float {
        if (bypass) return x
        val y = b0 * x + z1
        z1 = b1 * x - a1 * y + z2
        z2 = b2 * x - a2 * y
        return y.toFloat()
    }
}

/** Fractional delay line with linear interpolation. */
private class DelayLine(maxSamples: Int) {
    private val size = maxSamples + 4
    private val buf = FloatArray(size)
    private var w = 0

    fun clear() {
        buf.fill(0f)
    }

    fun write(x: Float) {
        buf[w] = x
        w = if (w + 1 == size) 0 else w + 1
    }

    /** Sample written [d] samples ago (d >= 1 counts the most recent write as 1). */
    fun read(d: Double): Float {
        val pos = w - d
        val i = floor(pos).toInt()
        val f = (pos - i).toFloat()
        val a = buf[Math.floorMod(i, size)]
        val b = buf[Math.floorMod(i + 1, size)]
        return a + (b - a) * f
    }
}

/** Short feedback comb with a slow LFO on the delay: the "metallic" and flanged character. */
private class ModComb(private val sr: Int) {
    private val line = DelayLine((sr * 0.04).toInt())
    private var lfo = 0.0
    private val mixS = Smooth(0f, sr)

    fun clear() = line.clear()

    fun process(x: Float, mix: Float, ms: Float, fb: Float, depth: Float, rateHz: Float): Float {
        lfo += 2 * PI * rateHz / sr
        if (lfo > 2 * PI) lfo -= 2 * PI
        val base = ms * 0.001 * sr
        val d = max(2.0, base * (1.0 + 0.5 * depth * sin(lfo)))
        val v = x + fb * line.read(d)
        line.write(v)
        val m = mixS.next(mix)
        return x * (1f - m) + v * 0.7f * m
    }
}

private class BitCrusher(sr: Int) {
    private var held = 0f
    private var count = 0
    private val mixS = Smooth(0f, sr)

    fun process(x: Float, mix: Float, bits: Int, hold: Int): Float {
        if (count <= 0) {
            held = x
            count = hold
        }
        count--
        val q = (1 shl (bits - 1)).toFloat()
        val crushed = Math.round(held * q) / q
        val m = mixS.next(mix)
        return x * (1f - m) + crushed * m
    }
}

/** Random stutter: now and then repeats the last few tens of milliseconds a couple of times. */
private class Glitch(private val sr: Int) {
    private val ring = FloatArray(sr / 2)
    private val loop = FloatArray(sr / 4)
    private var w = 0
    private var loopLen = 0
    private var pos = 0
    private var repeatsLeft = 0
    private var firstLoop = false
    private var level = 0f
    private var rng = 0x2545F491
    private val fade = max(8, sr / 1000)

    fun clear() {
        ring.fill(0f)
        repeatsLeft = 0
    }

    private fun rand01(): Float {
        rng = rng xor (rng shl 13)
        rng = rng xor (rng ushr 17)
        rng = rng xor (rng shl 5)
        return (rng ushr 8) / 16777216f
    }

    fun process(x: Float, eventsPerSecond: Float, lengthSamples: Int): Float {
        ring[w] = x
        w = if (w + 1 == ring.size) 0 else w + 1
        level += (abs(x) - level) * 0.001f
        if (repeatsLeft == 0) {
            if (eventsPerSecond > 0f && level > 0.004f && rand01() < eventsPerSecond / sr) {
                loopLen = lengthSamples.coerceIn(fade * 2, loop.size)
                for (i in 0 until loopLen) loop[i] = ring[Math.floorMod(w - loopLen + i, ring.size)]
                pos = 0
                repeatsLeft = 2 + (rand01() * 3).toInt()
                firstLoop = true
            }
            return x
        }
        var y = loop[pos]
        // Crossfade in at the very start and out at the end of the last repeat.
        if (firstLoop && pos < fade) {
            val t = pos.toFloat() / fade
            y = y * t + x * (1f - t)
        }
        if (repeatsLeft == 1 && pos >= loopLen - fade) {
            val t = (loopLen - pos).toFloat() / fade
            y = y * t + x * (1f - t)
        }
        if (++pos >= loopLen) {
            pos = 0
            firstLoop = false
            repeatsLeft--
        }
        return y
    }
}

private class Echo(private val sr: Int) {
    private val line = DelayLine(sr)
    private var damp = 0f
    private val mixS = Smooth(0f, sr)

    fun clear() {
        line.clear()
        damp = 0f
    }

    fun process(x: Float, mix: Float, ms: Float, fb: Float): Float {
        val d = line.read(max(2.0, ms * 0.001 * sr))
        damp += (d - damp) * 0.35f
        line.write(x + damp * fb)
        return x + d * mixS.next(mix)
    }
}

/** Freeverb: eight damped combs into four allpasses. Mono. */
private class Reverb(private val sr: Int) {
    private class Comb(val buf: FloatArray) {
        var i = 0
        var store = 0f
        fun process(x: Float, fb: Float, damp: Float): Float {
            val out = buf[i]
            store = out * (1f - damp) + store * damp
            buf[i] = x + store * fb
            if (++i == buf.size) i = 0
            return out
        }
    }

    private class AllPass(val buf: FloatArray) {
        var i = 0
        fun process(x: Float): Float {
            val b = buf[i]
            val out = -x + b
            buf[i] = x + b * 0.5f
            if (++i == buf.size) i = 0
            return out
        }
    }

    private val scale = sr / 44100.0
    private val combs = intArrayOf(1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617)
        .map { Comb(FloatArray(max(8, (it * scale).roundToInt()))) }
    private val allpasses = intArrayOf(556, 441, 341, 225)
        .map { AllPass(FloatArray(max(8, (it * scale).roundToInt()))) }
    private val mixS = Smooth(0f, sr)

    fun clear() {
        for (c in combs) { c.buf.fill(0f); c.store = 0f }
        for (a in allpasses) a.buf.fill(0f)
    }

    fun process(x: Float, mix: Float, size: Float, damping: Float): Float {
        val fb = 0.7f + 0.28f * size
        val damp = 0.1f + 0.6f * damping
        val input = x * 0.015f
        var sum = 0f
        for (c in combs) sum += c.process(input, fb, damp)
        for (a in allpasses) sum = a.process(sum)
        return x + sum * 3.0f * mixS.next(mix)
    }
}

private class Limiter(sr: Int) {
    private var gain = 1f
    private val release = (1.0 - exp(-1.0 / (0.080 * sr))).toFloat()
    private val ceiling = 0.89f
    private val knee = 0.9f

    fun process(x: Float): Float {
        val a = abs(x)
        val target = if (a > ceiling) ceiling / a else 1f
        gain = if (target < gain) target else gain + (1f - gain) * release
        val y = x * gain
        // Linear below the knee; only the last few percent are rounded off, so ordinary levels are untouched.
        val m = abs(y)
        if (m <= knee) return y
        val soft = knee + (1f - knee) * tanh((m - knee) / (1f - knee))
        return if (y < 0f) -soft else soft
    }
}

/**
 * The GLaDOS effect chain, applied to a mono stream: ring mod, metal comb, bit crush, glitch,
 * EQ, echo, reverb, output gain and a safety limiter. One instance per stream.
 * [configure] and [process] must be called from the same thread.
 */
class FxChain(private val sampleRate: Int) {
    private val comb = ModComb(sampleRate)
    private val crusher = BitCrusher(sampleRate)
    private val glitch = Glitch(sampleRate)
    private val echo = Echo(sampleRate)
    private val reverb = Reverb(sampleRate)
    private val limiter = Limiter(sampleRate)
    private val hpf = Biquad()
    private val lowShelf = Biquad()
    private val presence = Biquad()
    private val highShelf = Biquad()
    private val lpf = Biquad()

    private val ringMixS = Smooth(0f, sampleRate)
    private val outS = Smooth(1f, sampleRate)
    private var ringPhase = 0.0

    private var ringMix = 0f
    private var ringHz = 85.0
    private var combMix = 0f
    private var combMs = 3f
    private var combFb = 0.5f
    private var combDepth = 0.15f
    private var combRate = 0.35f
    private var crushMix = 0f
    private var crushBits = 10
    private var crushHold = 3
    private var glitchRate = 0f
    private var glitchLen = 3000
    private var echoMix = 0f
    private var echoMs = 220f
    private var echoFb = 0.35f
    private var reverbMix = 0f
    private var reverbSize = 0.55f
    private var reverbDamp = 0.5f
    private var outGain = 1f

    private var combOn = false
    private var crushOn = false
    private var glitchOn = false
    private var echoOn = false
    private var reverbOn = false

    fun configure(p: VoiceParams) {
        ringMix = p[Specs.RING_MIX] / 100f
        ringHz = p[Specs.RING_HZ].toDouble()
        combMix = p[Specs.COMB_MIX] / 100f
        combMs = p[Specs.COMB_MS]
        combFb = p[Specs.COMB_FB] / 100f
        combDepth = p[Specs.COMB_MOD] / 100f
        combRate = p[Specs.COMB_RATE]
        crushMix = p[Specs.CRUSH_MIX] / 100f
        crushBits = p.int(Specs.CRUSH_BITS)
        crushHold = p.int(Specs.CRUSH_HOLD)
        glitchRate = p[Specs.GLITCH] / 100f * 2.5f
        glitchLen = (p[Specs.GLITCH_MS] * 0.001f * sampleRate).roundToInt()
        echoMix = p[Specs.ECHO_MIX] / 100f
        echoMs = p[Specs.ECHO_MS]
        echoFb = p[Specs.ECHO_FB] / 100f
        reverbMix = p[Specs.REVERB_MIX] / 100f
        reverbSize = p[Specs.REVERB_SIZE] / 100f
        reverbDamp = p[Specs.REVERB_DAMP] / 100f
        outGain = 10f.pow(p[Specs.OUT_DB] / 20f)

        combOn = combMix > 0.001f
        crushOn = crushMix > 0.001f
        glitchOn = glitchRate > 0f
        echoOn = echoMix > 0.001f
        reverbOn = reverbMix > 0.001f
        if (!combOn) comb.clear()
        if (!glitchOn) glitch.clear()
        if (!echoOn) echo.clear()
        if (!reverbOn) reverb.clear()

        hpf.highpass(p[Specs.HPF].toDouble(), 0.707, sampleRate)
        lowShelf.lowShelf(200.0, p[Specs.LOW_DB].toDouble(), sampleRate)
        presence.peaking(p[Specs.MID_HZ].toDouble(), 1.0, p[Specs.MID_DB].toDouble(), sampleRate)
        highShelf.highShelf(6000.0, p[Specs.HIGH_DB].toDouble(), sampleRate)
        lpf.lowpass(p[Specs.LPF].toDouble(), 0.707, sampleRate)
    }

    fun reset() {
        comb.clear(); glitch.clear(); echo.clear(); reverb.clear()
        hpf.reset(); lowShelf.reset(); presence.reset(); highShelf.reset(); lpf.reset()
    }

    /** Processes [n] samples in place. */
    fun process(buf: FloatArray, n: Int) {
        val ringStep = 2 * PI * ringHz / sampleRate
        for (i in 0 until n) {
            var x = buf[i]

            val rm = ringMixS.next(ringMix)
            if (rm > 0.001f) {
                ringPhase += ringStep
                if (ringPhase > 2 * PI) ringPhase -= 2 * PI
                x = x * (1f - rm) + x * sin(ringPhase).toFloat() * rm
            }
            if (combOn) x = comb.process(x, combMix, combMs, combFb, combDepth, combRate)
            if (crushOn) x = crusher.process(x, crushMix, crushBits, crushHold)
            if (glitchOn) x = glitch.process(x, glitchRate, glitchLen)

            x = hpf.process(x)
            x = lowShelf.process(x)
            x = presence.process(x)
            x = highShelf.process(x)
            x = lpf.process(x)

            if (echoOn) x = echo.process(x, echoMix, echoMs, echoFb)
            if (reverbOn) x = reverb.process(x, reverbMix, reverbSize, reverbDamp)

            buf[i] = limiter.process(x * outS.next(outGain))
        }
    }

    companion object {
        /** Seconds of silence worth appending when rendering so echo and reverb tails are not cut off. */
        fun tailSeconds(p: VoiceParams): Double {
            var t = 0.0
            if (p[Specs.ECHO_MIX] > 0.5f) {
                val fb = (p[Specs.ECHO_FB] / 100.0).coerceAtMost(0.9)
                t = max(t, min(4.0, p[Specs.ECHO_MS] / 1000.0 * (1.0 + 4.0 * fb / (1.0 - fb) / 4.0) * 3.0))
            }
            if (p[Specs.REVERB_MIX] > 0.5f) t = max(t, 1.0 + 3.0 * p[Specs.REVERB_SIZE] / 100.0)
            return t
        }
    }
}

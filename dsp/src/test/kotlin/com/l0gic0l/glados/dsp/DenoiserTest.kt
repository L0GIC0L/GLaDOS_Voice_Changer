package com.l0gic0l.glados.dsp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Random
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.log10
import kotlin.math.sqrt

class DenoiserTest {
    private val sr = 44100

    /** Three vowel "syllables" with pauses between them and room tone before and after. */
    private fun phrase(): FloatArray {
        val out = FloatArray((sr * 2.4).toInt())
        var t = (0.3 * sr).toInt()
        for ((k, hz) in doubleArrayOf(140.0, 180.0, 120.0).withIndex()) {
            val seg = TestSignals.voice(sr, 0.4, { hz }, seed = k + 1L)
            val ramp = (0.02 * sr).toInt()
            for (i in seg.indices) out[t + i] = seg[i] * minOf(1f, i.toFloat() / ramp, (seg.size - i).toFloat() / ramp)
            t += seg.size + (0.25 * sr).toInt()
        }
        return out
    }

    private fun noise(n: Int, rms: Double, seed: Long): FloatArray {
        val r = Random(seed)
        return FloatArray(n) { (r.nextGaussian() * rms).toFloat() }
    }

    private fun plus(a: FloatArray, b: FloatArray) = FloatArray(a.size) { a[it] + b[it] }

    private fun snr(clean: FloatArray, test: FloatArray): Double {
        var s = 0.0
        var e = 0.0
        for (i in clean.indices) {
            s += clean[i] * clean[i].toDouble()
            val d = test[i] - clean[i].toDouble()
            e += d * d
        }
        return 10 * log10(s / e)
    }

    /** RMS over the pauses, where the clean phrase is silent. */
    private fun pauseRms(clean: FloatArray, x: FloatArray): Double {
        var s = 0.0
        var n = 0
        for (i in clean.indices) if (clean[i] == 0f) { s += x[i] * x[i].toDouble(); n++ }
        return sqrt(s / n)
    }

    @Test
    fun fftRoundTripsAndFindsATone() {
        val n = 512
        val fft = Fft(n)
        val r = Random(3)
        val re = DoubleArray(n) { r.nextGaussian() }
        val im = DoubleArray(n)
        val orig = re.copyOf()
        fft.transform(re, im, inverse = false)
        fft.transform(re, im, inverse = true)
        for (i in 0 until n) {
            assertEquals(orig[i], re[i], 1e-9)
            assertEquals(0.0, im[i], 1e-9)
        }
        val tone = DoubleArray(n) { cos(2 * PI * 5 * it / n) }
        val ti = DoubleArray(n)
        fft.transform(tone, ti, inverse = false)
        assertEquals(n / 2.0, tone[5], 1e-6)
        assertEquals(n / 2.0, tone[n - 5], 1e-6)
        assertEquals(0.0, tone[6], 1e-6)
    }

    @Test
    fun zeroAmountAndSilenceAreLeftAlone() {
        val x = phrase()
        assertSame(x, Denoiser.process(x, sr, 0f))
        val silent = FloatArray(sr)
        assertSame(silent, Denoiser.process(silent, sr, 1f))
    }

    @Test
    fun cleanSpeechIsLeftAlmostUntouched() {
        val clean = phrase()
        val x = plus(clean, noise(clean.size, 1e-5, 1)) // a -100 dB noise floor, like a good converter
        for (amount in floatArrayOf(0.5f, 1f)) {
            val y = Denoiser.process(x, sr, amount)
            assertEquals(x.size, y.size)
            val s = snr(x, y)
            println("clean speech at $amount: output differs from input by ${"%.1f".format(s)} dB SNR")
            assertTrue("clean speech changed too much: $s dB", s > 35.0)
        }
    }

    @Test
    fun whiteNoiseIsRemoved() {
        val clean = phrase()
        var speech = 0.0
        for (v in clean) speech += v * v.toDouble()
        val sigma = sqrt(speech / clean.size) / Math.pow(10.0, 5.0 / 20) // 5 dB SNR
        val noisy = plus(clean, noise(clean.size, sigma, 2))
        val before = snr(clean, noisy)
        for (amount in floatArrayOf(0.25f, 0.5f, 1f)) {
            val y = Denoiser.process(noisy, sr, amount)
            for (v in y) assertFalse(v.isNaN() || v.isInfinite())
            val gain = snr(clean, y) - before
            val pauseDrop = 20 * log10(pauseRms(clean, noisy) / pauseRms(clean, y))
            println("white noise at 5 dB, amount $amount: SNR +${"%.1f".format(gain)} dB, pauses down ${"%.1f".format(pauseDrop)} dB")
            assertTrue("SNR should improve at $amount: $gain", gain > 3.0)
            assertTrue("pauses should get quieter at $amount: $pauseDrop", pauseDrop > 6.0 * amount)
        }
    }

    @Test
    fun aRecorderWarmUpOfDigitalSilenceDoesNotHideTheNoise() {
        val clean = phrase()
        val noisy = plus(clean, noise(clean.size, 0.02, 4))
        for (i in 0 until (0.1 * sr).toInt()) noisy[i] = 0f
        val y = Denoiser.process(noisy, sr, 0.5f)
        val drop = 20 * log10(pauseRms(clean, noisy) / pauseRms(clean, y))
        assertTrue("pauses down only $drop dB", drop > 6.0)
    }

    @Test
    fun veryShortClipsWork() {
        val x = noise(100, 0.1, 5)
        val y = Denoiser.process(x, 8000, 0.7f)
        assertEquals(100, y.size)
        for (v in y) assertFalse(v.isNaN())
    }
}

package com.l0gic0l.glados.dsp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Random

class YinTest {
    @Test
    fun detectsSteadyVoicePitchAcrossRange() {
        for (sr in intArrayOf(48000, 44100, 22050)) {
            for (hz in doubleArrayOf(82.0, 110.0, 150.0, 220.0, 330.0, 523.0)) {
                val x = TestSignals.voice(sr, 0.6, { hz })
                val track = TestSignals.pitchTrack(x, sr, hop = 256)
                val med = TestSignals.median(track)
                assertEquals("sr=$sr hz=$hz", 0.0, TestSignals.cents(med, hz), 6.0)
                val voiced = track.count { it > 0 }
                assertTrue("sr=$sr hz=$hz voiced $voiced/${track.size}", voiced >= track.size - 1)
            }
        }
    }

    @Test
    fun noiseIsNotVoiced() {
        val rnd = Random(3)
        val x = FloatArray(48000) { (rnd.nextGaussian() * 0.1).toFloat() }
        val track = TestSignals.pitchTrack(x, 48000, hop = 512)
        assertTrue("false positives ${track.count { it > 0 }}/${track.size}", track.count { it > 0 } <= track.size / 10)
    }

    @Test
    fun silenceIsNotVoiced() {
        val x = FloatArray(24000)
        val yin = Yin(48000, 80.0, 700.0)
        assertFalse(yin.detect(x, -1, 0, 0.15))
    }

    @Test
    fun tracksGlide() {
        val x = TestSignals.voice(48000, 1.0, { 120.0 + 80.0 * it })
        val track = TestSignals.pitchTrack(x, 48000, hop = 480)
        val yin = Yin(48000, 60.0, 900.0)
        for (i in track.indices) {
            val centre = (i * 480 + yin.windowLength / 2) / 48000.0
            val expected = 120.0 + 80.0 * centre
            assertEquals("frame $i", 0.0, TestSignals.cents(track[i], expected), 40.0)
        }
    }
}

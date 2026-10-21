package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Random
import kotlin.math.PI
import kotlin.math.sin

/** Guards against the voicing detector going deaf on realistic recordings (rumble, hiss, a quiet mic). */
class VoicingTest {
    private val sr = 16000

    /** Three vowel-like syllables with gaps, roughly one second each, at a speaking pitch. */
    private fun phrase(): FloatArray {
        val x = FloatArray(sr * 4)
        val hz = doubleArrayOf(118.0, 135.0, 110.0)
        var t = (0.2 * sr).toInt()
        for (k in 0..2) {
            val seg = TestSignals.voice(sr, 0.7, { hz[k] * (1 + 0.05 * sin(2 * PI * 3 * it)) }, jitter = 0.006, seed = k + 7L)
            val ramp = (0.03 * sr).toInt()
            for (i in seg.indices) x[t + i] = seg[i] * minOf(1f, i.toFloat() / ramp, (seg.size - i).toFloat() / ramp)
            t += (1.1 * sr).toInt()
        }
        return x
    }

    private fun withNoise(x: FloatArray, snrDb: Double): FloatArray {
        val r = Random(3)
        val sigma = TestSignals.rms(x) / Math.pow(10.0, snrDb / 20)
        return FloatArray(x.size) { (x[it] + sigma * r.nextGaussian()).toFloat() }
    }

    private fun withRumble(x: FloatArray, amount: Double): FloatArray {
        val a = (TestSignals.rms(x) * amount).toFloat()
        return FloatArray(x.size) { (x[it] + a * (sin(2 * PI * 38.0 * it / sr) + 0.6 * sin(2 * PI * 17.0 * it / sr))).toFloat() }
    }

    private fun voicedFrames(x: FloatArray, p: VoiceParams = VoiceParams.defaults()): Int =
        Analyzer.analyze(x, sr, p).voiced.count { it }

    @Test
    fun higherSensitivityNeverFindsLessVoice() {
        val noisy = withNoise(phrase(), 6.0)
        var last = -1
        for (s in intArrayOf(0, 25, 50, 75, 100)) {
            val n = voicedFrames(noisy, VoiceParams.defaults().with(Specs.VOICING, s.toFloat()))
            println("voicing $s%: $n voiced frames")
            assertTrue("voiced frames fell from $last to $n when sensitivity rose to $s%", n >= last)
            last = n
        }
    }

    @Test
    fun sensitivityMapsToThresholdInTheRightDirection() {
        assertTrue(Voicing.threshold(100f) > Voicing.threshold(60f))
        assertTrue(Voicing.threshold(60f) > Voicing.threshold(0f))
        assertEquals(0.08, Voicing.threshold(0f), 1e-9)
        assertEquals(0.40, Voicing.threshold(100f), 1e-9)
    }

    @Test
    fun defaultSettingsFindMostOfTheVoiceDespiteRumble() {
        val clean = voicedFrames(phrase())
        for ((label, sig) in listOf(
            // Rumble at twice the voice level silences a detector that listens to the full spectrum.
            "rumble 2x" to withRumble(phrase(), 2.0),
            "hiss 6 dB" to withNoise(phrase(), 6.0),
            "rumble + hiss" to withNoise(withRumble(phrase(), 1.5), 12.0),
        )) {
            val n = voicedFrames(sig)
            println("$label: $n of $clean clean-voiced frames")
            assertTrue("$label: only $n of $clean voiced frames found", n >= clean * 0.8)
        }
    }

    @Test
    fun quietRecordingIsStillAnalysed() {
        val quiet = phrase().also { for (i in it.indices) it[i] *= 0.004f } // about -48 dBFS peak
        val n = voicedFrames(quiet)
        assertTrue("quiet clip found only $n voiced frames", n >= voicedFrames(phrase()) * 0.9)
    }

    @Test
    fun notesFollowSyllablesOnNoisyAudio() {
        val an = Analyzer.analyze(withNoise(withRumble(phrase(), 1.5), 12.0), sr, VoiceParams.defaults())
        val notes = NoteOps.segment(an).notes.size
        println("noisy phrase: $notes notes for 3 syllables")
        assertTrue("expected about 3 notes, got $notes", notes in 3..6)
    }

    @Test
    fun liveEngineHearsVoiceThroughRumbleAndHiss() {
        val sig = withNoise(withRumble(phrase(), 2.0), 12.0)
        val e = LivePitchEngine(sr, VoiceParams.defaults())
        val bi = FloatArray(128)
        val bo = FloatArray(128)
        var voiced = 0
        var frames = 0
        var i = 0
        while (i + 128 <= sig.size) {
            System.arraycopy(sig, i, bi, 0, 128)
            e.process(bi, bo, 128)
            // Count only the stretches where a syllable is sounding.
            val t = i.toDouble() / sr
            val inSyllable = (0..2).any { t in (0.2 + it * 1.1 + 0.15)..(0.2 + it * 1.1 + 0.55) }
            if (inSyllable) {
                frames++
                if (e.inputHz > 0) voiced++
            }
            i += 128
        }
        println("live: $voiced of $frames frames inside syllables were detected as voice")
        assertTrue("live engine detected only $voiced of $frames voiced frames", voiced >= frames * 0.75)
    }
}

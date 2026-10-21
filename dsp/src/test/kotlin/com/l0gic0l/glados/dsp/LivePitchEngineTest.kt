package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.round

class LivePitchEngineTest {
    private val sr = 48000

    private fun base(): VoiceParams = VoiceParams.defaults().withAll(
        mapOf(
            "autoReg" to 0f, "formant" to 0f, "formantFollow" to 0f, "strength" to 100f, "retuneMs" to 0f, "shift" to 0f,
            "gateDb" to -80f, "breath" to 100f, "dry" to 0f,
        ),
    )

    private fun run(x: FloatArray, p: VoiceParams, block: Int = 128): Pair<FloatArray, LivePitchEngine> {
        val e = LivePitchEngine(sr, p)
        val out = FloatArray(x.size)
        val inB = FloatArray(block)
        val outB = FloatArray(block)
        var i = 0
        while (i < x.size) {
            val n = minOf(block, x.size - i)
            System.arraycopy(x, i, inB, 0, n)
            e.process(inB, outB, n)
            System.arraycopy(outB, 0, out, i, n)
            i += n
        }
        return out to e
    }

    private fun assertSane(y: FloatArray) {
        for (v in y) {
            assertFalse("NaN/Inf in output", v.isNaN() || v.isInfinite())
            assertTrue("output exceeds range: $v", abs(v) < 2f)
        }
    }

    @Test
    fun hardTuneSnapsToNearestSemitone() {
        // 150 Hz is 50.3 in MIDI: nearest note is D3 (50) = 146.83 Hz.
        val x = TestSignals.voice(sr, 2.0, { 150.0 })
        val (y, e) = run(x, base())
        assertSane(y)
        assertEquals(0, e.starvedGrains)
        val from = sr / 2
        val track = TestSignals.pitchTrack(y, sr, from, y.size)
        val expected = Pitch.midiToHz(50.0)
        assertEquals(0.0, TestSignals.cents(TestSignals.median(track), expected), 8.0)
        val voiced = track.count { it > 0 }
        assertTrue("voiced $voiced/${track.size}", voiced > track.size * 0.9)
        // The corrected pitch should be steady, not wobbling with the input jitter.
        val spread = track.filter { it > 0 }.maxOf { abs(TestSignals.cents(it, expected)) }
        assertTrue("max deviation $spread cents", spread < 25.0)
    }

    @Test
    fun pitchShiftMovesAnOctave() {
        val x = TestSignals.voice(sr, 1.5, { 140.0 })
        val p = base().with(Specs.STRENGTH, 0f).with(Specs.SHIFT, 12f)
        val (y, e) = run(x, p)
        assertSane(y)
        assertEquals(0, e.starvedGrains)
        val track = TestSignals.pitchTrack(y, sr, sr / 2, y.size)
        assertEquals(0.0, TestSignals.cents(TestSignals.median(track), 280.0), 15.0)
    }

    @Test
    fun octaveDownIsClean() {
        val x = TestSignals.voice(sr, 1.5, { 240.0 })
        val p = base().with(Specs.STRENGTH, 0f).with(Specs.SHIFT, -12f)
        val (y, e) = run(x, p)
        assertSane(y)
        assertEquals(0, e.starvedGrains)
        val track = TestSignals.pitchTrack(y, sr, sr / 2, y.size)
        println("octave-down median=${TestSignals.median(track)} voiced=${track.count { it > 0 }}/${track.size} sample=${track.take(12).map { it.toInt() }}")
        assertEquals(0.0, TestSignals.cents(TestSignals.median(track), 120.0), 15.0)
    }

    @Test
    fun formantShiftLeavesPitchAlone() {
        val x = TestSignals.voice(sr, 1.5, { 150.0 })
        val p0 = base().with(Specs.STRENGTH, 0f)
        val (y0, _) = run(x, p0)
        val (y1, _) = run(x, p0.with(Specs.FORMANT, 5f))
        val t0 = TestSignals.median(TestSignals.pitchTrack(y0, sr, sr / 2, y0.size))
        val t1 = TestSignals.median(TestSignals.pitchTrack(y1, sr, sr / 2, y1.size))
        assertEquals("pitch moved by formant shift", 0.0, TestSignals.cents(t1, t0), 15.0)
        val c0 = TestSignals.spectralCentroid(y0, sr, sr, sr + 4800)
        val c1 = TestSignals.spectralCentroid(y1, sr, sr, sr + 4800)
        println("centroid no-shift=$c0 shifted=$c1")
        assertTrue("formant shift should raise the spectral centroid ($c0 -> $c1)", c1 > c0 * 1.1)
    }

    @Test
    fun formantFollowsPitchShift() {
        val x = TestSignals.voice(sr, 1.5, { 140.0 })
        val p = base().with(Specs.STRENGTH, 0f).with(Specs.SHIFT, 7f)
        val (flat, _) = run(x, p)
        val (follow, _) = run(x, p.with(Specs.FORMANT_FOLLOW, 100f))
        val c0 = TestSignals.spectralCentroid(flat, sr, sr, sr + 4800)
        val c1 = TestSignals.spectralCentroid(follow, sr, sr, sr + 4800)
        println("follow centroid $c0 -> $c1")
        assertTrue("formants should rise when following the shift ($c0 -> $c1)", c1 > c0 * 1.15)
        val t0 = TestSignals.median(TestSignals.pitchTrack(flat, sr, sr / 2, flat.size))
        val t1 = TestSignals.median(TestSignals.pitchTrack(follow, sr, sr / 2, follow.size))
        assertEquals(0.0, TestSignals.cents(t1, t0), 15.0)
    }

    @Test
    fun noCorrectionIsNearlyTransparent() {
        // PSOLA keeps pitch and waveform shape but may slip by up to half a period, so compare
        // short windows allowing a local lag search, and check the level is preserved.
        val raw = TestSignals.voice(sr, 1.5, { 150.0 }, jitter = 0.0)
        // The engine high-passes its input at ~15 Hz; apply the same filter to the reference.
        val x = FloatArray(raw.size)
        var px = 0f
        var py = 0f
        for (i in raw.indices) {
            val v = raw[i] - px + 0.998f * py
            px = raw[i]
            py = v
            x[i] = v
        }
        val p = base().with(Specs.STRENGTH, 0f)
        val (y, e) = run(raw, p)
        assertSane(y)
        val lag = e.latencySamples
        val win = 960
        val residuals = ArrayList<Double>()
        var start = sr
        while (start + win < y.size - 400) {
            var best = Double.MAX_VALUE
            for (d in lag - 330..lag + 330) {
                var err = 0.0
                var ref = 0.0
                for (i in start until start + win) {
                    val diff = y[i] - x[i - d].toDouble()
                    err += diff * diff
                    ref += x[i - d].toDouble() * x[i - d]
                }
                best = minOf(best, err / ref)
            }
            residuals += 10 * Math.log10(best)
            start += 2400
        }
        val median = residuals.sorted()[residuals.size / 2]
        println("transparent local residual median=$median dB all=${residuals.map { it.toInt() }}")
        assertTrue("residual $median dB", median < -24.0)
        val ratio = TestSignals.rms(y, sr, y.size - 400) / TestSignals.rms(x, sr, y.size - 400)
        assertEquals("level ratio", 1.0, ratio, 0.12)
    }

    @Test
    fun glideBecomesStaircase() {
        val x = TestSignals.voice(sr, 2.0, { 110.0 + 70.0 * it / 2.0 })
        val (y, _) = run(x, base())
        val track = TestSignals.pitchTrack(y, sr, sr / 4, y.size, hop = 480)
        var off = 0
        var n = 0
        for (f in track) {
            if (f <= 0) continue
            val m = Pitch.hzToMidi(f)
            val dev = abs(m - round(m)) * 100
            n++
            if (dev > 30) off++
        }
        println("off-grid frames $off/$n")
        assertTrue("too many frames off the semitone grid: $off/$n", off <= max(2, n / 8))
    }

    @Test
    fun customScaleOnlyUsesAllowedNotes() {
        // C major only: every output note must be a white key.
        val x = TestSignals.voice(sr, 2.0, { 120.0 + 90.0 * it / 2.0 })
        val p = base().with(Specs.SCALE, 1f).with(Specs.KEY, 0f)
        val (y, _) = run(x, p)
        val track = TestSignals.pitchTrack(y, sr, sr / 4, y.size, hop = 480)
        val white = setOf(0, 2, 4, 5, 7, 9, 11)
        var bad = 0
        var n = 0
        for (f in track) {
            if (f <= 0) continue
            val m = round(Pitch.hzToMidi(f)).toInt()
            n++
            if (Math.floorMod(m, 12) !in white) bad++
        }
        println("non-scale frames $bad/$n")
        assertTrue("non-scale frames $bad/$n", bad <= max(2, n / 8))
    }

    @Test
    fun monotoneHoldsOneNote() {
        val x = TestSignals.voice(sr, 2.0, { 130.0 + 30.0 * kotlin.math.sin(it * 3.0) })
        val p = base().with(Specs.MONOTONE, 1f).with(Specs.MONO_NOTE, 57f)
        val (y, _) = run(x, p)
        val track = TestSignals.pitchTrack(y, sr, sr / 2, y.size, hop = 480)
        val target = Pitch.midiToHz(57.0)
        val bad = track.count { it > 0 && abs(TestSignals.cents(it, target)) > 40 }
        println("monotone off-note frames $bad/${track.size}")
        assertTrue("off-note frames $bad", bad <= track.size / 8)
    }

    @Test
    fun autoRegisterMovesVoiceTowardTarget() {
        val x = TestSignals.voice(sr, 4.0, { 110.0 })
        val p = base().with(Specs.AUTO_REG, 1f).with(Specs.REG_HZ, 220f).with(Specs.STRENGTH, 0f)
        val (y, _) = run(x, p)
        val track = TestSignals.pitchTrack(y, sr, 3 * sr, y.size)
        assertEquals(0.0, TestSignals.cents(TestSignals.median(track), 220.0), 40.0)
    }

    @Test
    fun silenceStaysSilent() {
        val (y, e) = run(FloatArray(sr), base())
        assertSane(y)
        assertTrue(TestSignals.rms(y) < 1e-6)
        assertEquals(0, e.starvedGrains)
    }

    @Test
    fun unvoicedNoisePassesThroughAtComparableLevel() {
        val rnd = java.util.Random(5)
        val x = FloatArray(sr) { (rnd.nextGaussian() * 0.05).toFloat() }
        val (y, _) = run(x, base())
        val rin = TestSignals.rms(x, sr / 2, sr)
        val rout = TestSignals.rms(y, sr / 2, sr)
        println("noise rms in=$rin out=$rout")
        assertTrue("noise level changed too much: $rin -> $rout", rout > rin * 0.5 && rout < rin * 1.6)
    }

    @Test
    fun oddBlockSizesAreEquivalentlyStable() {
        val x = TestSignals.voice(sr, 1.5, { 176.0 })
        for (block in intArrayOf(64, 100, 240, 480, 1024)) {
            val (y, e) = run(x, base(), block)
            assertSane(y)
            assertEquals("block $block", 0, e.starvedGrains)
            val med = TestSignals.median(TestSignals.pitchTrack(y, sr, sr / 2, y.size))
            val expected = Pitch.midiToHz(round(Pitch.hzToMidi(176.0)))
            assertEquals("block $block", 0.0, TestSignals.cents(med, expected), 12.0)
        }
    }

    @Test
    fun speechLikeBurstsDoNotClick() {
        // Voiced bursts separated by silence: output should not contain spikes far above the input peak.
        val seg = TestSignals.voice(sr, 0.4, { 140.0 + 20.0 * it })
        val x = FloatArray(sr * 3)
        for (k in 0 until 3) System.arraycopy(seg, 0, x, k * sr + 4000, seg.size)
        val (y, _) = run(x, base())
        assertSane(y)
        var peakIn = 0f
        var peakOut = 0f
        for (v in x) peakIn = max(peakIn, abs(v))
        for (v in y) peakOut = max(peakOut, abs(v))
        println("peak in=$peakIn out=$peakOut")
        assertTrue("output peak $peakOut vs input $peakIn", peakOut < peakIn * 1.6f)
    }
}

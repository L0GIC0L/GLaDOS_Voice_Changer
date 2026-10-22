package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Presets
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Random
import kotlin.math.abs

class FxChainTest {
    private val sr = 48000

    private fun process(x: FloatArray, p: VoiceParams, block: Int = 256): FloatArray {
        val fx = FxChain(sr)
        fx.configure(p)
        val y = x.copyOf()
        var i = 0
        val tmp = FloatArray(block)
        while (i < y.size) {
            val n = minOf(block, y.size - i)
            System.arraycopy(y, i, tmp, 0, n)
            fx.process(tmp, n)
            System.arraycopy(tmp, 0, y, i, n)
            i += n
        }
        return y
    }

    private fun speechLike(): FloatArray = TestSignals.voice(sr, 1.5, { 140.0 + 40.0 * kotlin.math.sin(it * 4.0) }, amp = 0.5f)

    private fun assertFiniteAndBounded(y: FloatArray, label: String) {
        for (v in y) {
            assertFalse("$label: non-finite", v.isNaN() || v.isInfinite())
            assertTrue("$label: out of range $v", abs(v) <= 1.0f)
        }
    }

    @Test
    fun everyPresetIsStable() {
        val x = speechLike()
        for (preset in Presets.all) {
            assertFiniteAndBounded(process(x, preset.apply()), preset.name)
        }
    }

    @Test
    fun everyParameterAtItsExtremesIsStable() {
        val x = speechLike()
        for (spec in Specs.all) {
            for (v in floatArrayOf(spec.min, spec.max)) {
                val p = VoiceParams.defaults().with(spec, v)
                assertFiniteAndBounded(process(x, p), "${spec.id}=$v")
            }
        }
    }

    @Test
    fun allEffectsMaxedStaysBounded() {
        val maxed = Specs.all.filter { !it.hidden }.associate { spec ->
            spec.id to when (spec) {
                Specs.COMB_FB -> 95f
                Specs.CRUSH_BITS -> 3f
                else -> spec.max
            }
        }
        val y = process(speechLike(), VoiceParams.fromMap(maxed))
        assertFiniteAndBounded(y, "maxed")
    }

    @Test
    fun limiterHoldsLoudInput() {
        val loud = FloatArray(sr) { (5.0 * kotlin.math.sin(2 * Math.PI * 220.0 * it / sr)).toFloat() }
        val p = Presets.all.first { it.name == "Natural" }.apply() // no effects
        val y = process(loud, p)
        var peak = 0f
        for (v in y) peak = maxOf(peak, abs(v))
        assertTrue("peak $peak", peak <= 0.98f)
    }

    @Test
    fun effectsOffIsTransparentApartFromEq() {
        val x = speechLike()
        val p = VoiceParams.defaults().withAll(
            mapOf(
                "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f, "echoMix" to 0f, "reverbMix" to 0f,
                "hpf" to 20f, "lpf" to 20000f, "lowDb" to 0f, "midDb" to 0f, "highDb" to 0f, "outDb" to 0f,
            ),
        )
        val y = process(x, p)
        // The 20 Hz high-pass legitimately shifts the phase of a 140 Hz voice, so judge level and shape, not sample equality.
        var xy = 0.0
        var xx = 0.0
        var yy = 0.0
        for (i in 4800 until x.size) {
            xy += x[i].toDouble() * y[i]
            xx += x[i].toDouble() * x[i]
            yy += y[i].toDouble() * y[i]
        }
        val corr = xy / Math.sqrt(xx * yy)
        val level = Math.sqrt(yy / xx)
        println("fx-off correlation=$corr level=$level")
        assertTrue("correlation $corr", corr > 0.95)
        assertEquals("level", 1.0, level, 0.05)
    }

    @Test
    fun echoRepeatsAtTheRequestedTime() {
        val impulse = FloatArray(sr)
        impulse[1000] = 0.5f
        val p = VoiceParams.defaults().withAll(
            mapOf(
                "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f, "reverbMix" to 0f,
                "echoMix" to 60f, "echoMs" to 250f, "echoFb" to 0f,
                "hpf" to 20f, "lpf" to 20000f, "lowDb" to 0f, "midDb" to 0f, "highDb" to 0f,
            ),
        )
        val y = process(impulse, p)
        var best = 0
        var bestV = 0f
        for (i in 3000 until y.size) if (abs(y[i]) > bestV) { bestV = abs(y[i]); best = i }
        assertEquals(1000 + 0.25 * sr, best.toDouble(), 30.0)
        assertTrue("echo level $bestV", bestV > 0.05f)
    }

    @Test
    fun reverbAddsATailAtSensibleLevel() {
        val rnd = Random(2)
        val burst = FloatArray(sr * 2)
        for (i in 0 until 4800) burst[i] = (rnd.nextGaussian() * 0.2).toFloat()
        val p = VoiceParams.defaults().withAll(
            mapOf(
                "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f, "echoMix" to 0f,
                "reverbMix" to 50f, "reverbSize" to 70f,
                "hpf" to 20f, "lpf" to 20000f, "lowDb" to 0f, "midDb" to 0f, "highDb" to 0f,
            ),
        )
        val y = process(burst, p)
        val tail = TestSignals.rms(y, 9600, 19200)
        val body = TestSignals.rms(burst, 0, 4800)
        println("reverb tail rms=$tail burst rms=$body ratio=${tail / body}")
        assertTrue("tail should be audible but below the source: ${tail / body}", tail / body in 0.02..0.8)
        assertTrue("tail should decay", TestSignals.rms(y, 76800, 86400) < tail)
    }

    @Test
    fun glitchFiresAtRoughlyTheRequestedRate() {
        val x = FloatArray(sr * 20) { (0.3 * kotlin.math.sin(2 * Math.PI * 200.0 * it / sr)).toFloat() * (1f + 0.5f * kotlin.math.sin(it * 0.0005f)) }
        val clean = process(x, VoiceParams.defaults().withAll(mapOf("glitch" to 0f, "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "reverbMix" to 0f)))
        val glitchy = process(x, VoiceParams.defaults().withAll(mapOf("glitch" to 60f, "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "reverbMix" to 0f)))
        var changedBlocks = 0
        var i = 0
        while (i + 4800 < x.size) {
            var d = 0.0
            for (k in i until i + 4800) d += abs(glitchy[k] - clean[k]).toDouble()
            if (d / 4800 > 0.01) changedBlocks++
            i += 4800
        }
        println("glitched blocks $changedBlocks of ${x.size / 4800}")
        assertTrue("glitch never fired", changedBlocks > 3)
        assertTrue("glitch fired constantly", changedBlocks < x.size / 4800)
    }

    @Test
    fun everyGlitchKindChangesVoiceButLeavesSilenceSilent() {
        val x = FloatArray(sr * 10) { (0.3 * kotlin.math.sin(2 * Math.PI * 200.0 * it / sr)).toFloat() * (1f + 0.5f * kotlin.math.sin(it * 0.0005f)) }
        val base = Presets.all.first { it.name == "Natural" }.apply()
        val clean = process(x, base)
        for (spec in listOf(Specs.GLITCH, Specs.GLITCH_REVERSE, Specs.GLITCH_TAPE, Specs.GLITCH_PITCH, Specs.GLITCH_DROP, Specs.GLITCH_STATIC, Specs.BUZZ)) {
            val p = base.with(spec, 100f)
            val y = process(x, p)
            var changedBlocks = 0
            var i = 0
            while (i + 4800 < x.size) {
                var d = 0.0
                for (k in i until i + 4800) d += abs(y[k] - clean[k]).toDouble()
                if (d / 4800 > 0.01) changedBlocks++
                i += 4800
            }
            println("${spec.id}: changed $changedBlocks of ${x.size / 4800} blocks")
            assertTrue("${spec.id} never changed anything", changedBlocks > 2)
            assertFiniteAndBounded(y, spec.id)
            val quiet = process(FloatArray(sr * 2), p)
            assertTrue("${spec.id} made sound out of silence", quiet.all { it == 0f })
        }
    }

    @Test
    fun tailLengthIsZeroWithoutTimeEffects() {
        val p = VoiceParams.defaults().withAll(mapOf("echoMix" to 0f, "reverbMix" to 0f))
        assertEquals(0.0, FxChain.tailSeconds(p), 1e-9)
        assertTrue(FxChain.tailSeconds(p.with(Specs.REVERB_MIX, 30f)) > 1.0)
    }
}

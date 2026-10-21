package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Presets
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelTest {
    @Test
    fun specIdsAreUniqueAndDefaultsInRange() {
        assertEquals(Specs.all.size, Specs.all.map { it.id }.toSet().size)
        for (s in Specs.all) {
            assertTrue("${s.id} min<max", s.min < s.max)
            assertTrue("${s.id} default in range", s.default in s.min..s.max)
            assertEquals(s.id, Specs.all[s.index].id)
        }
    }

    @Test
    fun presetsOnlyReferenceKnownParametersAndStayInRange() {
        for (p in Presets.all) {
            for ((id, v) in p.overrides) {
                val spec = Specs.byId[id] ?: error("preset ${p.name} references unknown parameter $id")
                assertTrue("${p.name}.$id=$v out of [${spec.min}, ${spec.max}]", v in spec.min..spec.max)
            }
        }
        assertEquals(VoiceParams.defaults(), Presets.all.first().apply())
    }

    @Test
    fun paramsRoundTripThroughMapAndClamp() {
        val p = VoiceParams.defaults().with(Specs.RETUNE, 123f).with(Specs.KEY, 7f)
        assertEquals(p, VoiceParams.fromMap(p.toMap()))
        for (spec in Specs.all) assertEquals("${spec.id} default survives clamp", spec.default, spec.clamp(spec.default), 0f)
        val wild = VoiceParams.defaults().with(Specs.RETUNE, 99999f).with(Specs.SHIFT, -99f)
        assertEquals(Specs.RETUNE.max, wild[Specs.RETUNE], 0f)
        assertEquals(Specs.SHIFT.min, wild[Specs.SHIFT], 0f)
        assertEquals(VoiceParams.defaults(), VoiceParams.fromMap(mapOf("doesNotExist" to 3f)))
    }

    @Test
    fun formattingIsReadable() {
        assertEquals("20 ms", Specs.RETUNE.format(20f))
        assertEquals("0.0 st", Specs.SHIFT.format(0f))
        assertEquals("A3", Specs.MONO_NOTE.format(57f))
        assertEquals("On", Specs.MONOTONE.format(1f))
        assertEquals("D", Specs.KEY.format(2f))
    }

    @Test
    fun scaleMasksAndNearestNote() {
        val cMajor = Scales.mask(1, 0, 0)
        assertEquals(0b101010110101, cMajor)
        assertEquals(62, Scales.nearestAllowed(61.4, cMajor)) // C# is out of key; D is 0.6 away, C is 1.4
        assertEquals(60, Scales.nearestAllowed(60.9, cMajor))
        val aMinorFromC = Scales.mask(2, 9, 0)
        assertEquals(cMajor, aMinorFromC)
        assertEquals(61, Scales.nearestAllowed(61.2, Scales.CHROMATIC_MASK))
        assertEquals(60, Scales.nearestAllowed(60.4, 0)) // empty mask falls back to rounding
        assertEquals("A4", Scales.noteName(69))
        assertEquals("C♯3", Scales.noteName(49))
        assertEquals(0b000100000001, Scales.mask(Scales.CUSTOM, 5, 0b000100000001))
    }

    @Test
    fun tunerSnapsHoldsAndSmooths() {
        val t = Tuner(0.0029)
        t.retuneMs = 0.0
        assertEquals(60.0, t.update(60.3, true), 1e-9)
        // Hysteresis keeps the note until the pitch is clearly nearer the next one.
        t.holdSemitones = 0.25
        assertEquals(60.0, t.update(60.6, true), 1e-9)
        assertEquals(61.0, t.update(60.9, true), 1e-9)
        // Strength blends toward the target.
        t.reset(); t.strength = 0.5
        assertEquals(60.15, t.update(60.3, true), 1e-9)
        // Slow retune moves gradually after the first voiced frame.
        t.reset(); t.strength = 1.0; t.retuneMs = 200.0
        t.update(60.4, true)
        val y = t.update(60.4 + 0.5, true) // pitch jumps up: correction needs time to follow
        assertNotEquals(61.0, y, 1e-6)
    }

    @Test
    fun wavRoundTrip() {
        val x = FloatArray(1000) { (0.5 * Math.sin(it * 0.1)).toFloat() }
        val back = Wav.read(Wav.encode(Audio(x, 22050)))
        assertEquals(22050, back.sampleRate)
        assertEquals(x.size, back.samples.size)
        for (i in x.indices) assertEquals(x[i].toDouble(), back.samples[i].toDouble(), 1e-3)
    }
}

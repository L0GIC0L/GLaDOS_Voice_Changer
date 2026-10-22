package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Macros
import com.l0gic0l.glados.model.Presets
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
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
                assertTrue("preset ${p.name} sets the recording setting $id", !spec.input)
                assertTrue("${p.name}.$id=$v out of [${spec.min}, ${spec.max}]", v in spec.min..spec.max)
            }
        }
        assertEquals(VoiceParams.defaults(), Presets.all.first().apply())
        assertEquals("preset names must be unique", Presets.all.size, Presets.all.map { it.name.lowercase() }.toSet().size)
        for (p in Presets.all) assertEquals(p.name, Presets.matching(p.apply())?.name)
        // Recording settings belong to the clip, not the voice: presets keep them and still match.
        val noisy = VoiceParams.defaults().with(Specs.DENOISE, 90f).with(Specs.FMIN, 60f)
        val announcer = Presets.all.first { it.name == "Announcer" }
        val applied = announcer.applyTo(noisy)
        assertEquals(90f, applied[Specs.DENOISE], 0f)
        assertEquals(60f, applied[Specs.FMIN], 0f)
        assertEquals("Announcer", Presets.matching(applied)?.name)
    }

    @Test
    fun gladosPresetsKeepTheSpeakersOwnPitch() {
        for (p in Presets.all.filter { it.category == com.l0gic0l.glados.model.PresetCategory.GLADOS }) {
            val v = p.apply()
            assertTrue("${p.name} moves the voice to a target register", !v.bool(Specs.AUTO_REG))
            assertTrue("${p.name} pitches the voice up", v[Specs.SHIFT] <= 0f)
            if (v.bool(Specs.MONOTONE)) assertTrue("${p.name} holds a fixed note", v.bool(Specs.MONO_AUTO))
        }
    }

    @Test
    fun simpleModeSlidersLandWhereTheyAreSet() {
        for (start in listOf(VoiceParams.defaults(), Presets.all.first { it.name == "Natural" }.apply())) {
            for (m in Macros.all) {
                for (v in floatArrayOf(0f, 40f, 100f)) {
                    assertEquals("${m.id} -> $v", v, m.read(m.write(start, v)), 1.5f)
                }
            }
        }
        // Scaling keeps the balance between the settings a slider moves.
        val p = Macros.ROBOTIC.write(VoiceParams.defaults(), 30f) // defaults: ring 10, comb 15, crush 15
        assertEquals(20f, p[Specs.RING_MIX], 1f)
        assertEquals(30f, p[Specs.COMB_MIX], 1f)
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
    fun wavRoundTrip() {
        val x = FloatArray(1000) { (0.5 * Math.sin(it * 0.1)).toFloat() }
        val back = Wav.read(Wav.encode(Audio(x, 22050)))
        assertEquals(22050, back.sampleRate)
        assertEquals(x.size, back.samples.size)
        for (i in x.indices) assertEquals(x[i].toDouble(), back.samples[i].toDouble(), 1e-3)
    }

    @Test
    fun wavWithZeroDataLengthStillReads() {
        val x = FloatArray(500) { 0.25f }
        val bytes = Wav.encode(Audio(x, 22050))
        // Zero the RIFF and data sizes as a streaming writer would.
        for (i in 4..7) bytes[i] = 0
        for (i in 40..43) bytes[i] = 0
        assertEquals(500, Wav.read(bytes).samples.size)
    }

    @Test
    fun malformedWavHeadersAreRejectedCleanly() {
        val good = Wav.encode(Audio(FloatArray(100) { 0.1f }, 22050))
        // Zero channels, which would divide by zero.
        val noChannels = good.copyOf().also { it[22] = 0; it[23] = 0 }
        try { Wav.read(noChannels); org.junit.Assert.fail("zero channels accepted") } catch (e: IllegalArgumentException) { /* expected */ }
        // A chunk length near 2^32 must not overflow the bounds check.
        val huge = good.copyOf().also { for (i in 40..43) it[i] = 0xFF.toByte() }
        assertEquals(100, Wav.read(huge).samples.size)
        // Truncated file.
        try { Wav.read(good.copyOf(20)); org.junit.Assert.fail("truncated file accepted") } catch (e: IllegalArgumentException) { /* expected */ }
    }
}

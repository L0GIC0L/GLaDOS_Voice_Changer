package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs
import kotlin.math.round

class OfflineTest {
    private val sr = 48000
    private val params = VoiceParams.defaults().withAll(mapOf("autoReg" to 0f, "formant" to 0f, "formantFollow" to 0f, "shift" to 0f))

    /** Three "syllables": 150 Hz, 200 Hz, 120 Hz with 80 ms silences between. Returns signal and note start/end seconds. */
    private fun phrase(): Pair<FloatArray, List<Pair<Double, Double>>> {
        val hz = doubleArrayOf(150.0, 200.0, 120.0)
        val dur = 0.4
        val gap = 0.08
        val total = (0.1 + 3 * dur + 2 * gap + 0.1)
        val x = FloatArray((total * sr).toInt())
        val spans = ArrayList<Pair<Double, Double>>()
        var t = 0.1
        for (k in 0..2) {
            val seg = TestSignals.voice(sr, dur, { hz[k] }, seed = k + 1L)
            val n = seg.size
            val ramp = (0.015 * sr).toInt()
            for (i in 0 until n) {
                val env = minOf(1f, i.toFloat() / ramp, (n - i).toFloat() / ramp)
                x[(t * sr).toInt() + i] = seg[i] * env
            }
            spans += t to t + dur
            t += dur + gap
        }
        return x to spans
    }

    private fun analyse(x: FloatArray) = Analyzer.analyze(x, sr, params)

    private fun medianHz(y: FloatArray, from: Double, to: Double): Double =
        TestSignals.median(TestSignals.pitchTrack(y, sr, (from * sr).toInt(), (to * sr).toInt()))

    @Test
    fun findsOneNotePerSyllable() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        println("notes: " + edit.notes.map { "${it.f0 * an.hop / sr.toDouble()}..${it.f1 * an.hop / sr.toDouble()} c=${"%.2f".format(it.center)}" })
        assertEquals(3, edit.notes.size)
        val expected = doubleArrayOf(150.0, 200.0, 120.0).map { Pitch.hzToMidi(it) }
        for (k in 0..2) {
            assertEquals(expected[k], edit.notes[k].center, 0.15)
            assertEquals(spans[k].first, edit.notes[k].f0 * an.hop / sr.toDouble(), 0.06)
            assertEquals(spans[k].second, edit.notes[k].f1 * an.hop / sr.toDouble(), 0.06)
        }
        assertTrue("marks found", an.marks.size > 100)
    }

    @Test
    fun uneditedRenderKeepsPitchAndLevel() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val y = OfflineRenderer.render(x, an, edit, RenderSettings())
        for ((a, b) in spans) {
            val hz = medianHz(y, a + 0.08, b - 0.08)
            val hzIn = medianHz(x, a + 0.08, b - 0.08)
            assertEquals(0.0, TestSignals.cents(hz, hzIn), 8.0)
        }
        val ratio = TestSignals.rms(y) / TestSignals.rms(x)
        assertEquals(1.0, ratio, 0.1)
        for (v in y) assertFalse(v.isNaN() || abs(v) > 1.5f)
    }

    @Test
    fun pitchOffsetMovesOnlyThatNote() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        edit.notes[1].offset = 3.0
        val y = OfflineRenderer.render(x, an, edit, RenderSettings())
        val before = medianHz(x, spans[1].first + 0.08, spans[1].second - 0.08)
        assertEquals(300.0 / 100.0 * 100.0 / 100.0 * 0.0 + 3.0 * 100.0, TestSignals.cents(medianHz(y, spans[1].first + 0.08, spans[1].second - 0.08), before), 15.0)
        assertEquals(0.0, TestSignals.cents(medianHz(y, spans[0].first + 0.08, spans[0].second - 0.08), medianHz(x, spans[0].first + 0.08, spans[0].second - 0.08)), 8.0)
    }

    @Test
    fun correctionSnapsNotesToTheScaleAndFlattens() {
        val (x, spans) = phrase()
        // Add vibrato to the middle syllable so there is something to flatten.
        val v = TestSignals.voice(sr, 0.4, { 200.0 * Math.pow(2.0, 0.5 * Math.sin(2 * Math.PI * 5.5 * it) / 12.0) }, seed = 9)
        val start = (spans[1].first * sr).toInt()
        for (i in v.indices) x[start + i] = v[i]
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        assertEquals(3, edit.notes.size)
        NoteOps.correct(edit.notes, Scales.CHROMATIC_MASK, 1.0, 1.0, 1.0, 0.0)
        val y = OfflineRenderer.render(x, an, edit, RenderSettings())
        for ((k, hz) in doubleArrayOf(150.0, 200.0, 120.0).withIndex()) {
            val target = Pitch.midiToHz(round(Pitch.hzToMidi(hz)))
            val track = TestSignals.pitchTrack(y, sr, ((spans[k].first + 0.08) * sr).toInt(), ((spans[k].second - 0.06) * sr).toInt())
            val worst = track.filter { it > 0 }.maxOf { abs(TestSignals.cents(it, target)) }
            println("note $k target=${"%.2f".format(target)} worst deviation=${"%.1f".format(worst)} cents")
            assertTrue("note $k worst $worst", worst < 20.0)
        }
    }

    @Test
    fun scaleCorrectionMovesToAllowedNotes() {
        val (x, _) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val mask = Scales.mask(1, 0, 0) // C major
        NoteOps.correct(edit.notes, mask, 1.0, 1.0, 1.0, 0.0)
        for (n in edit.notes) {
            val finalNote = n.center + n.offset
            assertEquals(0.0, finalNote - round(finalNote), 1e-6)
            assertTrue(Scales.isAllowed(mask, round(finalNote).toInt()))
        }
    }

    @Test
    fun formantEditShiftsSpectrumNotPitch() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val y0 = OfflineRenderer.render(x, an, edit, RenderSettings())
        edit.notes[0].formant = 5f
        val y1 = OfflineRenderer.render(x, an, edit, RenderSettings())
        val a = (spans[0].first * sr).toInt() + 4800
        val c0 = TestSignals.spectralCentroid(y0, sr, a, a + 4800)
        val c1 = TestSignals.spectralCentroid(y1, sr, a, a + 4800)
        println("centroid $c0 -> $c1")
        assertTrue("centroid $c0 -> $c1", c1 > c0 * 1.1)
        assertEquals(0.0, TestSignals.cents(medianHz(y1, spans[0].first + 0.08, spans[0].second - 0.08), medianHz(y0, spans[0].first + 0.08, spans[0].second - 0.08)), 10.0)
    }

    @Test
    fun gainEditChangesLevel() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        edit.notes[2].gainDb = -12f
        val y = OfflineRenderer.render(x, an, edit, RenderSettings())
        val a = ((spans[2].first + 0.1) * sr).toInt()
        val b = ((spans[2].second - 0.1) * sr).toInt()
        val db = 20 * Math.log10(TestSignals.rms(y, a, b) / TestSignals.rms(x, a, b))
        assertEquals(-12.0, db, 1.5)
    }

    @Test
    fun timingEditStretchesANote() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val n = edit.notes[0]
        val key = n.inEnd(an)
        val lengthBefore = (n.inEnd(an) - n.inStart(an)).toDouble()
        val applied = edit.moveAnchors(listOf(key), 0.15 * lengthBefore, 0.005 * sr, an.length)
        println("applied $applied of ${0.15 * lengthBefore}")
        assertEquals(0.15 * lengthBefore, applied, 1.0)
        val y = OfflineRenderer.render(x, an, edit, RenderSettings())
        assertEquals(x.size, y.size)
        // Pitch of the stretched note unchanged, and sound now continues past the original end.
        val stretchedEnd = spans[0].second + 0.15 * lengthBefore / sr
        assertEquals(0.0, TestSignals.cents(medianHz(y, spans[0].first + 0.08, spans[0].second - 0.08), 150.0), 10.0)
        val tailLevel = TestSignals.rms(y, ((spans[0].second + 0.01) * sr).toInt(), ((stretchedEnd - 0.02) * sr).toInt())
        assertTrue("stretched region silent: $tailLevel", tailLevel > 0.02)
        // The next note keeps its pitch.
        assertEquals(0.0, TestSignals.cents(medianHz(y, spans[1].first + 0.1, spans[1].second - 0.08), 200.0), 12.0)
    }

    @Test
    fun anchorMovesAreConstrained() {
        val (x, _) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val key = edit.notes[0].inEnd(an)
        val huge = edit.moveAnchors(listOf(key), 1e9, 0.005 * sr, an.length)
        val nextStart = edit.notes[1].inStart(an)
        assertTrue("moved past the next anchor", edit.anchors[key]!! < edit.anchors[nextStart]!!)
        assertTrue(huge > 0)
        val negative = edit.moveAnchors(listOf(key), -1e9, 0.005 * sr, an.length)
        assertTrue(negative < 0)
        assertTrue(edit.anchors[key]!! > edit.anchors[edit.notes[0].inStart(an)]!!)
    }

    @Test
    fun splitAndMergeRoundTrip() {
        val (x, _) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val n0 = edit.notes[0]
        val mid = (n0.f0 + n0.f1) / 2
        assertTrue(NoteOps.split(edit, an, 0, mid))
        assertEquals(4, edit.notes.size)
        assertEquals(mid, edit.notes[1].f0)
        assertEquals(mid - 1, edit.notes[0].f1)
        val anchorsBefore = edit.anchors.size
        assertTrue(NoteOps.merge(edit, an, 0))
        assertEquals(3, edit.notes.size)
        assertEquals(anchorsBefore - 1, edit.anchors.size)
        // Unedited split + merge must not change the audio.
        val y = OfflineRenderer.render(x, an, edit, RenderSettings())
        val y0 = OfflineRenderer.render(x, an, NoteOps.segment(an), RenderSettings())
        var d = 0.0
        for (i in y.indices) d += abs(y[i] - y0[i]).toDouble()
        assertTrue("render changed by split+merge: $d", d / y.size < 0.01)
    }

    @Test
    fun pitchShiftMovesWholeClip() {
        val (x, spans) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val y = OfflineRenderer.render(x, an, edit, RenderSettings(pitchShift = 7.0))
        for (k in 0..2) {
            val before = medianHz(x, spans[k].first + 0.08, spans[k].second - 0.08)
            assertEquals(700.0, TestSignals.cents(medianHz(y, spans[k].first + 0.08, spans[k].second - 0.08), before), 15.0)
        }
    }

    @Test
    fun globalShiftHelperMatchesRegister() {
        val (x, _) = phrase()
        val an = analyse(x)
        val p = params.with(Specs.AUTO_REG, 1f).with(Specs.REG_HZ, 220f)
        val shift = NoteOps.globalShift(an, p)
        val med = an.medianMidi()
        assertEquals(Pitch.hzToMidi(220.0), med + shift, 0.01)
    }

    @Test
    fun oddHopKeepsTouchingNotesOnOneSharedAnchor() {
        // 44.1 kHz gives a 221-sample hop. Touching notes must share a single boundary anchor, or the Timing tool
        // cannot move them and splitting leaves a gap or an overlap.
        val sr44 = 44100
        // Legato steps, no gaps, so the notes touch each other.
        val x = TestSignals.voice(sr44, 2.1, { if (it < 0.7) 120.0 else if (it < 1.4) 150.0 else 190.0 }, seed = 4)
        val an = Analyzer.analyze(x, sr44, params)
        assertTrue("expected an odd hop, got ${an.hop}", an.hop % 2 == 1)
        val edit = NoteOps.segment(an)
        assertTrue("need several notes, got ${edit.notes.size}", edit.notes.size >= 3)
        assertTrue("need touching notes", edit.notes.zipWithNext().any { (a, b) -> b.f0 == a.f1 + 1 })
        for ((a, b) in edit.notes.zipWithNext()) {
            if (b.f0 == a.f1 + 1) assertEquals("touching notes must meet exactly", a.inEnd(an), b.inStart(an))
        }
        val n = edit.notes.first { it.frames >= 12 }
        val i = edit.notes.indexOf(n)
        val mid = (n.f0 + n.f1) / 2
        assertTrue(NoteOps.split(edit, an, i, mid))
        val left = edit.notes[i]
        val right = edit.notes[i + 1]
        assertEquals(left.inEnd(an), right.inStart(an))
        assertTrue("the split boundary must be a real anchor", edit.anchors.containsKey(right.inStart(an)))
        // The shared anchor moves both neighbours.
        val moved = edit.moveAnchors(listOf(right.inStart(an)), 20.0, 0.005 * sr44, an.length)
        assertTrue("the boundary would not move ($moved)", moved != 0.0)
    }

    @Test
    fun splitAndMergeKeepTheEditedPitch() {
        val (x, _) = phrase()
        val an = analyse(x)
        val edit = NoteOps.segment(an)
        val i = 0
        val n = edit.notes[i]
        NoteOps.correct(listOf(n), Scales.CHROMATIC_MASK, 1.0, 1.0, 1.0, 0.0)
        val target = n.center + n.offset
        assertTrue(NoteOps.split(edit, an, i, (n.f0 + n.f1) / 2))
        for (half in listOf(edit.notes[i], edit.notes[i + 1])) {
            assertEquals("a split half drifted off the corrected pitch", target, half.center + half.offset, 1e-9)
        }
        assertTrue(NoteOps.merge(edit, an, i))
        assertEquals("a merged note drifted off the corrected pitch", target, edit.notes[i].center + edit.notes[i].offset, 1e-9)
    }
}

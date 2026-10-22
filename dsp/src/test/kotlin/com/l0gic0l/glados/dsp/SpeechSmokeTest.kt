package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.round

/**
 * Runs real speech through the editor pipeline when GLADOS_TEST_DIR points at a folder of WAV files.
 * Prints quality metrics and writes processed audio to <dir>/out for inspection. Skipped otherwise.
 */
class SpeechSmokeTest {
    private val dir: File? = System.getenv("GLADOS_TEST_DIR")?.takeIf { it.isNotEmpty() }?.let(::File)?.takeIf { it.isDirectory }

    private fun gridStats(y: FloatArray, sr: Int, p: VoiceParams): String {
        val track = TestSignals.pitchTrack(y, sr, hop = sr / 100)
        var voiced = 0
        var onGrid = 0
        for (f in track) {
            if (f <= 0) continue
            voiced++
            val m = Pitch.hzToMidi(f)
            if (abs(m - round(m)) * 100 < 25) onGrid++
        }
        return "voiced=$voiced/${track.size} onGrid=$onGrid medianHz=${"%.1f".format(TestSignals.median(track))}"
    }

    @Test
    fun offlineEditorOnSpeech() {
        assumeTrue(dir != null)
        val out = File(dir, "out").also { it.mkdirs() }
        val files = dir!!.listFiles { f -> f.name.endsWith(".wav") }!!.sortedBy { it.name }
        val p = VoiceParams.defaults()
        for (f in files) {
            val a = Wav.read(f.readBytes())
            val t0 = System.nanoTime()
            val an = Analyzer.analyze(a.samples, a.sampleRate, p)
            val t1 = System.nanoTime()
            val edit = NoteOps.segment(an)
            val shift = NoteOps.globalShift(an, p)
            NoteOps.correctToSettings(edit.notes, p, shift)
            val t2 = System.nanoTime()
            val y = OfflineRenderer.render(a.samples, an, edit, RenderSettings(shift, NoteOps.formantShift(p, shift), p[Specs.BREATH] / 100f))
            val t3 = System.nanoTime()
            val fx = FxChain(a.sampleRate)
            fx.configure(p)
            val full = y.copyOf(y.size + (a.sampleRate * FxChain.tailSeconds(p)).toInt())
            fx.process(full, full.size)
            val t4 = System.nanoTime()
            File(out, f.name.removeSuffix(".wav") + ".offline-portal1.wav").writeBytes(Wav.encode(Audio(full, a.sampleRate)))
            File(out, f.name.removeSuffix(".wav") + ".offline-dry.wav").writeBytes(Wav.encode(Audio(y, a.sampleRate)))
            val voiced = an.voiced.count { it }
            println(
                "OFFLINE ${f.name} ${"%.1f".format(a.seconds)}s notes=${edit.notes.size} voicedFrames=$voiced/${an.frames} marks=${an.marks.size} shift=${"%.1f".format(shift)}st " +
                    "analyse=${(t1 - t0) / 1_000_000}ms segment=${(t2 - t1) / 1_000_000}ms render=${(t3 - t2) / 1_000_000}ms fx=${(t4 - t3) / 1_000_000}ms " +
                    "out: ${gridStats(y, a.sampleRate, p)}",
            )
        }
    }
}

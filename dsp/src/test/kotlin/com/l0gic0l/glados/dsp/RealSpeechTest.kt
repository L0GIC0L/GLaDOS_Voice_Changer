package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File

/** Runs the analyser over real recordings in $GLADOS_REAL_DIR and dumps tracks for plotting. Skipped when unset. */
class RealSpeechTest {
    @Test
    fun analyseRealRecordings() {
        val dir = System.getenv("GLADOS_REAL_DIR")?.takeIf { it.isNotEmpty() }?.let(::File)
        assumeTrue(dir != null && dir.isDirectory)
        File(dir, "out").mkdirs()
        val syl = File(dir, "syllables.json").takeIf { it.exists() }?.readText()?.let { txt ->
            Regex("\"([^\"]+)\":\\s*(\\d+)").findAll(txt).associate { it.groupValues[1] to it.groupValues[2].toInt() }
        } ?: emptyMap()
        val thresholds = doubleArrayOf(0.05, 0.1625, 0.27, 0.35, 0.4)
        for (f in dir!!.listFiles { x -> x.name.endsWith(".wav") }!!.sortedBy { it.name }) {
            val a = Wav.read(f.readBytes())
            val line = StringBuilder("REAL %-22s %2.0fs sr=%d expected~%s |".format(f.name, a.seconds, a.sampleRate, syl[f.name]?.toString() ?: "?"))
            for (t in thresholds) {
                val an = Analyzer.analyze(a.samples, a.sampleRate, VoiceParams.defaults(), thresholdOverride = t)
                val edit = NoteOps.segment(an)
                line.append(" thr%.3f: %2.0f%% voiced, %2d notes |".format(t, 100.0 * an.voiced.count { it } / an.frames, edit.notes.size))
                if (t == 0.27) {
                    File(dir, "out/${f.nameWithoutExtension}.f0.csv").writeText(
                        (0 until an.frames).joinToString("\n") { "${it * an.hop.toDouble() / an.sampleRate},${if (an.voiced[it]) an.f0[it] else 0f}" },
                    )
                    File(dir, "out/${f.nameWithoutExtension}.notes.csv").writeText(
                        edit.notes.joinToString("\n") { "${it.f0 * an.hop.toDouble() / an.sampleRate},${(it.f1 + 1) * an.hop.toDouble() / an.sampleRate},${it.center}" },
                    )
                }
            }
            println(line)
        }
    }
}

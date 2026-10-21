package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import java.util.Random
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Compares voicing detection on clean real speech against the same speech degraded like a phone
 * recording. Needs $GLADOS_REAL_DIR (see RealSpeechTest). Prints a table; asserts nothing.
 */
class DegradeTest {
    private fun rms(x: FloatArray): Double = sqrt(x.fold(0.0) { s, v -> s + v * v } / x.size)

    private fun addNoise(x: FloatArray, snrDb: Double, seed: Long): FloatArray {
        val r = Random(seed)
        val sigma = rms(x) / 10.0.pow(snrDb / 20)
        // Mildly coloured (pink-ish) noise, like room and mic hiss.
        var b = 0.0
        return FloatArray(x.size) { i ->
            b = 0.97 * b + r.nextGaussian() * 0.25
            (x[i] + sigma * (0.6 * r.nextGaussian() + 3.0 * b * 0.3)).toFloat()
        }
    }

    private fun reverb(x: FloatArray, sr: Int, rt60: Double, wet: Double, seed: Long): FloatArray {
        val r = Random(seed)
        val n = (rt60 * sr).toInt()
        val ir = FloatArray(n) { (r.nextGaussian() * exp(-6.9 * it / n)).toFloat() }
        ir[0] = 0f
        val out = FloatArray(x.size)
        for (i in x.indices) {
            var s = 0.0
            val lim = minOf(n - 1, i)
            for (k in 1..lim) s += x[i - k] * ir[k]
            out[i] = (x[i] + wet * 0.05 * s).toFloat()
        }
        return out
    }

    private fun rumble(x: FloatArray, sr: Int, amp: Float): FloatArray =
        FloatArray(x.size) { (x[it] + amp * (sin(2 * PI * 38.0 * it / sr) + 0.6 * sin(2 * PI * 17.0 * it / sr))).toFloat() }

    private fun gain(x: FloatArray, db: Double): FloatArray = FloatArray(x.size) { (x[it] * 10.0.pow(db / 20)).toFloat() }

    private fun clip(x: FloatArray): FloatArray {
        val g = 6f
        return FloatArray(x.size) { max(-1f, min1(x[it] * g)) }
    }

    private fun min1(v: Float) = if (v > 1f) 1f else v

    @Test
    fun degradedSpeech() {
        val dir = System.getenv("GLADOS_REAL_DIR")?.takeIf { it.isNotEmpty() }?.let(::File)
        assumeTrue(dir != null && dir.isDirectory)
        val p = VoiceParams.defaults()
        val thr = (System.getenv("GLADOS_THR") ?: "0.27").toDouble()
        for (name in listOf("libri1.wav", "libri3.wav", "signalogic_male.wav")) {
            val f = File(dir, name)
            if (!f.exists()) continue
            val a = Wav.read(f.readBytes())
            val clean = a.samples
            val ref = Analyzer.analyze(clean, a.sampleRate, p, thresholdOverride = thr)
            val conds = linkedMapOf(
                "clean" to clean,
                "snr20" to addNoise(clean, 20.0, 1),
                "snr10" to addNoise(clean, 10.0, 2),
                "reverb0.4" to reverb(clean, a.sampleRate, 0.4, 1.0, 3),
                "reverb0.8" to reverb(clean, a.sampleRate, 0.8, 1.5, 4),
                "rumble" to rumble(clean, a.sampleRate, (rms(clean) * 0.6).toFloat()),
                "quiet-40dB" to gain(clean, -40.0),
                "clipped" to clip(clean),
                "phone: snr15+rev0.3+rumble" to rumble(addNoise(reverb(clean, a.sampleRate, 0.3, 1.0, 5), 15.0, 6), a.sampleRate, (rms(clean) * 0.4).toFloat()),
            )
            println("DEG $name thr=$thr  (reference = clean analysis: ${ref.voiced.count { it }} voiced frames)")
            for ((label, sig) in conds) {
                val an = Analyzer.analyze(sig, a.sampleRate, p, thresholdOverride = thr)
                var tp = 0; var fp = 0; var fn = 0; var good = 0
                for (i in 0 until minOf(an.frames, ref.frames)) {
                    if (ref.voiced[i] && an.voiced[i]) {
                        tp++
                        if (abs(12 * Math.log(an.f0[i].toDouble() / ref.f0[i]) / Math.log(2.0)) < 0.5) good++
                    } else if (an.voiced[i]) fp++ else if (ref.voiced[i]) fn++
                }
                val notes = NoteOps.segment(an).notes.size
                println(
                    "DEG   %-30s recall=%3.0f%% precision=%3.0f%% f0ok=%3.0f%% notes=%d".format(
                        label, 100.0 * tp / max(1, tp + fn), 100.0 * tp / max(1, tp + fp), 100.0 * good / max(1, tp), notes,
                    ),
                )
            }
        }
    }

    @Test
    fun liveEngineOnDegradedSpeech() {
        val dir = System.getenv("GLADOS_REAL_DIR")?.takeIf { it.isNotEmpty() }?.let(::File)
        assumeTrue(dir != null && dir.isDirectory)
        val p = VoiceParams.defaults()
        for (name in listOf("libri1.wav", "libri3.wav", "signalogic_male.wav")) {
            val f = File(dir, name)
            if (!f.exists()) continue
            val a = Wav.read(f.readBytes())
            val clean = a.samples
            val ref = Analyzer.analyze(clean, a.sampleRate, p)
            val conds = linkedMapOf(
                "clean" to clean,
                "snr10" to addNoise(clean, 10.0, 2),
                "reverb0.4" to reverb(clean, a.sampleRate, 0.4, 1.0, 3),
                "rumble" to rumble(clean, a.sampleRate, (rms(clean) * 0.6).toFloat()),
                "phone: snr15+rev0.3+rumble" to rumble(addNoise(reverb(clean, a.sampleRate, 0.3, 1.0, 5), 15.0, 6), a.sampleRate, (rms(clean) * 0.4).toFloat()),
            )
            println("LIVE $name (reference voiced frames ${ref.voiced.count { it }}/${ref.frames})")
            for ((label, sig) in conds) {
                val e = LivePitchEngine(a.sampleRate, p)
                val out = FloatArray(sig.size)
                val bi = FloatArray(128)
                val bo = FloatArray(128)
                var tp = 0; var fp = 0; var fn = 0
                var k = 0
                var i = 0
                val half = Yin(a.sampleRate, p[com.l0gic0l.glados.model.Specs.FMIN].toDouble(), p[com.l0gic0l.glados.model.Specs.FMAX].toDouble()).windowLength / 2
                while (i + 128 <= sig.size) {
                    System.arraycopy(sig, i, bi, 0, 128)
                    e.process(bi, bo, 128)
                    System.arraycopy(bo, 0, out, i, 128)
                    val centre = i + 128 - half
                    val fr = (centre.toDouble() / ref.hop).toInt()
                    if (fr in 0 until ref.frames) {
                        val live = e.inputHz > 0
                        if (ref.voiced[fr] && live) tp++ else if (live) fp++ else if (ref.voiced[fr]) fn++
                    }
                    i += 128
                    k++
                }
                var voicedOut = 0
                var onGrid = 0
                val track = TestSignals.pitchTrack(out, a.sampleRate, hop = a.sampleRate / 100, fmin = 60.0, fmax = 900.0)
                for (hz in track) {
                    if (hz <= 0) continue
                    voicedOut++
                    val m = Pitch.hzToMidi(hz)
                    if (abs(m - Math.round(m)) * 100 < 30) onGrid++
                }
                println(
                    "LIVE   %-30s recall=%3.0f%% precision=%3.0f%% starved=%d outputOnGrid=%3.0f%% of %d pitched output frames".format(
                        label, 100.0 * tp / max(1, tp + fn), 100.0 * tp / max(1, tp + fp), e.starvedGrains, 100.0 * onGrid / max(1, voicedOut), voicedOut,
                    ),
                )
            }
        }
    }
}

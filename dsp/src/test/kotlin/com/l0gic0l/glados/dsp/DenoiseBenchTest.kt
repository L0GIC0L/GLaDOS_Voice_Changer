package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import java.util.Random
import kotlin.math.PI
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Benchmarks noise reduction on real speech from $GLADOS_REAL_DIR with noise added at known levels. Prints, per
 * condition: SNR gain, segmental SNR gain, how far the pauses drop, a musical-noise score (how much more spiky the
 * leftover noise is than the original, in dB of kurtosis; 0 is ideal), how much a clean recording is changed, and
 * pitch-detection recall and precision on the noisy versus the cleaned audio. Asserts nothing. Skipped when unset.
 */
class DenoiseBenchTest {
    private fun rms(x: FloatArray) = sqrt(x.fold(0.0) { s, v -> s + v * v } / x.size)

    private fun snr(clean: FloatArray, test: FloatArray): Double {
        var s = 0.0
        var e = 0.0
        for (i in clean.indices) {
            s += clean[i] * clean[i].toDouble()
            val d = test[i] - clean[i].toDouble()
            e += d * d
        }
        return 10 * log10(s / max(e, 1e-30))
    }

    private fun segSnr(clean: FloatArray, test: FloatArray, sr: Int, active: BooleanArray): Double {
        val w = sr / 50
        var sum = 0.0
        var n = 0
        var f = 0
        while ((f + 1) * w <= clean.size) {
            if (active[f]) {
                var s = 0.0
                var e = 0.0
                for (i in f * w until (f + 1) * w) {
                    s += clean[i] * clean[i].toDouble()
                    val d = test[i] - clean[i].toDouble()
                    e += d * d
                }
                sum += (10 * log10(s / max(e, 1e-20))).coerceIn(-10.0, 35.0)
                n++
            }
            f++
        }
        return sum / max(1, n)
    }

    /** 20 ms frames where the clean speech is within 35 dB of its loudest frames. */
    private fun activity(clean: FloatArray, sr: Int): BooleanArray {
        val w = sr / 50
        val frames = clean.size / w
        val e = DoubleArray(frames) { f -> var s = 0.0; for (i in f * w until (f + 1) * w) s += clean[i] * clean[i].toDouble(); s / w }
        val top = e.sorted()[(frames * 0.95).toInt().coerceAtMost(frames - 1)]
        return BooleanArray(frames) { e[it] > top * 10.0.pow(-3.5) }
    }

    private fun Double.pow(p: Double) = Math.pow(this, p)

    private fun pauseKurtosisAndRms(x: FloatArray, sr: Int, active: BooleanArray): Pair<Double, Double> {
        // Kurtosis of short-time power across time and frequency in the pauses: musical noise makes it spiky.
        val n = 512
        val fft = Fft(n)
        val w = sr / 50
        val vals = ArrayList<Double>()
        var energy = 0.0
        var count = 0
        var s = 0
        while (s + n <= x.size) {
            val f0 = s / w
            val f1 = (s + n - 1) / w
            if ((f0..f1).all { it < active.size && !active[it] }) {
                val re = DoubleArray(n) { x[s + it] * (0.5 - 0.5 * kotlin.math.cos(2 * PI * it / n)) }
                val im = DoubleArray(n)
                fft.transform(re, im, false)
                for (k in 4 until n / 2) vals += re[k] * re[k] + im[k] * im[k]
                for (i in s until s + n) energy += x[i] * x[i].toDouble()
                count += n
            }
            s += n / 2
        }
        if (vals.size < 10) return Double.NaN to Double.NaN
        val m = vals.average()
        var m2 = 0.0
        var m4 = 0.0
        for (v in vals) { val d = v - m; m2 += d * d; m4 += d * d * d * d }
        m2 /= vals.size; m4 /= vals.size
        return (m4 / (m2 * m2)) to sqrt(energy / max(1, count))
    }

    private fun noise(kind: String, n: Int, sr: Int, seed: Long): FloatArray {
        val r = Random(seed)
        var b0 = 0.0; var b1 = 0.0; var b2 = 0.0
        return FloatArray(n) { i ->
            val wv = r.nextGaussian()
            when (kind) {
                "white" -> wv
                "pink" -> {
                    b0 = 0.99765 * b0 + wv * 0.0990460
                    b1 = 0.96300 * b1 + wv * 0.2965164
                    b2 = 0.57000 * b2 + wv * 1.0526913
                    (b0 + b1 + b2 + wv * 0.1848) * 0.3
                }
                else -> { // mains hum with harmonics plus hiss
                    var h = 0.0
                    for (k in 1..6) h += sin(2 * PI * 60.0 * k * i / sr) / k
                    0.8 * h + 0.5 * wv
                }
            }.toFloat()
        }.also { y -> val g = 1.0 / rms(y); for (i in y.indices) y[i] = (y[i] * g).toFloat() }
    }

    /** [clean] with its quiet 20 ms frames cut out, as if the talker never paused. */
    private fun withoutPauses(clean: FloatArray, sr: Int, active: BooleanArray): FloatArray {
        val w = sr / 50
        val out = FloatArray(active.count { it } * w)
        var o = 0
        for (f in active.indices) if (active[f]) { System.arraycopy(clean, f * w, out, o, w); o += w }
        return out
    }

    /** [x] swelling and fading by up to 6 dB, one slow cycle over the clip, at the same overall level. */
    private fun drift(x: FloatArray): FloatArray {
        val y = FloatArray(x.size) { (x[it] * 10.0.pow(0.3 * sin(2 * PI * it / x.size))).toFloat() }
        val g = 1.0 / rms(y)
        for (i in y.indices) y[i] = (y[i] * g).toFloat()
        return y
    }

    private fun detection(ref: Analysis, an: Analysis): Pair<Double, Double> {
        var tp = 0; var fp = 0; var fn = 0
        for (i in 0 until minOf(ref.frames, an.frames)) {
            if (ref.voiced[i] && an.voiced[i]) tp++ else if (an.voiced[i]) fp++ else if (ref.voiced[i]) fn++
        }
        return 100.0 * tp / max(1, tp + fn) to 100.0 * tp / max(1, tp + fp)
    }

    @Test
    fun benchmark() {
        val dir = System.getenv("GLADOS_REAL_DIR")?.takeIf { it.isNotEmpty() }?.let(::File)
        assumeTrue(dir != null && dir.isDirectory)
        val amounts = (System.getenv("GLADOS_NR_AMOUNTS") ?: "0.25,0.5,0.75,1.0").split(",").map { it.trim().toFloat() }
        val snrs = (System.getenv("GLADOS_NR_SNRS") ?: "0,5,10,20").split(",").map { it.trim().toDouble() }
        val kinds = (System.getenv("GLADOS_NR_KINDS") ?: "white,pink,hum").split(",").map { it.trim() }
        val detect = System.getenv("GLADOS_NR_DETECT") != "0"
        val p = VoiceParams.defaults()
        val totals = HashMap<String, DoubleArray>()
        val files = dir!!.listFiles { f -> f.name.endsWith(".wav") }!!.sortedBy { it.name }
        for (f in files) {
            val a = Wav.read(f.readBytes())
            val clean = a.samples
            val sr = a.sampleRate
            val act = activity(clean, sr)
            val ref = if (detect) Analyzer.analyze(clean, sr, p) else null
            for (amount in amounts) {
                val t0 = System.nanoTime()
                val dc = Denoiser.process(clean, sr, amount)
                val ms = (System.nanoTime() - t0) / 1e6
                println("NR %-14s clean-in amount=%.2f changedBy=%5.1f dB SNR  speed=%.0f ms per audio second".format(f.name, amount, snr(clean, dc), ms / a.seconds))
            }
            // Kinds ending in -nopause use the speech run together so no frame is free of it; -drift makes the noise
            // level wander slowly by up to 6 dB either way.
            val unpaused = withoutPauses(clean, sr, act)
            val unpausedAct = activity(unpaused, sr)
            val unpausedRef by lazy { if (detect) Analyzer.analyze(unpaused, sr, p) else null }
            for (kind in kinds) for (snrIn in snrs) {
                val nopause = kind.endsWith("-nopause")
                val speech = if (nopause) unpaused else clean
                val sAct = if (nopause) unpausedAct else act
                val sRef = if (nopause) unpausedRef else ref
                val nz = noise(kind.substringBefore('-'), speech.size, sr, 7).let { if (kind.endsWith("-drift")) drift(it) else it }
                val g = rms(speech) / 10.0.pow(snrIn / 20)
                val noisy = FloatArray(speech.size) { (speech[it] + nz[it] * g).toFloat() }
                val (kIn, rIn) = pauseKurtosisAndRms(noisy, sr, sAct)
                val detIn = sRef?.let { detection(it, Analyzer.analyze(noisy, sr, p)) }
                for (amount in amounts) {
                    val y = Denoiser.process(noisy, sr, amount)
                    val dSnr = snr(speech, y) - snr(speech, noisy)
                    val dSeg = segSnr(speech, y, sr, sAct) - segSnr(speech, noisy, sr, sAct)
                    val (kOut, rOut) = pauseKurtosisAndRms(y, sr, sAct)
                    val drop = 20 * log10(rIn / rOut)
                    val musical = 10 * log10(kOut / kIn)
                    val detOut = sRef?.let { detection(it, Analyzer.analyze(y, sr, p)) }
                    println(
                        "NR %-14s %-5s snr=%4.0f amount=%.2f  dSNR=%+5.1f dSegSNR=%+5.1f pauseDrop=%5.1f dB musical=%+5.2f dB %s".format(
                            f.name, kind, snrIn, amount, dSnr, dSeg, drop, musical,
                            if (detIn != null && detOut != null) "recall %3.0f->%3.0f precision %3.0f->%3.0f".format(detIn.first, detOut.first, detIn.second, detOut.second) else "",
                        ),
                    )
                    val key = "$kind snr=${snrIn.toInt()} amount=$amount"
                    val tot = totals.getOrPut(key) { DoubleArray(8) }
                    tot[0] += dSnr; tot[1] += dSeg; tot[4] += 1.0
                    // Some clips have too few pauses to measure; leave them out of the pause averages.
                    if (drop.isFinite() && musical.isFinite()) { tot[2] += drop; tot[3] += musical; tot[7] += 1.0 }
                    if (detIn != null && detOut != null) { tot[5] += detOut.first - detIn.first; tot[6] += detOut.second - detIn.second }
                }
            }
        }
        println("NR SUMMARY (mean over files)")
        for ((k, t) in totals.toSortedMap()) {
            val n = t[4]
            val m = max(1.0, t[7])
            println("NR SUM %-28s dSNR=%+5.1f dSegSNR=%+5.1f pauseDrop=%5.1f musical=%+5.2f dRecall=%+5.1f dPrecision=%+5.1f".format(k, t[0] / n, t[1] / n, t[2] / m, t[3] / m, t[5] / n, t[6] / n))
        }
    }
}

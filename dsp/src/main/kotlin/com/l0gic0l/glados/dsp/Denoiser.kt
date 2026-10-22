package com.l0gic0l.glados.dsp

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Offline noise reduction for steady background noise: hiss, fans, air conditioning, hum.
 *
 * The clip is cut into overlapping short-time spectra. The noise spectrum is learned from the clip itself, as the
 * average of its quietest frames (the pauses between words), and its level is then followed through the clip so noise
 * that swells and fades (a fan, a phone's automatic gain) is still removed evenly. Each frequency is then scaled by a Wiener gain whose
 * signal-to-noise estimate uses the decision-directed rule of Ephraim and Malah, which removes noise with far less
 * of the warbling "musical noise" left by plain spectral subtraction, refined by the second step of Plapous, Marro
 * and Scalart's two-step noise reduction so the estimate does not lag a frame behind word onsets and endings. A floor
 * on the gain bounds how much is taken away, so quiet speech is turned down a little rather than gated out.
 */
object Denoiser {
    /** Smoothing of the decision-directed estimate; 0.98 is the classic value. */
    private const val ALPHA = 0.98

    /** Lowest a-priori SNR, about -25 dB. Keeps residual noise smooth instead of twinkling. */
    private const val XI_MIN = 0.003

    /** Share of frames, quietest first, averaged into the noise estimate. */
    private const val QUIET_SHARE = 0.1

    /** Per-frame smoothing of the noise level tracker. */
    private const val TRACK_SMOOTHING = 0.95

    /** The tracker's fixed a-priori SNR for "speech is present", 15 dB (Gerkmann and Hendriks). */
    private const val XI_SPEECH = 31.6

    /** The tracked level sits a little low on average; this brings it back to the true noise level. */
    private const val LEVEL_BIAS = 1.2

    /** The tracker only needs the overall level, so it follows every fourth frequency bin. */
    private const val TRACK_STEP = 4

    /**
     * Returns a cleaned copy of [x]. [amount] 0..1 sets how far noise may be pushed down (up to about 30 dB) and how
     * boldly it is estimated. 0 returns [x] itself. If [cancelled] turns true the result is incomplete and should be
     * thrown away.
     */
    fun process(x: FloatArray, sampleRate: Int, amount: Float, cancelled: () -> Boolean = { false }): FloatArray {
        val t = amount.coerceIn(0f, 1f)
        if (t <= 0f || x.isEmpty()) return x
        val n = Integer.highestOneBit((0.04 * sampleRate).toInt()).coerceIn(256, 4096)
        val hop = n / 4
        val bins = n / 2 + 1
        val fft = Fft(n)
        val win = FloatArray(n) { sqrt(0.5 - 0.5 * cos(2 * PI * it / n)).toFloat() }

        // Pad by a whole frame at each end so the first and last samples are covered by a full set of frames.
        val padded = FloatArray(x.size + 2 * n)
        System.arraycopy(x, 0, padded, n, x.size)
        val frames = (padded.size - n) / hop + 1

        val profile = noiseSpectrum(padded, frames, hop, win, fft) ?: return x
        val levels = noiseLevels(padded, frames, hop, win, fft, profile, cancelled)
        val overSubtract = 1.0 + 0.75 * t
        val floor = 10.0.pow(-30.0 * t / 20.0)
        val noise = DoubleArray(bins)

        val re = DoubleArray(n)
        val im = DoubleArray(n)
        val prevClean = DoubleArray(bins) // |S|^2 estimate from the previous frame, before the floor
        val out = FloatArray(padded.size)
        var first = true
        for (f in 0 until frames) {
            if (f % 64 == 0 && cancelled()) break
            val start = f * hop
            for (k in 0 until bins) noise[k] = max(profile[k] * levels[f] * overSubtract, 1e-20)
            for (i in 0 until n) {
                re[i] = (padded[start + i] * win[i]).toDouble()
                im[i] = 0.0
            }
            fft.transform(re, im, inverse = false)
            for (k in 0 until bins) {
                val power = re[k] * re[k] + im[k] * im[k]
                val gamma = power / noise[k]
                val ml = max(gamma - 1.0, 0.0)
                val xi = if (first) max(ml, XI_MIN) else max(ALPHA * prevClean[k] / noise[k] + (1 - ALPHA) * ml, XI_MIN)
                val g1 = xi / (1.0 + xi)
                // The floor is left out of the recursion: fed back, it makes the estimate follow the leftover noise,
                // which twinkles at gentle settings.
                prevClean[k] = g1 * g1 * power
                // Second step: re-estimate the SNR from this frame's own first estimate, removing the one-frame lag.
                val xi2 = max(g1 * g1 * gamma, XI_MIN)
                val g = max(xi2 / (1.0 + xi2), floor)
                re[k] *= g
                im[k] *= g
                if (k in 1 until n / 2) {
                    re[n - k] *= g
                    im[n - k] *= g
                }
            }
            first = false
            fft.transform(re, im, inverse = true)
            // sqrt-Hann in and out makes Hann overall, which sums to 2 at a quarter-frame hop.
            for (i in 0 until n) out[start + i] += (re[i] * win[i] * 0.5).toFloat()
        }
        return out.copyOfRange(n, n + x.size)
    }

    /** Average power spectrum of the quietest frames, or null if there is nothing but digital silence. */
    private fun noiseSpectrum(padded: FloatArray, frames: Int, hop: Int, win: FloatArray, fft: Fft): DoubleArray? {
        val n = win.size
        // Frames that lie wholly inside the clip, or every frame if the clip is shorter than one.
        val firstReal = n / hop
        val lastReal = frames - 1 - n / hop
        val range = if (lastReal >= firstReal) firstReal..lastReal else 0 until frames
        val energy = DoubleArray(frames)
        for (f in range) {
            var e = 0.0
            val s = f * hop
            for (i in 0 until n) {
                val v = padded[s + i] * win[i]
                e += v * v
            }
            energy[f] = e
        }
        // Exact digital silence (a recorder warming up, padding) is not the noise to remove, so leave it out.
        val silence = 1e-12 * n
        val idx = range.filter { energy[it] > silence }
        if (idx.isEmpty()) return null
        val take = max(min(8, idx.size), (idx.size * QUIET_SHARE).toInt())
        val quiet = idx.sortedBy { energy[it] }.take(take).toIntArray()
        return quietSpectrum(padded, quiet, hop, win, fft)
    }

    /**
     * How loud the noise is in each frame, relative to [profile]. Gerkmann and Hendriks's speech-presence-probability
     * tracker follows the noise in every bin, forwards and then backwards through the clip, starting from the profile;
     * only the overall level is kept (the geometric mean over bins), so the profile's shape stays put, and the smaller
     * of the two directions is used, so a tracker still catching up after a word is not mistaken for louder noise.
     * Frames of exact digital silence leave the level unchanged. Memory is one number per frame per direction.
     */
    private fun noiseLevels(
        padded: FloatArray, frames: Int, hop: Int, win: FloatArray, fft: Fft, profile: DoubleArray, cancelled: () -> Boolean,
    ): DoubleArray {
        val forward = trackLevels(padded, 0 until frames, frames, hop, win, fft, profile, cancelled)
        val backward = trackLevels(padded, frames - 1 downTo 0, frames, hop, win, fft, profile, cancelled)
        return DoubleArray(frames) { min(forward[it], backward[it]) * LEVEL_BIAS }
    }

    private fun trackLevels(
        padded: FloatArray, order: IntProgression, frames: Int, hop: Int, win: FloatArray, fft: Fft, profile: DoubleArray,
        cancelled: () -> Boolean,
    ): DoubleArray {
        val n = win.size
        val bins = profile.size
        val silence = 1e-12 * n
        val noise = profile.copyOf()
        val levels = DoubleArray(frames)
        val re = DoubleArray(n)
        val im = DoubleArray(n)
        var level = 1.0
        var count = 0
        for (f in order) {
            if (++count % 64 == 0 && cancelled()) break
            val s = f * hop
            var e = 0.0
            for (i in 0 until n) {
                val v = padded[s + i] * win[i]
                re[i] = v.toDouble()
                im[i] = 0.0
                e += v * v
            }
            if (e > silence) {
                fft.transform(re, im, inverse = false)
                var logSum = 0.0
                var used = 0
                var k = 1
                while (k < bins - 1) {
                    val y = re[k] * re[k] + im[k] * im[k]
                    // Probability that speech is present in this bin, given how far it stands above the noise.
                    val p = 1.0 / (1.0 + (1.0 + XI_SPEECH) * exp(-y / max(noise[k], 1e-30) * XI_SPEECH / (1.0 + XI_SPEECH)))
                    noise[k] = TRACK_SMOOTHING * noise[k] + (1 - TRACK_SMOOTHING) * ((1 - p) * y + p * noise[k])
                    if (profile[k] > 0.0) {
                        logSum += ln(max(noise[k], 1e-30) / profile[k])
                        used++
                    }
                    k += TRACK_STEP
                }
                if (used > 0) level = exp(logSum / used)
            }
            levels[f] = level
        }
        return levels
    }

    private fun quietSpectrum(padded: FloatArray, frames: IntArray, hop: Int, win: FloatArray, fft: Fft): DoubleArray? {
        val n = win.size
        val bins = n / 2 + 1
        val sum = DoubleArray(bins)
        val re = DoubleArray(n)
        val im = DoubleArray(n)
        for (f in frames) {
            val s = f * hop
            for (i in 0 until n) {
                re[i] = (padded[s + i] * win[i]).toDouble()
                im[i] = 0.0
            }
            fft.transform(re, im, inverse = false)
            for (k in 0 until bins) sum[k] += re[k] * re[k] + im[k] * im[k]
        }
        var total = 0.0
        for (k in 0 until bins) {
            sum[k] /= frames.size
            total += sum[k]
        }
        return if (total <= 0.0) null else sum
    }
}

/** In-place iterative radix-2 complex FFT. The inverse includes the 1/n scaling. */
internal class Fft(private val n: Int) {
    private val cosT = DoubleArray(n / 2) { cos(2 * PI * it / n) }
    private val sinT = DoubleArray(n / 2) { sin(2 * PI * it / n) }
    private val rev = IntArray(n).also { r ->
        val bits = Integer.numberOfTrailingZeros(n)
        for (i in 0 until n) r[i] = Integer.reverse(i) ushr (32 - bits)
    }

    init {
        require(n >= 2 && n and (n - 1) == 0) { "FFT size must be a power of two" }
    }

    fun transform(re: DoubleArray, im: DoubleArray, inverse: Boolean) {
        for (i in 0 until n) {
            val j = rev[i]
            if (j > i) {
                var t = re[i]; re[i] = re[j]; re[j] = t
                t = im[i]; im[i] = im[j]; im[j] = t
            }
        }
        val sign = if (inverse) 1.0 else -1.0
        var size = 2
        while (size <= n) {
            val half = size / 2
            val step = n / size
            var i = 0
            while (i < n) {
                var k = 0
                for (j in i until i + half) {
                    val wr = cosT[k]
                    val wi = sign * sinT[k]
                    val xr = re[j + half] * wr - im[j + half] * wi
                    val xi = re[j + half] * wi + im[j + half] * wr
                    re[j + half] = re[j] - xr
                    im[j + half] = im[j] - xi
                    re[j] += xr
                    im[j] += xi
                    k += step
                }
                i += size
            }
            size *= 2
        }
        if (inverse) {
            val s = 1.0 / n
            for (i in 0 until n) {
                re[i] *= s
                im[i] *= s
            }
        }
    }
}

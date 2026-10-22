package com.l0gic0l.glados.dsp

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Offline noise reduction for steady background noise: hiss, fans, air conditioning, hum.
 *
 * The clip is cut into overlapping short-time spectra. The noise spectrum is learned from the clip itself, as the
 * average of its quietest frames (the pauses between words). Each frequency is then scaled by a Wiener gain whose
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

        val noise = noiseSpectrum(padded, frames, hop, win, fft) ?: return x
        val overSubtract = 1.0 + 0.75 * t
        val floor = 10.0.pow(-30.0 * t / 20.0)
        for (k in 0 until bins) noise[k] = max(noise[k] * overSubtract, 1e-20)

        val re = DoubleArray(n)
        val im = DoubleArray(n)
        val prevClean = DoubleArray(bins) // |S|^2 estimate from the previous frame, before the floor
        val out = FloatArray(padded.size)
        var first = true
        for (f in 0 until frames) {
            if (f % 64 == 0 && cancelled()) break
            val start = f * hop
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

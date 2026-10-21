package com.l0gic0l.glados.dsp

import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin

/**
 * Streaming pitch corrector and formant shifter (causal TD-PSOLA).
 *
 * Per block it (1) ingests input through a noise gate, (2) runs YIN every [HOP] samples and feeds
 * the tuner, (3) tracks pitch marks, and (4) lays down windowed grains. Grains are spaced by the
 * *target* period, which sets the pitch, and are cut from the nearest input mark, which keeps
 * the original vocal tract. Stretching or squeezing each grain sets the formants.
 *
 * Output is delayed by [latencySamples] so that marks and pitch frames are always available.
 * Not thread-safe: call [configure] and [process] from the same thread.
 */
class LivePitchEngine(private val sampleRate: Int, params: VoiceParams = VoiceParams.defaults()) {
    companion object {
        const val HOP = 128
        private const val RING = 1 shl 15
        private const val MASK = RING - 1
        private const val ACC = 1 shl 14
        private const val AMASK = ACC - 1
        private const val FRAMES = 256
        private const val MARKS = 512
        private const val NONE = Long.MIN_VALUE / 4

        /** Absolute positions are rebased well before they could overflow an Int index (2^31 samples is about 12 h at 48 kHz). */
        private const val REBASE_AT = 1L shl 30
        private const val REBASE_BY = 1L shl 29
    }

    // ---- configuration ----
    private var fmin = 0.0
    private var fmax = 0.0
    private lateinit var yin: Yin
    private var dMap = 0
    private var a4 = 440.0
    private var shift = 0.0
    private var formantSemis = 0.0
    private var formantFollow = 0.0
    private var formantRatio = 1.0
    private var breath = 0.8f
    private var dry = 0f
    private var gateLin = 0.001f
    private var yinThreshold = 0.16
    private var autoRegister = true
    private var registerMidi = 57.0
    private var vibratoDepth = 0.0
    private var vibratoRate = 5.5
    private val tuner = Tuner(HOP.toDouble() / sampleRate)

    // ---- input side ----
    private val inBuf = FloatArray(RING)
    private val anaBuf = FloatArray(RING) // band-passed copy, for pitch analysis only
    private lateinit var band: PitchBandpass
    private var wIn = 0L
    private var dcX = 0f
    private var dcY = 0f
    private var gateEnv = 0f
    private var gateHold = 0
    private var gateOpen = false
    private var gateGain = 0f
    private var sinceDetect = 0

    // ---- pitch frames (one per HOP) ----
    private val fTime = LongArray(FRAMES)
    private val fVoiced = BooleanArray(FRAMES)
    private val fInHz = DoubleArray(FRAMES)
    private val fOutHz = DoubleArray(FRAMES)
    private var fCount = 0L
    private var hang = 0
    private var lastF0 = 0.0
    private var wasPeriodic = false
    private val med = DoubleArray(3)
    private var medN = 0
    private var vibPhase = 0.0
    private var regAvg = 0.0
    private var regN = 0
    private var autoShift = 0.0

    // ---- pitch marks ----
    private val mPos = LongArray(MARKS)
    private var mCount = 0L
    private var chain = false
    private var chainPeriod = 0.0
    private var lastMarkPos = NONE

    // ---- synthesis ----
    private val acc = FloatArray(ACC)
    private val norm = FloatArray(ACC)
    private var wOut = 0L
    private var sNext = 0.0

    // ---- telemetry (read from another thread) ----
    @Volatile
    var inputHz = 0.0
        private set

    @Volatile
    var outputHz = 0.0
        private set

    @Volatile
    var targetMidi = Int.MIN_VALUE
        private set

    /** Grains that wanted samples not yet received. Should stay 0; exposed for tests. */
    var starvedGrains = 0
        private set

    /** Fixed delay between input and output, in samples. */
    val latencySamples: Int get() = dMap

    init {
        configure(params)
    }

    fun configure(p: VoiceParams) {
        val newMin = p[Specs.FMIN].toDouble()
        val newMax = max(p[Specs.FMAX].toDouble(), newMin * 2.0)
        if (!::yin.isInitialized || newMin != fmin || newMax != fmax) {
            fmin = newMin
            fmax = newMax
            yin = Yin(sampleRate, fmin, fmax)
            band = PitchBandpass(sampleRate, fmin, fmax)
            dMap = (3.5 * sampleRate / fmin).roundToInt() + 64
            resetStreams()
        }
        a4 = p[Specs.REF_A4].toDouble()
        shift = p[Specs.SHIFT].toDouble()
        formantSemis = p[Specs.FORMANT].toDouble()
        formantFollow = p[Specs.FORMANT_FOLLOW] / 100.0
        updateFormant()
        breath = p[Specs.BREATH] / 100f
        dry = p[Specs.DRY] / 100f
        gateLin = 10f.pow(p[Specs.GATE] / 20f)
        yinThreshold = Voicing.threshold(p[Specs.VOICING])
        autoRegister = p.bool(Specs.AUTO_REG)
        registerMidi = Pitch.hzToMidi(p[Specs.REG_HZ].toDouble(), a4)
        vibratoDepth = p[Specs.VIB_DEPTH] / 100.0
        vibratoRate = p[Specs.VIB_RATE].toDouble()

        tuner.mask = Scales.mask(p.int(Specs.SCALE), p.int(Specs.KEY), p[Specs.CUSTOM_MASK].roundToInt())
        tuner.retuneMs = p[Specs.RETUNE].toDouble()
        tuner.strength = p[Specs.STRENGTH] / 100.0
        tuner.holdSemitones = p[Specs.HOLD] / 100.0
        tuner.monotone = p.bool(Specs.MONOTONE)
        tuner.monotoneNote = p.int(Specs.MONO_NOTE)
    }

    /** Clears audio state while keeping the configuration. */
    fun resetStreams() {
        inBuf.fill(0f)
        anaBuf.fill(0f)
        band.reset()
        wasPeriodic = false
        dcX = 0f
        dcY = 0f
        gateEnv = 0f
        acc.fill(0f)
        norm.fill(0f)
        wIn = 0
        wOut = 0
        sNext = 0.0
        fCount = 0
        mCount = 0
        chain = false
        lastMarkPos = NONE
        sinceDetect = 0
        medN = 0
        hang = 0
        tuner.reset()
        gateGain = 0f
        gateOpen = false
        gateHold = 0
        starvedGrains = 0
    }

    /** Processes [n] samples. [input] and [output] may be the same array. */
    fun process(input: FloatArray, output: FloatArray, n: Int) {
        if (wIn > REBASE_AT) shiftClock(-REBASE_BY)
        ingest(input, n)
        advanceMarks()
        synthesize(n)
        emit(output, n)
    }

    /**
     * Moves every absolute position by [delta] samples. [delta] must be a multiple of the ring sizes so that
     * buffer indices are unchanged. Used to rebase before positions grow past what an Int index can hold, and by
     * tests to simulate many hours of streaming.
     */
    internal fun shiftClock(delta: Long) {
        require(delta % RING == 0L) { "delta must keep ring indices aligned" }
        wIn += delta
        wOut += delta
        sNext += delta.toDouble()
        if (lastMarkPos != NONE) lastMarkPos += delta
        for (i in mPos.indices) mPos[i] += delta
        for (i in fTime.indices) fTime[i] += delta
    }

    // ------------------------------------------------------------------ input

    private fun ingest(input: FloatArray, n: Int) {
        val decay = exp(-1.0 / (0.020 * sampleRate)).toFloat()
        val attack = (1.0 - exp(-1.0 / (0.002 * sampleRate))).toFloat()
        val release = (1.0 - exp(-1.0 / (0.050 * sampleRate))).toFloat()
        val holdSamples = (0.120 * sampleRate).toInt()
        for (i in 0 until n) {
            var raw = input[i]
            // A NaN or Inf from a misbehaving driver would otherwise poison the filters for good.
            if (!(raw > -8f && raw < 8f)) raw = 0f
            // DC blocker
            val x = raw - dcX + 0.998f * dcY
            dcX = raw
            dcY = x

            val ax = abs(x)
            gateEnv = if (ax > gateEnv) ax else gateEnv * decay
            val threshold = if (gateOpen) gateLin * 0.7f else gateLin
            if (gateEnv > threshold) gateHold = holdSamples else if (gateHold > 0) gateHold--
            gateOpen = gateHold > 0
            gateGain += ((if (gateOpen) 1f else 0f) - gateGain) * (if (gateOpen) attack else release)

            val gated = x * gateGain
            inBuf[wIn.toInt() and MASK] = gated
            anaBuf[wIn.toInt() and MASK] = band.process(gated)
            wIn++
            if (++sinceDetect >= HOP) {
                sinceDetect = 0
                detectFrame()
            }
        }
    }

    private fun detectFrame() {
        val window = yin.windowLength
        if (wIn < window) {
            pushFrame(false, 0.0, 0.0)
            return
        }
        yin.detect(anaBuf, MASK, (wIn - window).toInt(), yinThreshold)
        // Hysteresis: a clear onset needs the strict threshold, but once voiced, a frame stays voiced while
        // it is still fairly periodic and its pitch is continuous. Keeps vowels from flickering.
        val clear = yin.aperiodicity < yinThreshold
        val continuing = wasPeriodic && lastF0 > 0.0 && yin.aperiodicity < yinThreshold * 1.5 &&
            abs(Math.log(yin.f0 / lastF0)) < 0.12
        val periodic = clear || continuing
        wasPeriodic = periodic && gateOpen
        var voiced = periodic && gateOpen
        var f0 = yin.f0
        if (voiced) {
            hang = 3
            lastF0 = f0
        } else if (hang > 0 && lastF0 > 0.0) {
            hang--
            voiced = true
            f0 = lastF0
        }
        if (!voiced) {
            tuner.update(0.0, false)
            medN = 0
            pushFrame(false, 0.0, 0.0)
            return
        }

        // Median of three keeps single-frame octave glitches out of the tuner.
        if (medN < 3) {
            med[medN++] = f0
        } else {
            med[0] = med[1]; med[1] = med[2]; med[2] = f0
        }
        val fUse = if (medN < 3) f0 else median3(med[0], med[1], med[2])

        val midiIn = Pitch.hzToMidi(fUse, a4)
        if (autoRegister) {
            regN++
            regAvg += max(1.0 / regN, 1.0 / 800.0) * (midiIn - regAvg)
            val target = (registerMidi - regAvg).coerceIn(-14.0, 14.0)
            autoShift += max(0.003, 1.0 / regN) * (target - autoShift)
        } else {
            autoShift += 0.02 * (0.0 - autoShift)
        }

        updateFormant()
        var midiOut = tuner.update(midiIn + shift + autoShift, true)
        if (vibratoDepth > 0.0) {
            vibPhase += 2.0 * PI * vibratoRate * HOP / sampleRate
            if (vibPhase > 2.0 * PI) vibPhase -= 2.0 * PI
            midiOut += vibratoDepth * sin(vibPhase)
        }
        val outHz = Pitch.midiToHz(midiOut, a4).coerceIn(40.0, 1600.0)
        pushFrame(true, fUse, outHz)
        inputHz = fUse
        outputHz = outHz
        targetMidi = tuner.target
    }

    private fun updateFormant() {
        val semis = (formantSemis + formantFollow * (shift + autoShift)).coerceIn(-12.0, 12.0)
        formantRatio = 2.0.pow(semis / 12.0)
    }

    private fun median3(a: Double, b: Double, c: Double): Double =
        max(min(a, b), min(max(a, b), c))

    private fun pushFrame(voiced: Boolean, inHz: Double, outHz: Double) {
        val i = (fCount % FRAMES).toInt()
        fTime[i] = wIn - yin.windowLength / 2
        fVoiced[i] = voiced
        fInHz[i] = inHz
        fOutHz[i] = outHz
        fCount++
        if (!voiced) {
            inputHz = 0.0
            outputHz = 0.0
        }
    }

    // ------------------------------------------------------------------ frames and marks

    /** Index (in the frame ring) of the latest frame at or before [t]; the oldest if none. -1 if empty. */
    private fun frameAt(t: Long): Int {
        if (fCount == 0L) return -1
        val oldest = max(0L, fCount - FRAMES + 1)
        var k = fCount - 1
        while (k > oldest && fTime[(k % FRAMES).toInt()] > t) k--
        return (k % FRAMES).toInt()
    }

    private fun markAt(k: Long): Long = mPos[(k % MARKS).toInt()]

    private fun pushMark(pos: Long) {
        mPos[(mCount % MARKS).toInt()] = pos
        mCount++
        lastMarkPos = pos
    }

    private fun advanceMarks() {
        if (fCount == 0L) return
        val newest = ((fCount - 1) % FRAMES).toInt()

        if (!chain) {
            if (!fVoiced[newest]) return
            var k = fCount - 1
            val oldest = max(0L, fCount - FRAMES + 1)
            while (k > oldest && fVoiced[((k - 1) % FRAMES).toInt()]) k--
            val startTime = fTime[(k % FRAMES).toInt()]
            val period = sampleRate / fInHz[newest]
            val from = max(startTime, lastMarkPos + 1)
            val len = period.toInt()
            if (from + len + 2 > wIn || from < wIn - RING / 2) return
            pushMark(Marks.peak(anaBuf, MASK, from, len))
            chain = true
            chainPeriod = period
        }

        while (chain) {
            val last = markAt(mCount - 1)
            val fi = frameAt(last)
            var period = if (fi >= 0 && fVoiced[fi]) sampleRate / fInHz[fi] else chainPeriod
            period = period.coerceIn(sampleRate / fmax, sampleRate / fmin)
            chainPeriod = period
            val reach = (period * 1.7).toLong() + 4
            if (last + reach >= wIn) break
            val ahead = frameAt(last + period.toLong())
            if (ahead >= 0 && !fVoiced[ahead]) {
                chain = false
                break
            }
            pushMark(Marks.next(anaBuf, MASK, last, period))
        }
    }

    /** Index of the mark nearest [t], or -1 if none lies within three quarters of a period. */
    private fun nearestMark(t: Double): Long {
        if (mCount == 0L) return -1
        val oldest = max(0L, mCount - MARKS + 1)
        var k = mCount - 1
        while (k > oldest && markAt(k) > t) k--
        var best = k
        if (k + 1 < mCount && abs(markAt(k + 1) - t) < abs(markAt(k) - t)) best = k + 1
        val period = periodAt(best)
        return if (abs(markAt(best) - t) <= 0.75 * period) best else -1
    }

    private fun periodAt(k: Long): Double {
        val oldest = max(0L, mCount - MARKS + 1)
        val before = if (k > oldest) (markAt(k) - markAt(k - 1)).toDouble() else -1.0
        val after = if (k + 1 < mCount) (markAt(k + 1) - markAt(k)).toDouble() else -1.0
        val lo = sampleRate / fmax * 0.8
        val hi = sampleRate / fmin * 1.25
        val p = when {
            before > 0 && after > 0 -> if (max(before, after) < 1.6 * min(before, after)) 0.5 * (before + after) else min(before, after)
            before > 0 -> before
            after > 0 -> after
            else -> chainPeriod
        }
        return p.coerceIn(lo, hi)
    }

    // ------------------------------------------------------------------ synthesis

    private fun synthesize(n: Int) {
        val horizon = wOut + n
        val unvoicedHalf = sampleRate * 0.005
        val rho = formantRatio
        while (true) {
            val a = sNext - dMap
            val fi = frameAt(a.toLong())
            var voiced = false
            var tin = 0.0
            var center = a
            var ts = 0.0
            if (fi >= 0 && fVoiced[fi]) {
                val mi = nearestMark(a)
                if (mi >= 0) {
                    voiced = true
                    tin = periodAt(mi)
                    center = markAt(mi).toDouble()
                    ts = sampleRate / fOutHz[fi]
                }
            }
            val half: Double
            val gain: Float
            if (voiced) {
                half = min(max(tin, 0.6 * ts * rho), 2.0 * tin)
                gain = 1f
            } else {
                half = unvoicedHalf
                ts = half / rho
                gain = breath
                center = a
            }
            if (sNext - half / rho >= horizon) break
            if (center + half + 2 > wIn) {
                starvedGrains++
                center = (wIn - half - 2).toDouble()
            }
            Grain.add(inBuf, MASK, center, half, acc, norm, AMASK, sNext, rho, gain)
            sNext += ts
        }
    }

    private fun emit(output: FloatArray, n: Int) {
        for (i in 0 until n) {
            val pos = wOut + i
            val idx = pos.toInt() and AMASK
            val nr = norm[idx]
            var y = acc[idx] / max(nr, Grain.NORM_FLOOR)
            acc[idx] = 0f
            norm[idx] = 0f
            if (dry > 0f) y = y * (1f - dry) + dry * inBuf[(pos - dMap).toInt() and MASK]
            output[i] = y
        }
        wOut += n
    }
}

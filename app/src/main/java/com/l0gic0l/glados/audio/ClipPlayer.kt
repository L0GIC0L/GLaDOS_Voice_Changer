package com.l0gic0l.glados.audio

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Process
import com.l0gic0l.glados.dsp.FxChain
import com.l0gic0l.glados.model.VoiceParams
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlin.math.max
import kotlin.math.min

/**
 * Plays a pitch-edited clip through the effects chain. The effects run in real time, so sliders in
 * the Voice tab can be moved while a clip plays. The clip itself can be swapped mid-playback, which
 * is how editor changes are heard immediately.
 */
class ClipPlayer(private val params: StateFlow<VoiceParams>) {
    private val _playing = MutableStateFlow(false)
    val playing: StateFlow<Boolean> = _playing.asStateFlow()

    @Volatile
    private var clip: FloatArray = FloatArray(0)

    @Volatile
    private var sampleRate = 48000

    @Volatile
    private var position = 0

    @Volatile
    private var seeked = false

    @Volatile
    private var stopRequested = false

    @Volatile
    var loop = false

    @Volatile
    private var thread: Thread? = null

    val positionSeconds: Float get() = position.toFloat() / sampleRate
    val durationSeconds: Float get() = clip.size.toFloat() / sampleRate

    /** Sets the clip. If the sample rate matches and a clip is loaded, the playhead is kept. */
    fun setClip(samples: FloatArray, rate: Int) {
        if (rate != sampleRate) {
            stop()
            sampleRate = rate
            position = 0
        }
        clip = samples
        if (position > samples.size) position = 0
    }

    fun seek(seconds: Float) {
        position = (seconds * sampleRate).toInt().coerceIn(0, clip.size)
        seeked = true
    }

    fun play(fromStart: Boolean = false) {
        if (clip.isEmpty()) return
        if (fromStart || position >= clip.size) position = 0
        if (thread != null) return
        stopRequested = false
        _playing.value = true
        val t = Thread({ run() }, "glados-clip")
        thread = t
        t.start()
    }

    fun pause() {
        stopRequested = true
        thread?.let { runCatching { it.join(1000) } }
        thread = null
    }

    fun stop() {
        pause()
        position = 0
    }

    private fun run() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        val sr = sampleRate
        val block = 1024
        val minBytes = AudioTrack.getMinBufferSize(sr, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        val track = try {
            AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build(),
                )
                .setAudioFormat(
                    AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setSampleRate(sr)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build(),
                )
                .setBufferSizeInBytes(max(minBytes, block * 4 * 2))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()
        } catch (e: Exception) {
            _playing.value = false
            thread = null
            return
        }
        val fx = FxChain(sr)
        var applied: VoiceParams? = null
        val buf = FloatArray(block)
        var tailLeft = -1
        try {
            track.play()
            while (!stopRequested) {
                val p = params.value
                if (p !== applied) {
                    applied = p
                    fx.configure(p)
                }
                if (seeked) {
                    seeked = false
                    fx.reset()
                    tailLeft = -1
                }
                val data = clip
                val pos = position
                val n = min(block, max(0, data.size - pos))
                if (n > 0) {
                    System.arraycopy(data, pos, buf, 0, n)
                    position = pos + n
                }
                if (n < block) buf.fill(0f, n, block)
                fx.process(buf, block)
                track.write(buf, 0, block, AudioTrack.WRITE_BLOCKING)

                if (position >= data.size) {
                    if (loop) {
                        position = 0
                        tailLeft = -1
                    } else {
                        if (tailLeft < 0) tailLeft = (FxChain.tailSeconds(p) * sr).toInt()
                        tailLeft -= block
                        if (tailLeft <= 0) break
                    }
                }
            }
            if (!stopRequested) track.stop()
        } finally {
            track.release()
            if (!stopRequested) position = 0
            _playing.value = false
            if (!stopRequested) thread = null
        }
    }
}

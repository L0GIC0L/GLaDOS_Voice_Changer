package com.l0gic0l.glados.audio

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import androidx.core.content.ContextCompat
import com.l0gic0l.glados.dsp.Audio
import kotlin.math.abs
import kotlin.math.max

/** Records the microphone, unprocessed, into memory for the editor. */
class Recorder(private val context: Context) {
    @Volatile
    var level = 0f
        private set

    @Volatile
    var recording = false
        private set

    @Volatile
    private var thread: Thread? = null

    @Volatile
    private var result: Audio? = null

    /** Called, from the recording thread, when recording ends by itself rather than through [stop]. */
    @Volatile
    var onAutoStop: ((String) -> Unit)? = null
    private var chunks = ArrayList<FloatArray>()

    @SuppressLint("MissingPermission") // checked immediately below
    fun start(sampleRate: Int = 48000, maxSeconds: Int = 120): Boolean {
        if (recording) return false
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) return false
        val min = AudioRecord.getMinBufferSize(sampleRate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        if (min <= 0) return false
        val rec = MicInput.open(sampleRate, max(min, 8192 * 4)) ?: return false
        chunks = ArrayList()
        result = null
        recording = true
        val t = Thread {
            val buf = FloatArray(4096)
            var total = 0
            var reason: String? = null
            try {
                rec.startRecording()
                while (recording) {
                    if (total >= maxSeconds * sampleRate) {
                        reason = "Recording stopped at the ${maxSeconds / 60} minute limit."
                        break
                    }
                    val n = rec.read(buf, 0, buf.size, AudioRecord.READ_BLOCKING)
                    if (n <= 0) {
                        reason = "Recording stopped because the microphone became unavailable."
                        break
                    }
                    chunks += buf.copyOf(n)
                    total += n
                    var peak = 0f
                    for (i in 0 until n) peak = max(peak, abs(buf[i]))
                    level = peak
                }
            } finally {
                runCatching { rec.stop() }
                rec.release()
                val all = FloatArray(total)
                var o = 0
                for (c in chunks) {
                    System.arraycopy(c, 0, all, o, c.size)
                    o += c.size
                }
                result = Audio(all, sampleRate)
                level = 0f
                // If the loop ended on its own, `recording` is still true here: tell the owner.
                val stoppedItself = recording
                if (stoppedItself) onAutoStop?.invoke(reason ?: "Recording stopped.")
                recording = false
            }
        }
        thread = t
        t.start()
        return true
    }

    /** Stops and returns the recording, or null if nothing was captured. */
    fun stop(): Audio? {
        recording = false
        thread?.let { runCatching { it.join(2000) } }
        thread = null
        return result?.takeIf { it.samples.isNotEmpty() }
    }
}

package com.l0gic0l.glados.audio

import android.content.Context
import android.Manifest
import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.os.Process
import androidx.core.content.ContextCompat
import com.l0gic0l.glados.dsp.FxChain
import com.l0gic0l.glados.dsp.LivePitchEngine
import com.l0gic0l.glados.model.VoiceParams
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlin.math.abs
import kotlin.math.max

data class LiveState(
    val running: Boolean = false,
    val error: String? = null,
    val latencyMs: Int = 0,
    val sampleRate: Int = 0,
    /** Something the user should know about while live, such as output having been muted. */
    val notice: String? = null,
)

/**
 * Real-time microphone to speaker path: AudioRecord -> pitch engine -> effects -> AudioTrack,
 * run on a dedicated audio-priority thread inside [LiveService] so it survives the screen turning off.
 */
class LiveController(
    private val context: Context,
    private val params: StateFlow<VoiceParams>,
) {
    private val _state = MutableStateFlow(LiveState())
    val state: StateFlow<LiveState> = _state.asStateFlow()

    val trace = PitchTrace()

    private val _bypass = MutableStateFlow(false)
    private val _muted = MutableStateFlow(false)

    /** Observable by the UI; also read directly by the audio thread. */
    val bypassState: StateFlow<Boolean> = _bypass.asStateFlow()
    val mutedState: StateFlow<Boolean> = _muted.asStateFlow()

    var bypass: Boolean
        get() = _bypass.value
        set(v) { _bypass.value = v }

    var muted: Boolean
        get() = _muted.value
        set(v) { _muted.value = v }

    @Volatile
    var inputDeviceId = 0

    @Volatile
    var outputDeviceId = 0

    @Volatile
    var inputLevel = 0f
        private set

    @Volatile
    var outputLevel = 0f
        private set

    @Volatile
    var targetMidi = Int.MIN_VALUE
        private set

    /** True while the latest analysis frame is pitched voice. */
    @Volatile
    var voiceDetected = false
        private set

    /** True when the mic has been hearing sound for a while but no pitched voice: noise, whisper, or settings too strict. */
    @Volatile
    var noVoiceWarning = false
        private set

    @Volatile private var thread: Thread? = null
    @Volatile private var stopRequested = false

    /**
     * Headphones unplugged or a Bluetooth speaker dropped: Android would move the output to the loudspeaker, where
     * the processed voice would be picked straight back up by the microphone. Mute instead and say so.
     */
    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                muted = true
                _state.value = _state.value.copy(notice = "Output device disconnected. Muted to avoid feedback.")
            }
        }
    }
    private var receiverRegistered = false

    /** Starts the foreground service, which calls [begin]. Caller must hold RECORD_AUDIO. */
    fun start() {
        if (_state.value.running) return
        ContextCompat.startForegroundService(context, Intent(context, LiveService::class.java))
    }

    /** Stops audio and the service. */
    fun stop() {
        stopRequested = true
        context.stopService(Intent(context, LiveService::class.java))
        thread?.let {
            runCatching { it.join(1500) }
        }
        thread = null
    }

    /** Called by the service once it is in the foreground. */
    internal fun begin() {
        if (thread != null) return
        stopRequested = false
        _state.value = LiveState(running = true)
        if (!receiverRegistered) {
            ContextCompat.registerReceiver(
                context, noisyReceiver, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED,
            )
            receiverRegistered = true
        }
        val t = Thread({ loop() }, "glados-live")
        thread = t
        t.start()
    }

    /** Called by the service when it is destroyed. */
    internal fun halt() {
        stopRequested = true
        unregisterReceiver()
    }

    private fun unregisterReceiver() {
        if (receiverRegistered) {
            receiverRegistered = false
            runCatching { context.unregisterReceiver(noisyReceiver) }
        }
    }

    private fun fail(message: String) {
        thread = null
        _state.value = LiveState(running = false, error = message)
        context.stopService(Intent(context, LiveService::class.java))
    }

    @SuppressLint("MissingPermission") // checked immediately below
    private fun loop() {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            return fail("Microphone permission was not granted.")
        }
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val sr = am.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)?.toIntOrNull() ?: 48000
        val framesPerBuffer = am.getProperty(AudioManager.PROPERTY_OUTPUT_FRAMES_PER_BUFFER)?.toIntOrNull() ?: 192
        val block = max(framesPerBuffer, 192)

        val inMin = AudioRecord.getMinBufferSize(sr, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        val outMin = AudioTrack.getMinBufferSize(sr, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        if (inMin <= 0 || outMin <= 0) return fail("This device does not support low-latency float audio.")

        val record = MicInput.open(sr, max(inMin, block * 4 * 2))
            ?: return fail("Microphone unavailable. Is another app using it?")
        val track: AudioTrack
        try {
            track = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build(),
                )
                .setAudioFormat(
                    AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setSampleRate(sr)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build(),
                )
                .setBufferSizeInBytes(max(outMin, block * 4 * 2))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .build()
        } catch (e: Exception) {
            record.release()
            return fail("Could not open the audio output: ${e.message}")
        }

        var appliedIn = -1
        var appliedOut = -1
        var appliedParams: VoiceParams? = null
        val engine = LivePitchEngine(sr, params.value)
        val fx = FxChain(sr)
        val buf = FloatArray(block)
        val out = FloatArray(block)

        try {
            record.startRecording()
            track.play()
            val deviceLatency = (track.bufferSizeInFrames + record.bufferSizeInFrames / 2) * 1000 / sr
            var frame = 0
            var loudUnpitchedSamples = 0L
            while (!stopRequested) {
                val inId = inputDeviceId
                if (inId != appliedIn) {
                    appliedIn = inId
                    record.setPreferredDevice(AudioDevices.find(AudioDevices.inputs(context), inId))
                }
                val outId = outputDeviceId
                if (outId != appliedOut) {
                    appliedOut = outId
                    track.setPreferredDevice(AudioDevices.find(AudioDevices.outputs(context), outId))
                }
                val p = params.value
                if (p !== appliedParams) {
                    appliedParams = p
                    engine.configure(p)
                    fx.configure(p)
                    _state.value = _state.value.copy(
                        latencyMs = engine.latencySamples * 1000 / sr + deviceLatency, sampleRate = sr,
                    )
                }

                val n = record.read(buf, 0, block, AudioRecord.READ_BLOCKING)
                if (n < 0) {
                    fail("Audio input stopped (error $n).")
                    break
                }
                if (n == 0) continue

                var peakIn = 0f
                for (i in 0 until n) peakIn = max(peakIn, abs(buf[i]))
                if (bypass) {
                    System.arraycopy(buf, 0, out, 0, n)
                } else {
                    engine.process(buf, out, n)
                    fx.process(out, n)
                }
                var peakOut = 0f
                if (muted) out.fill(0f, 0, n)
                for (i in 0 until n) peakOut = max(peakOut, abs(out[i]))
                inputLevel = peakIn
                outputLevel = peakOut
                val written = track.write(out, 0, n, AudioTrack.WRITE_BLOCKING)
                if (written < 0) {
                    fail("Audio output stopped (error $written). Tap Go live to restart.")
                    break
                }

                // Sustained loud-but-unpitched input means detection is failing, not that the speaker paused.
                val pitched = engine.inputHz > 0
                voiceDetected = pitched && !bypass
                loudUnpitchedSamples = if (peakIn > 0.02f && !pitched) loudUnpitchedSamples + n else 0L
                noVoiceWarning = loudUnpitchedSamples > sr * 3L / 2

                if (++frame % 4 == 0) {
                    trace.push(engine.inputHz.toFloat(), if (bypass) engine.inputHz.toFloat() else engine.outputHz.toFloat())
                    targetMidi = engine.targetMidi
                }
            }
        } finally {
            runCatching { record.stop() }
            record.release()
            runCatching { track.stop() }
            track.release()
            inputLevel = 0f
            outputLevel = 0f
            voiceDetected = false
            noVoiceWarning = false
            unregisterReceiver()
            trace.clear()
            thread = null
            if (_state.value.error == null) _state.value = LiveState()
        }
    }
}

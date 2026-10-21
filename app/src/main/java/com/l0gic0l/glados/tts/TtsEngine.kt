package com.l0gic0l.glados.tts

import android.content.Context
import android.os.Bundle
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import com.l0gic0l.glados.dsp.Audio
import com.l0gic0l.glados.dsp.Wav
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

class TtsVoice(val name: String, val label: String)

/**
 * Wraps the system text-to-speech engine. Speech is rendered to a file rather than played, so it
 * can be pitch-corrected and run through the GLaDOS effects like any other clip.
 */
class TtsEngine(private val context: Context) {
    private val _ready = MutableStateFlow(false)
    val ready: StateFlow<Boolean> = _ready.asStateFlow()

    private val _voices = MutableStateFlow<List<TtsVoice>>(emptyList())
    val voices: StateFlow<List<TtsVoice>> = _voices.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    private var tts: TextToSpeech? = null
    private var defaultVoice: Voice? = null
    private var voiceByName: Map<String, Voice> = emptyMap()
    private val pending = ConcurrentHashMap<String, Pair<CancellableContinuation<Audio>, File>>()
    private var counter = 0

    /** Connects to the engine. Safe to call repeatedly. */
    fun init() {
        if (tts != null) return
        tts = TextToSpeech(context) { status ->
            val engine = tts
            if (status != TextToSpeech.SUCCESS || engine == null) {
                _error.value = "No text-to-speech engine is available. Install or enable one in Android settings."
                // Forget the failed instance so a later retry() can start over.
                engine?.shutdown()
                tts = null
                return@TextToSpeech
            }
            _error.value = null
            engine.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
                override fun onStart(utteranceId: String?) {}

                override fun onStop(utteranceId: String?, interrupted: Boolean) {
                    val id = utteranceId ?: return
                    pending.remove(id)?.let { (cont, file) ->
                        file.delete()
                        if (cont.isActive) cont.resumeWithException(IllegalStateException("The speech was interrupted."))
                    }
                }

                override fun onDone(utteranceId: String?) {
                    val id = utteranceId ?: return
                    val (cont, file) = pending.remove(id) ?: return
                    try {
                        val audio = Wav.read(file.readBytes())
                        file.delete()
                        if (cont.isActive) cont.resume(audio)
                    } catch (e: Exception) {
                        if (cont.isActive) cont.resumeWithException(e)
                    }
                }

                @Deprecated("Deprecated in Java")
                override fun onError(utteranceId: String?) {
                    val id = utteranceId ?: return
                    pending.remove(id)?.let { (cont, file) ->
                        file.delete()
                        if (cont.isActive) cont.resumeWithException(IllegalStateException("The speech engine failed."))
                    }
                }
            })
            val installed = runCatching { engine.voices.orEmpty() }.getOrDefault(emptySet<Voice>())
                .filter { !it.features.contains("notInstalled") }
                .sortedWith(compareBy({ it.locale.displayName }, { it.name }))
            // Looking voices up is a call into the speech service; do it once here rather than on every request.
            voiceByName = installed.associateBy { it.name }
            defaultVoice = runCatching { engine.defaultVoice }.getOrNull()
            _voices.value = installed.map { TtsVoice(it.name, "${it.locale.displayName} - ${it.name}") }
            _ready.value = true
        }
    }

    /**
     * Renders [text] to audio.
     * @param rate 0.25 to 3, 1 is normal
     * @param pitch 0.25 to 2, 1 is normal
     */
    suspend fun synthesize(text: String, voiceName: String?, rate: Float, pitch: Float): Audio {
        val engine = tts ?: throw IllegalStateException("Speech engine not ready.")
        // Always set a voice: choosing "Engine default" after a specific voice must go back to the default.
        (voiceName?.let { voiceByName[it] } ?: defaultVoice)?.let { engine.voice = it }
        engine.setSpeechRate(rate)
        engine.setPitch(pitch)
        val id = "u${counter++}"
        val file = File(context.cacheDir, "tts-$id.wav")
        return suspendCancellableCoroutine { cont ->
            pending[id] = cont to file
            cont.invokeOnCancellation {
                pending.remove(id)
                engine.stop()
                file.delete()
            }
            val result = engine.synthesizeToFile(text, Bundle(), file, id)
            if (result != TextToSpeech.SUCCESS) {
                pending.remove(id)
                if (cont.isActive) cont.resumeWithException(IllegalStateException("The speech engine rejected that text."))
            }
        }
    }

    /** Starts over after a failed [init]. */
    fun retry() {
        if (tts == null) {
            _error.value = null
            init()
        }
    }

    fun shutdown() {
        tts?.shutdown()
        tts = null
        _ready.value = false
    }
}

package com.l0gic0l.glados

import android.app.Application
import com.l0gic0l.glados.audio.ClipPlayer
import com.l0gic0l.glados.audio.Recorder
import com.l0gic0l.glados.data.Prefs
import com.l0gic0l.glados.data.ThemeSettings
import com.l0gic0l.glados.model.VoiceParams
import com.l0gic0l.glados.tts.TtsEngine
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.launch

/** Process-wide singletons: settings, presets, theme and the audio helpers. */
class AppContainer(app: Application) {
    val prefs = Prefs(app)

    private val _params = MutableStateFlow(prefs.loadParams())
    val params: StateFlow<VoiceParams> = _params.asStateFlow()

    private val _userPresets = MutableStateFlow(prefs.loadUserPresets())
    val userPresets: StateFlow<Map<String, Map<String, Float>>> = _userPresets.asStateFlow()

    private val _theme = MutableStateFlow(prefs.loadTheme())
    val theme: StateFlow<ThemeSettings> = _theme.asStateFlow()

    private val _simpleMode = MutableStateFlow(prefs.simpleMode)
    val simpleMode: StateFlow<Boolean> = _simpleMode.asStateFlow()

    val player = ClipPlayer(params)
    val recorder = Recorder(app)
    val tts = TtsEngine(app)

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    init {
        @OptIn(FlowPreview::class)
        scope.launch { params.debounce(400).collect { prefs.saveParams(it) } }
    }

    fun update(change: (VoiceParams) -> VoiceParams) {
        _params.value = change(_params.value)
    }

    fun setParams(p: VoiceParams) {
        _params.value = p
    }

    fun saveUserPreset(name: String) {
        val next = _userPresets.value + (name to _params.value.toMap())
        _userPresets.value = next
        prefs.saveUserPresets(next)
    }

    fun deleteUserPreset(name: String) {
        val next = _userPresets.value - name
        _userPresets.value = next
        prefs.saveUserPresets(next)
    }

    fun setSimpleMode(on: Boolean) {
        _simpleMode.value = on
        prefs.simpleMode = on
    }

    fun setTheme(t: ThemeSettings) {
        _theme.value = t
        prefs.saveTheme(t)
    }
}

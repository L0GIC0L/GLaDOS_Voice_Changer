package com.l0gic0l.glados

import android.app.Application
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.l0gic0l.glados.audio.AudioDecoder
import com.l0gic0l.glados.dsp.Analysis
import com.l0gic0l.glados.dsp.Analyzer
import com.l0gic0l.glados.dsp.Audio
import com.l0gic0l.glados.dsp.Edit
import com.l0gic0l.glados.dsp.FxChain
import com.l0gic0l.glados.dsp.NoteOps
import com.l0gic0l.glados.dsp.OfflineRenderer
import com.l0gic0l.glados.dsp.RenderSettings
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.dsp.Wav
import com.l0gic0l.glados.model.Presets
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

enum class Tool { PITCH, TIME, SPLIT }

class ClipState(val audio: Audio, val analysis: Analysis, val title: String, val autoCorrect: Boolean)

private data class RenderKey(
    val shift: Float, val formant: Float, val follow: Float, val autoReg: Boolean, val regHz: Float, val breath: Float, val a4: Float,
)

class MainViewModel(app: Application) : AndroidViewModel(app) {
    private val container = (app as GladosApp).container
    val params = container.params
    val userPresets = container.userPresets
    val theme = container.theme
    val live = container.live
    val player = container.player
    val recorder = container.recorder
    val tts = container.tts

    // ---- editor state ----
    var clip by mutableStateOf<ClipState?>(null)
        private set
    var edit by mutableStateOf<Edit?>(null)
        private set

    /** Bumped on every in-place change to [edit] so canvases know to redraw. */
    var editVersion by mutableIntStateOf(0)
        private set
    var selection by mutableStateOf<Set<Int>>(emptySet())
        private set
    var tool by mutableStateOf(Tool.PITCH)
    var snapToScale by mutableStateOf(true)
    var busy by mutableStateOf<String?>(null)
        private set
    var message by mutableStateOf<String?>(null)
    var recording by mutableStateOf(false)
        private set
    var canUndo by mutableStateOf(false)
        private set
    var canRedo by mutableStateOf(false)
        private set
    var segTolerance by mutableFloatStateOf(80f)
    var segSensitivity by mutableFloatStateOf(50f)

    /** True when detection settings changed after edits were made, so the notes no longer match the settings. */
    var analysisStale by mutableStateOf(false)
        private set

    private val undoStack = ArrayDeque<Edit>()
    private val redoStack = ArrayDeque<Edit>()
    private var lastCoalesce: String? = null
    private var lastMutation = 0L
    private var renderJob: Job? = null
    private var reanalyseJob: Job? = null
    private var loadJob: Job? = null
    private var dry: FloatArray? = null

    init {
        tts.init()
        viewModelScope.launch {
            params.map {
                RenderKey(
                    it[Specs.SHIFT], it[Specs.FORMANT], it[Specs.FORMANT_FOLLOW], it.bool(Specs.AUTO_REG),
                    it[Specs.REG_HZ], it[Specs.BREATH], it[Specs.REF_A4],
                )
            }.distinctUntilChanged().drop(1).collect { scheduleRender(80) }
        }
        // Voice-range and voicing settings change what counts as a note, so the clip has to be analysed again.
        viewModelScope.launch {
            params.map { Triple(it[Specs.VOICING], it[Specs.FMIN], it[Specs.FMAX]) }
                .distinctUntilChanged().drop(1).collect { onDetectionSettingsChanged() }
        }
    }

    private fun onDetectionSettingsChanged() {
        val cs = clip ?: return
        if (undoStack.isEmpty()) {
            // Nothing has been edited yet, so re-analysing loses nothing: do it automatically.
            reanalyseJob?.cancel()
            reanalyseJob = viewModelScope.launch {
                delay(500)
                loadAudio(cs.audio, cs.title, cs.autoCorrect, autoplay = false)
            }
        } else {
            analysisStale = true
        }
    }

    // ---- parameters ----

    fun setParam(spec: com.l0gic0l.glados.model.ParamSpec, value: Float) = container.update { it.with(spec, value) }

    fun resetParams(specs: List<com.l0gic0l.glados.model.ParamSpec>) =
        container.update { p -> specs.fold(p) { acc, s -> acc.with(s, s.default) } }

    fun applyPreset(name: String) {
        Presets.all.firstOrNull { it.name == name }?.let { container.setParams(it.apply()); return }
        userPresets.value[name]?.let { container.setParams(VoiceParams.fromMap(it)) }
    }

    fun saveUserPreset(name: String) {
        val clean = name.trim()
        when {
            clean.isEmpty() -> return
            Presets.all.any { it.name.equals(clean, ignoreCase = true) } -> message = "\"$clean\" is a built-in preset name. Choose another."
            else -> container.saveUserPreset(clean)
        }
    }
    fun deleteUserPreset(name: String) = container.deleteUserPreset(name)
    fun setTheme(t: com.l0gic0l.glados.data.ThemeSettings) = container.setTheme(t)

    // ---- selection and undo ----

    fun selectOnly(index: Int) { selection = setOf(index) }
    fun toggleSelect(index: Int) { selection = if (index in selection) selection - index else selection + index }
    fun selectAll() { selection = edit?.notes?.indices?.toSet() ?: emptySet() }
    fun clearSelection() { selection = emptySet() }

    private fun pushUndo() {
        val e = edit ?: return
        undoStack.addLast(e.copy())
        if (undoStack.size > 100) undoStack.removeFirst()
        redoStack.clear()
        canUndo = true
        canRedo = false
    }

    /** Call once when a drag starts, so the whole drag is a single undo step. */
    fun beginGesture() {
        pushUndo()
        lastCoalesce = null
    }

    /** Applies [block] to the edit in place and re-renders. Use between [beginGesture] and the end of a drag. */
    fun modify(block: (Edit, Analysis) -> Unit) {
        val e = edit ?: return
        val a = clip?.analysis ?: return
        block(e, a)
        bump()
    }

    /** A discrete change (macro, split, merge) or a slider move; consecutive moves with the same key share one undo step. */
    fun mutate(coalesceKey: String? = null, block: (Edit, Analysis) -> Unit) {
        val now = System.currentTimeMillis()
        if (coalesceKey == null || coalesceKey != lastCoalesce || now - lastMutation > 800) pushUndo()
        lastCoalesce = coalesceKey
        lastMutation = now
        modify(block)
    }

    fun undo() {
        val prev = undoStack.removeLastOrNull() ?: return
        edit?.let { redoStack.addLast(it.copy()) }
        edit = prev
        afterHistoryChange()
    }

    fun redo() {
        val next = redoStack.removeLastOrNull() ?: return
        edit?.let { undoStack.addLast(it.copy()) }
        edit = next
        afterHistoryChange()
    }

    private fun afterHistoryChange() {
        val n = edit?.notes?.size ?: 0
        selection = selection.filter { it < n }.toSet()
        canUndo = undoStack.isNotEmpty()
        canRedo = redoStack.isNotEmpty()
        lastCoalesce = null
        bump()
    }

    private fun bump() {
        editVersion++
        scheduleRender()
    }

    // ---- editor operations ----

    private fun targets(e: Edit): List<com.l0gic0l.glados.dsp.Note> =
        if (selection.isEmpty()) e.notes else selection.sorted().mapNotNull { e.notes.getOrNull(it) }

    fun applyCorrection(center: Double, drift: Double, mod: Double) = mutate {  e, a ->
        val p = params.value
        val shift = NoteOps.globalShift(a, p)
        val mask = Scales.mask(p.int(Specs.SCALE), p.int(Specs.KEY), p[Specs.CUSTOM_MASK].roundToInt())
        NoteOps.correct(targets(e), mask, center, drift, mod, shift, p.bool(Specs.MONOTONE), p.int(Specs.MONO_NOTE))
    }

    fun matchLiveSettings() = mutate { e, a ->
        val p = params.value
        NoteOps.correctLikeLive(targets(e), p, NoteOps.globalShift(a, p))
    }

    fun resetNotes() = mutate { e, _ ->
        for (n in targets(e)) {
            n.offset = 0.0; n.drift = 1f; n.mod = 1f; n.formant = 0f; n.gainDb = 0f
        }
    }

    fun resetTiming() = mutate { e, _ ->
        for (k in e.anchors.keys.toList()) e.anchors[k] = k.toDouble()
    }

    /**
     * Runs [block] on a throwaway copy first; only if it succeeds is the real edit changed and an undo step recorded.
     * A refused operation therefore leaves the undo and redo history untouched.
     */
    private fun tryMutate(failureMessage: String, block: (Edit, Analysis) -> Boolean): Boolean {
        val e = edit ?: return false
        val a = clip?.analysis ?: return false
        if (!block(e.copy(), a)) {
            message = failureMessage
            return false
        }
        pushUndo()
        lastCoalesce = null
        block(e, a)
        bump()
        return true
    }

    fun splitNote(index: Int, frame: Int) {
        if (tryMutate("That cut is too close to the edge of the note.") { e, a -> NoteOps.split(e, a, index, frame) }) {
            selection = setOf(index, index + 1)
        }
    }

    fun mergeSelected() {
        val idx = selection.sorted()
        if (idx.isEmpty()) return
        val first = idx.first()
        val last = if (idx.size == 1) first + 1 else idx.last()
        val ok = tryMutate("Those notes are not next to each other.") { e, a ->
            var merged = false
            for (i in last - 1 downTo first) merged = NoteOps.merge(e, a, i) || merged
            merged
        }
        if (ok) selection = setOf(first)
    }

    /** Re-runs pitch analysis on the current audio, for when the voice range or sensitivity settings changed. Edits are lost. */
    fun reanalyse() {
        val cs = clip ?: return
        loadAudio(cs.audio, cs.title, cs.autoCorrect, autoplay = false)
    }

    fun resegment() {
        val cs = clip ?: return
        val e = NoteOps.segment(cs.analysis, segTolerance.toDouble(), segSensitivity / 100.0)
        pushUndo()
        edit = e
        selection = emptySet()
        bump()
    }

    // ---- clip lifecycle ----

    /**
     * Analyses [audio], splits it into notes and makes it the editor clip. A newer call cancels an older one that is
     * still running, so two overlapping loads can never interleave their results.
     * @param autoCorrect apply the live Tuning settings to every note, so speech matches the live voice
     */
    fun loadAudio(audio: Audio, title: String, autoCorrect: Boolean, autoplay: Boolean) {
        loadJob?.cancel()
        renderJob?.cancel()
        val job = viewModelScope.launch(start = CoroutineStart.LAZY) {
            player.stop()
            busy = "Analysing..."
            val p = params.value
            try {
                val an = withContext(Dispatchers.Default) {
                    Analyzer.analyze(
                        audio.samples, audio.sampleRate, p,
                        progress = { busy = "Analysing ${(it * 100).toInt()}%" },
                        cancelled = { !isActive },
                    )
                }
                val e = withContext(Dispatchers.Default) {
                    NoteOps.segment(an, segTolerance.toDouble(), segSensitivity / 100.0).also {
                        if (autoCorrect) NoteOps.correctLikeLive(it.notes, p, NoteOps.globalShift(an, p))
                    }
                }
                val out = withContext(Dispatchers.Default) { renderClip(audio, an, e, params.value) { !isActive } }
                ensureActive()
                if (e.notes.isEmpty()) message = "No pitched voice found. It will play back unchanged."
                clip = ClipState(audio, an, title, autoCorrect)
                analysisStale = false
                edit = e
                selection = emptySet()
                undoStack.clear()
                redoStack.clear()
                canUndo = false
                canRedo = false
                editVersion++
                dry = out
                player.setClip(out, audio.sampleRate)
                if (autoplay) player.play(fromStart = true)
            } catch (ex: CancellationException) {
                throw ex
            } catch (ex: Exception) {
                message = "Could not analyse that audio: ${ex.message}"
            } finally {
                // A newer load owns the busy indicator once it has started.
                if (loadJob === coroutineContext[Job]) busy = null
            }
        }
        loadJob = job
        job.start()
    }

    private fun renderClip(audio: Audio, an: Analysis, e: Edit, p: VoiceParams, cancelled: () -> Boolean): FloatArray {
        val shift = NoteOps.globalShift(an, p)
        val settings = RenderSettings(shift, NoteOps.formantShift(p, shift), p[Specs.BREATH] / 100f)
        return OfflineRenderer.render(audio.samples, an, e, settings, cancelled)
    }

    private fun scheduleRender(delayMs: Long = 40) {
        val cs = clip ?: return
        val ed = edit?.copy() ?: return
        val p = params.value
        renderJob?.cancel()
        renderJob = viewModelScope.launch(Dispatchers.Default) {
            delay(delayMs)
            val out = renderClip(cs.audio, cs.analysis, ed, p) { !isActive }
            // Only publish if this render is still for the clip on screen.
            if (isActive && clip === cs) {
                dry = out
                player.setClip(out, cs.audio.sampleRate)
            }
        }
    }

    fun togglePlay() {
        if (player.playing.value) player.pause() else player.play()
    }

    fun toggleRecord() {
        if (!recording) {
            if (busy != null) return
            player.stop()
            recorder.onAutoStop = { reason ->
                // The recorder stopped by itself (length cap or a read error); finish up exactly as if the user had.
                viewModelScope.launch { finishRecording(reason) }
            }
            if (recorder.start()) recording = true else message = "Could not start the microphone."
        } else {
            finishRecording(null)
        }
    }

    private fun finishRecording(note: String?) {
        if (!recording) return
        recording = false
        val audio = recorder.stop()
        when {
            audio == null -> message = note ?: "Nothing was recorded."
            else -> {
                if (note != null) message = note
                loadAudio(audio, "Recording", autoCorrect = true, autoplay = true)
            }
        }
    }

    fun importFile(uri: Uri) {
        if (busy != null) return
        viewModelScope.launch {
            busy = "Decoding..."
            try {
                val decoded = withContext(Dispatchers.IO) { AudioDecoder.decode(getApplication(), uri) }
                if (decoded.truncated) message = "Only the first ${AudioDecoder.MAX_SECONDS / 60} minutes were imported."
                loadAudio(decoded.audio, "Imported audio", autoCorrect = true, autoplay = true)
            } catch (ex: CancellationException) {
                throw ex
            } catch (ex: Exception) {
                busy = null
                message = "Could not read that file: ${ex.message}"
            }
        }
    }

    fun exportWav(uri: Uri) {
        val cs = clip ?: return
        val samples = dry ?: return
        val p = params.value
        viewModelScope.launch {
            busy = "Rendering..."
            try {
                withContext(Dispatchers.IO) {
                    val sr = cs.audio.sampleRate
                    val out = samples.copyOf(samples.size + (FxChain.tailSeconds(p) * sr).toInt())
                    val fx = FxChain(sr).also { it.configure(p) }
                    val tmp = FloatArray(4096)
                    var i = 0
                    while (i < out.size) {
                        val n = minOf(tmp.size, out.size - i)
                        System.arraycopy(out, i, tmp, 0, n)
                        fx.process(tmp, n)
                        System.arraycopy(tmp, 0, out, i, n)
                        i += n
                    }
                    getApplication<Application>().contentResolver.openOutputStream(uri)?.use { Wav.write(it, Audio(out, sr)) }
                }
                message = "Saved."
            } catch (ex: Exception) {
                message = "Could not save: ${ex.message}"
            } finally {
                busy = null
            }
        }
    }

    // ---- text to speech ----

    fun speak(text: String, voice: String?, rate: Float, pitch: Float, autoTune: Boolean) {
        if (text.isBlank() || busy != null) return
        viewModelScope.launch {
            busy = "Speaking..."
            try {
                val raw = withTimeout(90_000) { tts.synthesize(text, voice, rate, pitch) }
                val peak = raw.samples.fold(0f) { m, v -> max(m, abs(v)) }
                if (peak > 1e-4f) {
                    val g = 0.8f / peak
                    for (i in raw.samples.indices) raw.samples[i] *= g
                }
                loadAudio(raw, "Speech", autoCorrect = autoTune, autoplay = true)
            } catch (ex: TimeoutCancellationException) {
                busy = null
                message = "The speech engine did not answer in time. Try again, or pick another voice."
            } catch (ex: CancellationException) {
                throw ex
            } catch (ex: Exception) {
                busy = null
                message = ex.message ?: "Speech failed."
            }
        }
    }

    override fun onCleared() {
        player.stop()
        if (recording) recorder.stop()
        super.onCleared()
    }
}

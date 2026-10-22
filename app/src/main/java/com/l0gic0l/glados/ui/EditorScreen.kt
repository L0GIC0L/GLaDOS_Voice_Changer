package com.l0gic0l.glados.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Redo
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.ZoomIn
import androidx.compose.material.icons.filled.ZoomOut
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.Tool
import com.l0gic0l.glados.dsp.NoteOps
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.ui.components.ParamControl
import com.l0gic0l.glados.ui.components.rememberFrameTick
import com.l0gic0l.glados.ui.editor.PianoRoll
import com.l0gic0l.glados.ui.editor.RollControls
import kotlin.math.roundToInt

@Composable
fun EditorScreen(vm: MainViewModel, goSpeak: () -> Unit) {
    val context = LocalContext.current
    val playing by vm.player.playing.collectAsStateWithLifecycle()
    val params by vm.params.collectAsStateWithLifecycle()
    var loop by remember { mutableStateOf(vm.player.loop) }
    val controls = remember { RollControls() }
    val tick by rememberFrameTick(playing)

    val open = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { it?.let(vm::importFile) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("audio/wav")) { it?.let(vm::exportWav) }
    val mic = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) vm.toggleRecord() else vm.message = "Microphone permission is needed to record."
    }
    fun record() {
        if (vm.recording || ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            vm.toggleRecord()
        } else {
            mic.launch(Manifest.permission.RECORD_AUDIO)
        }
    }

    val clip = vm.clip
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        // Transport and file actions.
        Row(Modifier.horizontalScroll(rememberScrollState()), verticalAlignment = Alignment.CenterVertically) {
            IconButton(
                onClick = ::record,
                enabled = vm.recording || vm.busy == null,
                colors = if (vm.recording) IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.error) else IconButtonDefaults.iconButtonColors(),
            ) { Icon(if (vm.recording) Icons.Filled.Stop else Icons.Filled.Mic, contentDescription = if (vm.recording) "Stop recording" else "Record") }
            IconButton(onClick = { open.launch(arrayOf("audio/*")) }, enabled = vm.busy == null && !vm.recording) {
                Icon(Icons.Filled.FileOpen, contentDescription = "Import audio")
            }
            IconButton(onClick = vm::togglePlay, enabled = clip != null) {
                Icon(if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow, contentDescription = if (playing) "Pause" else "Play")
            }
            IconButton(onClick = { vm.player.stop() }, enabled = clip != null) { Icon(Icons.Filled.Stop, contentDescription = "Stop") }
            IconButton(
                onClick = { loop = !loop; vm.player.loop = loop },
                colors = if (loop) IconButtonDefaults.filledTonalIconButtonColors() else IconButtonDefaults.iconButtonColors(),
            ) { Icon(Icons.Filled.Repeat, contentDescription = "Loop") }
            IconButton(onClick = vm::undo, enabled = vm.canUndo) { Icon(Icons.AutoMirrored.Filled.Undo, contentDescription = "Undo") }
            IconButton(onClick = vm::redo, enabled = vm.canRedo) { Icon(Icons.AutoMirrored.Filled.Redo, contentDescription = "Redo") }
            IconButton(onClick = { save.launch("glados.wav") }, enabled = clip != null && vm.busy == null) { Icon(Icons.Filled.Save, contentDescription = "Save as WAV") }
        }

        if (clip == null) {
            EmptyEditor(onRecord = ::record, onImport = { open.launch(arrayOf("audio/*")) }, onSpeak = goSpeak, recording = vm.recording)
            return@Column
        }

        @Suppress("UNUSED_VARIABLE") val redraw = tick
        val pos = vm.player.positionSeconds
        Text(
            "${clip.title}: ${formatClock(pos)} / ${formatClock(vm.player.durationSeconds)}   ${vm.edit?.notes?.size ?: 0} notes",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        // Tools.
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            FilterChip(selected = vm.tool == Tool.PITCH, onClick = { vm.tool = Tool.PITCH }, label = { Text("Pitch") })
            FilterChip(selected = vm.tool == Tool.TIME, onClick = { vm.tool = Tool.TIME }, label = { Text("Timing") })
            FilterChip(selected = vm.tool == Tool.SPLIT, onClick = { vm.tool = Tool.SPLIT }, label = { Text("Split") })
            FilterChip(selected = vm.snapToScale, onClick = { vm.snapToScale = !vm.snapToScale }, label = { Text("Snap to scale") })
            AssistChip(onClick = vm::mergeSelected, label = { Text("Join") })
            AssistChip(onClick = vm::selectAll, label = { Text("Select all") })
            IconButton(onClick = { controls.zoomTime(0.7f) }) { Icon(Icons.Filled.ZoomOut, contentDescription = "Zoom out time") }
            IconButton(onClick = { controls.zoomTime(1.4f) }) { Icon(Icons.Filled.ZoomIn, contentDescription = "Zoom in time") }
            TextButton(onClick = { controls.zoomPitch(0.8f) }) { Text("Rows -") }
            TextButton(onClick = { controls.zoomPitch(1.25f) }) { Text("Rows +") }
            TextButton(onClick = { controls.fit() }) { Text("Fit") }
        }
        Text(
            when (vm.tool) {
                Tool.PITCH -> "Tap notes to select. Drag a note up or down to change its pitch. Pinch to zoom, drag empty space to scroll."
                Tool.TIME -> "Drag a note's edge to stretch it, or its middle to slide it. Tap the ruler to move the playhead."
                Tool.SPLIT -> "Tap inside a note to cut it in two."
            },
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        ElevatedCard(Modifier.fillMaxWidth().height(340.dp)) {
            PianoRoll(vm, controls, Modifier.fillMaxSize())
        }

        PresetPicker(vm)
        SelectedNotes(vm, params)
        CorrectionCard(vm)
        DetectionCard(vm, params)
    }
}

@Composable
private fun EmptyEditor(onRecord: () -> Unit, onImport: () -> Unit, onSpeak: () -> Unit, recording: Boolean) {
    ElevatedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Note editor", style = MaterialTheme.typography.titleLarge)
            Text(
                "Every syllable becomes a note you can drag to any pitch, stretch, retune to a scale, or push toward GLaDOS. " +
                    "Start by recording your voice, importing an audio file, or generating speech.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Button(onClick = onRecord, modifier = Modifier.fillMaxWidth()) {
                Icon(if (recording) Icons.Filled.Stop else Icons.Filled.Mic, contentDescription = null)
                Text(if (recording) "Stop recording" else "Record", modifier = Modifier.padding(start = 8.dp))
            }
            FilledTonalButton(onClick = onImport, modifier = Modifier.fillMaxWidth()) { Text("Import audio file") }
            OutlinedButton(onClick = onSpeak, modifier = Modifier.fillMaxWidth()) { Text("Generate speech") }
        }
    }
}

@Composable
private fun SelectedNotes(vm: MainViewModel, params: com.l0gic0l.glados.model.VoiceParams) {
    val edit = vm.edit ?: return
    @Suppress("UNUSED_VARIABLE") val version = vm.editVersion
    val an = vm.clip?.analysis ?: return
    val selected = vm.selection.sorted().mapNotNull { edit.notes.getOrNull(it) }
    val shift = NoteOps.globalShift(an, params)

    ElevatedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            if (selected.isEmpty()) {
                Text("Selected notes", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Select one or more notes to adjust them precisely. Corrections below apply to everything when nothing is selected.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                return@Column
            }
            val n = selected.first()
            val final = n.center + n.offset + shift
            Text(
                "${selected.size} selected: ${Scales.noteName(final.roundToInt())} (${"%+.0f".format((final - final.roundToInt()) * 100)} cents)",
                style = MaterialTheme.typography.titleMedium,
            )
            LabeledSlider("Pitch offset", n.offset.toFloat(), -12f..12f, "%+.2f st") { v ->
                vm.mutate("offset") { _, _ -> selected.forEach { it.offset = v.toDouble() } }
            }
            LabeledSlider("Pitch drift", n.drift * 100f, 0f..200f, "%.0f %%") { v ->
                vm.mutate("drift") { _, _ -> selected.forEach { it.drift = v / 100f } }
            }
            LabeledSlider("Vibrato and modulation", n.mod * 100f, 0f..300f, "%.0f %%") { v ->
                vm.mutate("mod") { _, _ -> selected.forEach { it.mod = v / 100f } }
            }
            LabeledSlider("Formant", n.formant, -12f..12f, "%+.1f st") { v ->
                vm.mutate("formant") { _, _ -> selected.forEach { it.formant = v } }
            }
            LabeledSlider("Level", n.gainDb, -24f..12f, "%+.1f dB") { v ->
                vm.mutate("gain") { _, _ -> selected.forEach { it.gainDb = v } }
            }
        }
    }
}

@Composable
private fun CorrectionCard(vm: MainViewModel) {
    var center by remember { mutableFloatStateOf(100f) }
    var drift by remember { mutableFloatStateOf(100f) }
    var mod by remember { mutableFloatStateOf(100f) }
    val scope = if (vm.selection.isEmpty()) "all notes" else "${vm.selection.size} selected"
    ElevatedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text("Correct pitch", style = MaterialTheme.typography.titleMedium)
            Text(
                "Uses the key and scale from the Voice tab. Applies to $scope.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            LabeledSlider("Pitch center", center, 0f..100f, "%.0f %%") { center = it }
            LabeledSlider("Remove drift", drift, 0f..100f, "%.0f %%") { drift = it }
            LabeledSlider("Remove modulation", mod, 0f..100f, "%.0f %%") { mod = it }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.horizontalScroll(rememberScrollState())) {
                Button(onClick = { vm.applyCorrection(center / 100.0, drift / 100.0, mod / 100.0) }) { Text("Apply") }
                FilledTonalButton(onClick = vm::applyVoiceTuning) { Text("Use Voice tab tuning") }
                OutlinedButton(onClick = vm::resetNotes) { Text("Reset notes") }
                OutlinedButton(onClick = vm::resetTiming) { Text("Reset timing") }
            }
        }
    }
}

@Composable
private fun DetectionCard(vm: MainViewModel, params: com.l0gic0l.glados.model.VoiceParams) {
    val clip = vm.clip ?: return
    val an = clip.analysis
    val voicedPercent = (100.0 * an.voiced.count { it } / an.frames.coerceAtLeast(1)).roundToInt()
    ElevatedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text("Clean-up and detection", style = MaterialTheme.typography.titleMedium)
            Text(
                "Found ${vm.edit?.notes?.size ?: 0} notes. $voicedPercent% of the clip is pitched voice.",
                style = MaterialTheme.typography.bodyMedium,
            )
            if (voicedPercent < 20) {
                Text(
                    "Very little pitched voice was found. Raise Voicing sensitivity, widen the pitch range, or check that the recording is speech.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            if (vm.analysisStale) {
                Text(
                    "Detection settings changed after you made edits. Tap Re-analyse to apply them (edits will be lost).",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.tertiary,
                )
            }
            ParamControl(Specs.DENOISE, params, vm::setParam)
            if (!clip.noisy) {
                Text(
                    "Generated speech is already clean, so noise reduction is skipped for it.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            ParamControl(Specs.VOICING, params, vm::setParam)
            ParamControl(Specs.FMIN, params, vm::setParam)
            ParamControl(Specs.FMAX, params, vm::setParam)
            Text(
                "Voicing and the pitch range re-analyse the clip automatically while you have not edited it yet.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            LabeledSlider("Pitch jump that starts a new note", vm.segTolerance, 30f..300f, "%.0f cents") { vm.segTolerance = it }
            LabeledSlider("Syllable splitting", vm.segSensitivity, 0f..100f, "%.0f %%") { vm.segSensitivity = it }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = vm::resegment) { Text("Re-detect notes") }
                OutlinedButton(onClick = vm::reanalyse) { Text("Re-analyse") }
            }
        }
    }
}

@Composable
private fun LabeledSlider(label: String, value: Float, range: ClosedFloatingPointRange<Float>, format: String, onChange: (Float) -> Unit) {
    Column {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(label, style = MaterialTheme.typography.bodyMedium)
            Text(format.format(value), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
        }
        Slider(value = value.coerceIn(range), onValueChange = onChange, valueRange = range)
    }
}

private fun formatClock(sec: Float): String {
    val m = (sec / 60).toInt()
    return "%d:%04.1f".format(m, sec - m * 60)
}

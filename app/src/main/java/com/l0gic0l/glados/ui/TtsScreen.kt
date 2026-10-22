package com.l0gic0l.glados.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.Button
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.ui.components.SelectField

@Composable
fun TtsScreen(vm: MainViewModel, openEditor: () -> Unit) {
    var text by rememberSaveable {
        mutableStateOf("Hello, and, again, welcome to the Aperture Science Computer-Aided Enrichment Center.")
    }
    val ready by vm.tts.ready.collectAsStateWithLifecycle()
    val voices by vm.tts.voices.collectAsStateWithLifecycle()
    val error by vm.tts.error.collectAsStateWithLifecycle()
    val playing by vm.player.playing.collectAsStateWithLifecycle()
    var voiceIndex by rememberSaveable { mutableIntStateOf(0) }
    var rate by rememberSaveable { mutableFloatStateOf(1f) }
    var pitch by rememberSaveable { mutableFloatStateOf(1f) }
    var autoTune by rememberSaveable { mutableStateOf(true) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("audio/wav")) { uri ->
        uri?.let(vm::exportWav)
    }

    val voiceLabels = listOf("Engine default") + voices.map { it.label }
    val hasClip = vm.clip != null

    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            label = { Text("What should GLaDOS say?") },
            minLines = 4,
            modifier = Modifier.fillMaxWidth(),
        )
        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error)
            TextButton(onClick = { vm.tts.retry() }) { Text("Try again") }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = {
                    vm.speak(
                        text, voices.getOrNull(voiceIndex - 1)?.name, rate, pitch, autoTune,
                    )
                },
                enabled = ready && vm.busy == null && text.isNotBlank(),
                modifier = Modifier.weight(1f).testTag("speak-button"),
            ) {
                Icon(Icons.Filled.PlayArrow, contentDescription = null)
                Text("Speak", modifier = Modifier.padding(start = 8.dp))
            }
            OutlinedButton(onClick = { vm.player.stop() }, enabled = playing) {
                Icon(Icons.Filled.Stop, contentDescription = "Stop")
            }
            OutlinedButton(onClick = { save.launch("glados.wav") }, enabled = hasClip && vm.busy == null) {
                Icon(Icons.Filled.Save, contentDescription = "Save as WAV")
            }
        }

        PresetPicker(vm)

        ElevatedCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                SelectField("Voice", voiceLabels.getOrElse(voiceIndex) { "Engine default" }, voiceLabels, { voiceIndex = it })
                Text("Speaking rate: ${"%.2f".format(rate)}x", style = MaterialTheme.typography.bodyLarge)
                Slider(value = rate, onValueChange = { rate = it }, valueRange = 0.5f..2f)
                Text("Base pitch: ${"%.2f".format(pitch)}x", style = MaterialTheme.typography.bodyLarge)
                Slider(value = pitch, onValueChange = { pitch = it }, valueRange = 0.5f..2f)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Tune speech to the scale", style = MaterialTheme.typography.bodyLarge)
                        Text(
                            "Snaps each syllable to the key and scale from the Tuning settings. Turn off to keep the speech's own melody.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Switch(checked = autoTune, onCheckedChange = { autoTune = it })
                }
            }
        }

        if (hasClip) {
            TextButton(onClick = openEditor) { Text("Open in the note editor to sing it or fine-tune every syllable") }
        }
        Text(
            "Speech is generated by the voice engine built into Android, then pitch-corrected and run through the effects from the Voice tab.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

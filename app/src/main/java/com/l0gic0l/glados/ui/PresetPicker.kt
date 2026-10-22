package com.l0gic0l.glados.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.LibraryMusic
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.model.Presets

/** A card naming the current preset. Tapping it opens the whole library, grouped by category. */
@Composable
fun PresetPicker(vm: MainViewModel, modifier: Modifier = Modifier) {
    val params by vm.params.collectAsStateWithLifecycle()
    val userPresets by vm.userPresets.collectAsStateWithLifecycle()
    var open by rememberSaveable { mutableStateOf(false) }
    val current = remember(params, userPresets) { vm.presetName(params) }
    val blurb = Presets.all.firstOrNull { it.name == current }?.blurb

    ElevatedCard(onClick = { open = true }, modifier = modifier.fillMaxWidth().testTag("preset-picker")) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Preset", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(current ?: "Custom settings", style = MaterialTheme.typography.titleMedium)
                Text(
                    blurb ?: if (current == null) "Tap to choose from ${Presets.all.size} voices." else "One of your saved presets.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Icon(Icons.Filled.LibraryMusic, contentDescription = "Choose a preset")
        }
    }
    if (open) PresetLibraryDialog(vm, current, userPresets.keys.toList(), onDismiss = { open = false })
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PresetLibraryDialog(vm: MainViewModel, current: String?, userNames: List<String>, onDismiss: () -> Unit) {
    var saving by remember { mutableStateOf(false) }
    val playing by vm.player.playing.collectAsStateWithLifecycle()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Presets", modifier = Modifier.weight(1f))
                if (vm.clip != null) {
                    IconButton(onClick = vm::togglePlay) {
                        Icon(if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow, contentDescription = if (playing) "Pause" else "Play")
                    }
                }
            }
        },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    Presets.all.firstOrNull { it.name == current }?.blurb ?: "Tap a preset to hear it. Your current clip keeps playing.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                for ((category, presets) in Presets.byCategory()) {
                    Text(category.title, style = MaterialTheme.typography.titleSmall)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (p in presets) {
                            FilterChip(selected = p.name == current, onClick = { vm.applyPreset(p.name) }, label = { Text(p.name) })
                        }
                    }
                }
                Text("Your presets", style = MaterialTheme.typography.titleSmall)
                if (userNames.isEmpty()) {
                    Text(
                        "None yet. Adjust a voice, then tap Save current.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (name in userNames) {
                        InputChip(
                            selected = name == current,
                            onClick = { vm.applyPreset(name) },
                            label = { Text(name) },
                            trailingIcon = {
                                Icon(
                                    Icons.Filled.Close, contentDescription = "Delete preset $name",
                                    modifier = Modifier.clickable { vm.deleteUserPreset(name) }.padding(2.dp),
                                )
                            },
                        )
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Done") } },
        dismissButton = { TextButton(onClick = { saving = true }) { Text("Save current...") } },
    )
    if (saving) SavePresetDialog(onDismiss = { saving = false }, onSave = { vm.saveUserPreset(it); saving = false })
}

@Composable
private fun SavePresetDialog(onDismiss: () -> Unit, onSave: (String) -> Unit) {
    var name by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Save preset") },
        text = {
            OutlinedTextField(value = name, onValueChange = { name = it.take(32) }, label = { Text("Name") }, singleLine = true)
        },
        confirmButton = { TextButton(onClick = { onSave(name) }, enabled = name.isNotBlank()) { Text("Save") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

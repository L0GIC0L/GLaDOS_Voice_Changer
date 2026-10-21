package com.l0gic0l.glados.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.InputChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.model.Group
import com.l0gic0l.glados.model.Presets
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.ui.components.CustomScaleChips
import com.l0gic0l.glados.ui.components.ParamControl
import kotlin.math.roundToInt

/** Every setting in the app, grouped, with presets on top. */
@Composable
fun VoiceScreen(vm: MainViewModel) {
    val params by vm.params.collectAsStateWithLifecycle()

    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Presets", style = MaterialTheme.typography.titleMedium)
        PresetChips(vm)
        Text(
            "Double-tap a slider's name to reset it. Settings are saved automatically.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        for (group in Group.entries) {
            GroupCard(group, vm, params)
        }
    }
}

/** Built-in and saved presets as a row of chips, plus a button to save the current settings. */
@Composable
fun PresetChips(vm: MainViewModel, modifier: Modifier = Modifier) {
    val userPresets by vm.userPresets.collectAsStateWithLifecycle()
    var saving by remember { mutableStateOf(false) }
    LazyRow(modifier, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(Presets.all) { p ->
            FilterChip(selected = false, onClick = { vm.applyPreset(p.name) }, label = { Text(p.name) })
        }
        items(userPresets.keys.toList()) { name ->
            InputChip(
                selected = false,
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
        item { AssistChip(onClick = { saving = true }, label = { Text("Save current...") }) }
    }
    if (saving) SavePresetDialog(onDismiss = { saving = false }, onSave = { vm.saveUserPreset(it); saving = false })
}

@Composable
private fun GroupCard(group: Group, vm: MainViewModel, params: com.l0gic0l.glados.model.VoiceParams) {
    var open by rememberSaveable(group.name) { mutableStateOf(group == Group.TUNING) }
    ElevatedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            Row(
                Modifier.fillMaxWidth().clickable { open = !open }.padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(group.title, style = MaterialTheme.typography.titleMedium)
                    Text(group.blurb, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Icon(if (open) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore, contentDescription = if (open) "Collapse" else "Expand")
            }
            AnimatedVisibility(open) {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    for (spec in Specs.inGroup(group)) {
                        ParamControl(spec, params, vm::setParam)
                        if (spec == Specs.SCALE && params.int(Specs.SCALE) == Scales.CUSTOM) {
                            CustomScaleChips(
                                mask = params[Specs.CUSTOM_MASK].roundToInt(),
                                onChange = { vm.setParam(Specs.CUSTOM_MASK, it.toFloat()) },
                            )
                        }
                    }
                    TextButton(onClick = { vm.resetParams(Specs.inGroup(group)) }) { Text("Reset ${group.title.lowercase()}") }
                }
            }
        }
    }
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

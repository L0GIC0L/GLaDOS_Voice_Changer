package com.l0gic0l.glados.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.model.Group
import com.l0gic0l.glados.model.Macros
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.ui.components.CustomScaleChips
import com.l0gic0l.glados.ui.components.MacroControl
import com.l0gic0l.glados.ui.components.ParamControl
import kotlin.math.roundToInt

/** Presets on top, then either a few combined sliders (Simple) or every setting, grouped (Advanced). */
@Composable
fun VoiceScreen(vm: MainViewModel) {
    val params by vm.params.collectAsStateWithLifecycle()
    val simple by vm.simpleMode.collectAsStateWithLifecycle()

    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        PresetPicker(vm)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            SegmentedButton(
                selected = simple,
                onClick = { vm.setSimpleMode(true) },
                shape = SegmentedButtonDefaults.itemShape(0, 2),
                modifier = Modifier.testTag("mode-simple"),
            ) { Text("Simple") }
            SegmentedButton(
                selected = !simple,
                onClick = { vm.setSimpleMode(false) },
                shape = SegmentedButtonDefaults.itemShape(1, 2),
                modifier = Modifier.testTag("mode-advanced"),
            ) { Text("Advanced") }
        }

        if (simple) {
            SimpleCard(vm, params)
        } else {
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
}

@Composable
private fun SimpleCard(vm: MainViewModel, params: com.l0gic0l.glados.model.VoiceParams) {
    ElevatedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Quick controls", style = MaterialTheme.typography.titleMedium)
            Text(
                "Pick a preset, then season it here. Most sliders move several settings at once; Advanced shows them all.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(bottom = 8.dp),
            )
            ParamControl(Specs.SHIFT, params, vm::setParam)
            ParamControl(Specs.FORMANT, params, vm::setParam)
            for (m in Macros.all) MacroControl(m, params, onChange = { vm.setMacro(m, it) })
            ParamControl(Specs.OUT_DB, params, vm::setParam)
        }
    }
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

package com.l0gic0l.glados.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
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
import com.l0gic0l.glados.audio.AudioDevices
import com.l0gic0l.glados.dsp.Pitch
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.ui.components.ParamControl
import com.l0gic0l.glados.ui.components.PitchTraceView
import com.l0gic0l.glados.ui.components.SelectField
import com.l0gic0l.glados.ui.components.rememberFrameTick
import kotlin.math.roundToInt
import kotlin.math.sqrt

@Composable
fun LiveScreen(vm: MainViewModel) {
    val context = LocalContext.current
    val state by vm.live.state.collectAsStateWithLifecycle()
    val params by vm.params.collectAsStateWithLifecycle()
    var denied by remember { mutableStateOf(false) }
    val tick by rememberFrameTick(state.running)
    // The controller can change these on its own, for example muting when headphones are unplugged.
    val bypass by vm.live.bypassState.collectAsStateWithLifecycle()
    val muted by vm.live.mutedState.collectAsStateWithLifecycle()

    val permissions = buildList {
        add(Manifest.permission.RECORD_AUDIO)
        if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
    }.toTypedArray()
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        if (result[Manifest.permission.RECORD_AUDIO] == true) {
            denied = false
            vm.live.start()
        } else {
            denied = true
        }
    }

    fun toggle() {
        if (state.running) {
            vm.live.stop()
        } else if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            vm.live.start()
        } else {
            launcher.launch(permissions)
        }
    }

    val inputs = remember(state.running) { AudioDevices.inputs(context) }
    val outputs = remember(state.running) { AudioDevices.outputs(context) }
    // The chosen devices live in the controller, so the pickers keep their selection across tab changes and rotation.
    var inId by remember { mutableIntStateOf(vm.live.inputDeviceId) }
    var outId by remember { mutableIntStateOf(vm.live.outputDeviceId) }
    val inIndex = inputs.indexOfFirst { it.id == inId }.coerceAtLeast(0)
    val outIndex = outputs.indexOfFirst { it.id == outId }.coerceAtLeast(0)

    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ExtendedFloatingActionButton(
            onClick = ::toggle,
            icon = { Icon(if (state.running) Icons.Filled.Stop else Icons.Filled.Mic, contentDescription = null) },
            text = { Text(if (state.running) "Stop" else "Go live") },
            modifier = Modifier.fillMaxWidth(),
        )
        when {
            denied -> Text(
                "Microphone permission is needed. Allow it in Android settings, then try again.",
                color = MaterialTheme.colorScheme.error,
            )
            state.error != null -> Text(state.error!!, color = MaterialTheme.colorScheme.error)
            state.running && state.notice != null -> Text(state.notice!!, color = MaterialTheme.colorScheme.error)
            state.running -> Text(
                "Live at ${state.sampleRate / 1000.0} kHz, about ${state.latencyMs} ms delay. Use headphones to avoid feedback.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            else -> Text(
                "Speak into the microphone and hear the GLaDOS voice. Headphones recommended.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        ElevatedCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                val mask = Scales.mask(params.int(Specs.SCALE), params.int(Specs.KEY), params[Specs.CUSTOM_MASK].roundToInt())
                PitchTraceView(vm.live.trace, mask, params[Specs.REF_A4].toDouble(), state.running)
                val target = vm.live.targetMidi
                @Suppress("UNUSED_VARIABLE") val redraw = tick
                val outLevel = vm.live.outputLevel
                val inLevel = vm.live.inputLevel
                Text(
                    if (state.running && target != Int.MIN_VALUE) "Target note ${Scales.noteName(target)} (${"%.0f".format(Pitch.midiToHz(target.toDouble(), params[Specs.REF_A4].toDouble()))} Hz)"
                    else "Pitch trace appears here while live",
                    style = MaterialTheme.typography.labelLarge,
                )
                if (state.running) {
                    val detected = vm.live.voiceDetected
                    Text(
                        if (detected) "Voice detected" else "Listening...",
                        style = MaterialTheme.typography.labelLarge,
                        color = if (detected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (vm.live.noVoiceWarning) {
                        Text(
                            "The mic is hearing sound but no pitched voice. Speak closer, raise Voicing sensitivity or lower " +
                                "Lowest pitch in the Voice tab, and try a quieter room.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                }
                Meter("Mic", inLevel)
                Meter("Output", outLevel)
            }
        }

        ElevatedCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SwitchLine("Hear original voice (bypass)", bypass) { vm.live.bypass = it }
                SwitchLine("Mute output", muted) { vm.live.muted = it }
                SelectField(
                    "Microphone", inputs.getOrNull(inIndex)?.label ?: "Default", inputs.map { it.label },
                    { inId = inputs[it].id; vm.live.inputDeviceId = inputs[it].id },
                )
                SelectField(
                    "Output", outputs.getOrNull(outIndex)?.label ?: "Default", outputs.map { it.label },
                    { outId = outputs[it].id; vm.live.outputDeviceId = outputs[it].id },
                )
            }
        }

        ElevatedCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("Quick controls", style = MaterialTheme.typography.titleMedium)
                for (spec in listOf(Specs.KEY, Specs.SCALE, Specs.RETUNE, Specs.STRENGTH, Specs.SHIFT, Specs.FORMANT)) {
                    ParamControl(spec, params, vm::setParam)
                }
                Text(
                    "Everything else, including presets, is in the Voice tab.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun Meter(label: String, level: Float) {
    // Peak sample amplitude, shown on a square-root scale so quiet speech still moves the bar.
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, style = MaterialTheme.typography.labelMedium, modifier = Modifier.size(width = 52.dp, height = 20.dp))
        LinearProgressIndicator(progress = { sqrt(level.coerceIn(0f, 1f)) }, modifier = Modifier.weight(1f))
    }
}

@Composable
private fun SwitchLine(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

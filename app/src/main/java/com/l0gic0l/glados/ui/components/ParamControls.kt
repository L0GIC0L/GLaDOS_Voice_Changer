package com.l0gic0l.glados.ui.components

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.model.ParamSpec
import com.l0gic0l.glados.model.VoiceParams

/** Renders the right control for a parameter: switch, dropdown or slider. Double-tap a slider's label to reset it. */
@Composable
fun ParamControl(
    spec: ParamSpec,
    params: VoiceParams,
    onChange: (ParamSpec, Float) -> Unit,
    modifier: Modifier = Modifier,
) {
    val value = params[spec]
    when {
        spec.toggle -> Column(modifier) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(spec.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                Switch(checked = value >= 0.5f, onCheckedChange = { onChange(spec, if (it) 1f else 0f) })
            }
            HelpText(spec)
        }
        spec.isChoice -> SelectField(
            label = spec.label,
            selected = spec.format(value),
            options = spec.choices!!,
            onSelect = { onChange(spec, it.toFloat()) },
            modifier = modifier.fillMaxWidth(),
        )
        else -> Column(modifier) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .pointerInput(spec) { detectTapGestures(onDoubleTap = { onChange(spec, spec.default) }) },
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(spec.label, style = MaterialTheme.typography.bodyLarge)
                Text(
                    spec.format(value),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            Slider(
                value = value,
                onValueChange = { onChange(spec, spec.clamp(it)) },
                valueRange = spec.min..spec.max,
            )
            HelpText(spec)
        }
    }
}

@Composable
private fun HelpText(spec: ParamSpec) {
    if (spec.help.isNotEmpty()) {
        Text(
            spec.help,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = 4.dp),
        )
    }
}

/** Twelve note chips for building a custom scale. */
@Composable
fun CustomScaleChips(mask: Int, onChange: (Int) -> Unit, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (row in 0..1) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                for (i in row * 6 until row * 6 + 6) {
                    val on = (mask shr i) and 1 == 1
                    FilterChip(
                        selected = on,
                        onClick = { onChange(mask xor (1 shl i)) },
                        label = { Text(Scales.noteNames[i], maxLines = 1) },
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
    }
}

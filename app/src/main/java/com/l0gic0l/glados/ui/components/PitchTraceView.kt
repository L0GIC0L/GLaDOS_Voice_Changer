package com.l0gic0l.glados.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.l0gic0l.glados.audio.PitchTrace
import com.l0gic0l.glados.dsp.Pitch
import com.l0gic0l.glados.dsp.Scales
import kotlin.math.floor

/**
 * Scrolling pitch view in the style of a tuner: grey dots are the detected voice, the bright line is
 * the corrected pitch. Shaded rows are notes the scale allows.
 */
@Composable
fun PitchTraceView(trace: PitchTrace, scaleMask: Int, a4: Double, active: Boolean, modifier: Modifier = Modifier) {
    val tick by rememberFrameTick(active)
    val inHz = remember { FloatArray(trace.size) }
    val outHz = remember { FloatArray(trace.size) }
    val measurer = rememberTextMeasurer()
    val colors = MaterialTheme.colorScheme
    val rowAllowed = colors.primary.copy(alpha = 0.10f)
    val rowBlack = colors.onSurface.copy(alpha = 0.07f)
    val lane = colors.outlineVariant.copy(alpha = 0.35f)
    val labelStyle = TextStyle(fontSize = 10.sp, color = colors.onSurfaceVariant)
    val center = remember { doubleArrayOf(57.0) }

    Canvas(modifier.fillMaxWidth().height(200.dp).clipToBounds()) {
        @Suppress("UNUSED_VARIABLE") val redraw = tick // reading it invalidates the canvas every frame
        trace.snapshot(inHz, outHz)
        val rows = 19
        // Follow the voice: centre the view on the recent corrected pitch.
        var sum = 0.0
        var n = 0
        for (i in trace.size - 60 until trace.size) if (outHz[i] > 0f) { sum += Pitch.hzToMidi(outHz[i].toDouble(), a4); n++ }
        if (n > 5) center[0] += (sum / n - center[0]) * 0.08
        val top = center[0] + rows / 2.0
        val rowH = size.height / rows
        val gutter = 28.dp.toPx()
        val w = size.width - gutter

        val chromatic = mask12(scaleMask)
        val firstNote = floor(center[0] - rows / 2.0).toInt() - 1
        for (m in firstNote..firstNote + rows + 2) {
            val y = ((top - m - 0.5) * rowH).toFloat()
            val allowed = Scales.isAllowed(scaleMask, m)
            val black = Math.floorMod(m, 12) in intArrayOf(1, 3, 6, 8, 10)
            // With a restricted scale, tint the allowed notes. In chromatic mode every row is allowed, so
            // shade the black keys instead to keep the piano-roll structure visible.
            if (!chromatic && allowed) drawRect(rowAllowed, Offset(gutter, y), Size(w, rowH))
            else if (black) drawRect(rowBlack, Offset(gutter, y), Size(w, rowH))
            drawLine(lane, Offset(gutter, y + rowH), Offset(gutter + w, y + rowH), 1f)
            val natural = !black
            if (allowed && (Math.floorMod(m, 12) == 0 || (rowH > 11.dp.toPx() && natural))) {
                drawText(measurer, Scales.noteName(m), Offset(2.dp.toPx(), y + rowH / 2 - 7.dp.toPx()), labelStyle)
            }
        }

        fun x(i: Int) = gutter + w * i / (trace.size - 1)
        fun y(hz: Float) = ((top - Pitch.hzToMidi(hz.toDouble(), a4)) * rowH).toFloat()
        for (i in 0 until trace.size) {
            if (inHz[i] > 0f) drawCircle(colors.onSurfaceVariant.copy(alpha = 0.45f), 1.6.dp.toPx(), Offset(x(i), y(inHz[i])))
        }
        for (i in 1 until trace.size) {
            if (outHz[i] > 0f && outHz[i - 1] > 0f) {
                drawLine(colors.primary, Offset(x(i - 1), y(outHz[i - 1])), Offset(x(i), y(outHz[i])), strokeWidth = 3.dp.toPx())
            }
        }
        val last = outHz[trace.size - 1]
        if (last > 0f) drawCircle(colors.tertiary, 5.dp.toPx(), Offset(x(trace.size - 1), y(last)))
    }
}

private fun mask12(mask: Int): Boolean = mask and Scales.CHROMATIC_MASK == Scales.CHROMATIC_MASK

package com.l0gic0l.glados.ui.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.Tool
import com.l0gic0l.glados.dsp.Analysis
import com.l0gic0l.glados.dsp.Edit
import com.l0gic0l.glados.dsp.Note
import com.l0gic0l.glados.dsp.NoteOps
import com.l0gic0l.glados.dsp.Scales
import com.l0gic0l.glados.dsp.TimeMap
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.ui.components.rememberFrameTick
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** Visible window onto the clip: horizontal scale and scroll in seconds, vertical scale and top edge in MIDI notes. */
data class RollView(val pxPerSec: Float, val scrollSec: Float, val pxPerSemi: Float, val topMidi: Float)

/** Hooks so toolbar buttons outside the canvas can zoom it. */
class RollControls {
    /** Centre of a note on screen in canvas pixels, or null if it is off screen. Set by the roll; used by UI tests. */
    var noteCenter: (Int) -> Offset? = { null }
    var zoomPitch: (Float) -> Unit = {}
    var zoomTime: (Float) -> Unit = {}
    var fit: () -> Unit = {}
}

private class Geo(val view: RollView, val gutter: Float, val ruler: Float) {
    fun x(sec: Double): Float = gutter + ((sec - view.scrollSec) * view.pxPerSec).toFloat()
    fun sec(x: Float): Double = view.scrollSec + (x - gutter) / view.pxPerSec.toDouble()
    fun y(midi: Double): Float = ruler + ((view.topMidi - midi) * view.pxPerSemi).toFloat()
    fun midi(y: Float): Double = view.topMidi - (y - ruler) / view.pxPerSemi.toDouble()
}

private class NoteBox(val index: Int, val note: Note, val x0: Float, val x1: Float)

private enum class Drag { NONE, PAN, SEEK, PITCH, TIME_LEFT, TIME_RIGHT, TIME_BODY, ZOOM }

@Composable
fun PianoRoll(vm: MainViewModel, controls: RollControls, modifier: Modifier = Modifier) {
    val clip = vm.clip
    val edit = vm.edit
    if (clip == null || edit == null) return

    val an = clip.analysis
    val sr = an.sampleRate
    val params by vm.params.collectAsStateWithLifecycle()
    val playing by vm.player.playing.collectAsStateWithLifecycle()
    val tick by rememberFrameTick(playing)
    val density = androidx.compose.ui.platform.LocalDensity.current
    val dp1 = density.density
    val gutter = with(density) { 40.dp.toPx() }
    val ruler = with(density) { 22.dp.toPx() }
    val edgeZone = with(density) { 22.dp.toPx() }
    val colors = MaterialTheme.colorScheme
    val measurer = rememberTextMeasurer()

    var boxSize by remember { mutableStateOf(IntSize.Zero) }
    var view by remember(clip) { mutableStateOf<RollView?>(null) }
    var hint by remember { mutableStateOf<String?>(null) }

    val shift = NoteOps.globalShift(an, params)
    val mask = Scales.mask(params.int(Specs.SCALE), params.int(Specs.KEY), params[Specs.CUSTOM_MASK].roundToInt())

    val editState = rememberUpdatedState(edit)
    val viewState = rememberUpdatedState(view)
    val toolState = rememberUpdatedState(vm.tool)
    val snapState = rememberUpdatedState(vm.snapToScale)
    val maskState = rememberUpdatedState(mask)
    val shiftState = rememberUpdatedState(shift)
    val selectionState = rememberUpdatedState(vm.selection)

    LaunchedEffect(clip, boxSize) {
        if (view == null && boxSize.width > 0) view = fitView(clip.analysis, edit, shift, boxSize, gutter, ruler, dp1)
    }
    controls.fit = { if (boxSize.width > 0) view = fitView(an, vm.edit ?: edit, shift, boxSize, gutter, ruler, dp1) }
    controls.noteCenter = { i ->
        val v = view
        val n = edit.notes.getOrNull(i)
        if (v == null || n == null) {
            null
        } else {
            val g = Geo(v, gutter, ruler)
            val tm = edit.timeMap(an.length)
            val mid = (n.f0 + n.f1) / 2
            Offset(g.x(tm.toOutput((mid * an.hop).toDouble()) / sr), g.y(n.pitchAt(mid.toDouble()) + shift))
        }
    }
    controls.zoomPitch = { f ->
        view = view?.let { v ->
            val centre = v.topMidi - (boxSize.height - ruler) / 2 / v.pxPerSemi
            val ps = (v.pxPerSemi * f).coerceIn(6f * dp1, 60f * dp1)
            v.copy(pxPerSemi = ps, topMidi = centre.toFloat() + (boxSize.height - ruler) / 2 / ps)
        }
    }
    controls.zoomTime = { f ->
        view = view?.let { v ->
            val centreSec = v.scrollSec + (boxSize.width - gutter) / 2 / v.pxPerSec
            val pps = (v.pxPerSec * f).coerceIn(8f, 4000f)
            v.copy(pxPerSec = pps, scrollSec = centreSec - (boxSize.width - gutter) / 2 / pps)
        }
    }

    // Keep the playhead on screen while playing.
    LaunchedEffect(playing) {
        while (playing) {
            androidx.compose.runtime.withFrameNanos { }
            val v = view ?: continue
            val x = gutter + (vm.playheadSeconds - v.scrollSec) * v.pxPerSec
            if (x > boxSize.width - 24 || x < gutter) view = v.copy(scrollSec = max(0f, vm.playheadSeconds - 0.5f))
        }
    }

    val labelStyle = TextStyle(fontSize = 10.sp, color = colors.onSurfaceVariant)

    Canvas(
        modifier
            .fillMaxSize()
            .onSizeChanged { boxSize = it }
            .pointerInput(clip) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val v0 = viewState.value ?: return@awaitEachGesture
                    val e0 = editState.value
                    val tm0 = e0.timeMap(an.length)
                    val geo0 = Geo(v0, gutter, ruler)
                    val boxes0 = layout(e0, an, tm0, geo0, sr)
                    val hitBox = hitTest(boxes0, down.position, geo0, an, tm0, shiftState.value, dp1)

                    var mode = when {
                        down.position.y < ruler -> Drag.SEEK
                        hitBox != null && toolState.value == Tool.SPLIT -> Drag.NONE
                        hitBox != null && toolState.value == Tool.TIME -> when {
                            abs(down.position.x - hitBox.x0) < edgeZone && abs(down.position.x - hitBox.x0) <= abs(down.position.x - hitBox.x1) -> Drag.TIME_LEFT
                            abs(down.position.x - hitBox.x1) < edgeZone -> Drag.TIME_RIGHT
                            else -> Drag.TIME_BODY
                        }
                        hitBox != null -> Drag.PITCH
                        else -> Drag.PAN
                    }
                    if (mode == Drag.SEEK) vm.seekTimeline(max(0.0, geo0.sec(down.position.x)).toFloat())

                    var started = false
                    var moved = false
                    var lastPos = down.position
                    var totalDy = 0f
                    var startOffsets = emptyMap<Int, Double>()
                    val slop = viewConfiguration.touchSlop

                    while (true) {
                        val event = awaitPointerEvent()
                        val pressed = event.changes.filter { it.pressed }
                        if (pressed.isEmpty()) break

                        if (pressed.size >= 2) {
                            // Two fingers: pinch to zoom time, drag to pan.
                            mode = Drag.ZOOM
                            val zoom = event.calculateZoom()
                            val pan = event.calculatePan()
                            val c = event.calculateCentroid()
                            val v = viewState.value ?: break
                            val pps = (v.pxPerSec * zoom).coerceIn(8f, 4000f)
                            val secAtCentroid = v.scrollSec + (c.x - gutter) / v.pxPerSec
                            view = v.copy(
                                pxPerSec = pps,
                                scrollSec = max(-1f, secAtCentroid - (c.x - gutter) / pps - pan.x / pps),
                                topMidi = (v.topMidi + pan.y / v.pxPerSemi).coerceIn(30f, 110f),
                            )
                            moved = true
                            event.changes.forEach { it.consume() }
                            continue
                        }

                        val change = pressed.first()
                        val delta = change.position - lastPos
                        lastPos = change.position
                        if (!moved && (change.position - down.position).getDistance() > slop) moved = true
                        if (!moved) continue

                        when (mode) {
                            Drag.SEEK -> vm.seekTimeline(max(0.0, geo0.sec(change.position.x)).toFloat())
                            Drag.PAN -> {
                                val v = viewState.value ?: break
                                view = v.copy(
                                    scrollSec = max(-1f, v.scrollSec - delta.x / v.pxPerSec),
                                    topMidi = (v.topMidi + delta.y / v.pxPerSemi).coerceIn(30f, 110f),
                                )
                            }
                            Drag.PITCH -> {
                                val box = hitBox!!
                                if (!started) {
                                    started = true
                                    if (box.index !in selectionState.value) vm.selectOnly(box.index)
                                    vm.beginGesture()
                                    val sel = if (box.index in selectionState.value) selectionState.value else setOf(box.index)
                                    startOffsets = sel.associateWith { editState.value.notes[it].offset }
                                    totalDy = 0f
                                }
                                totalDy += delta.y
                                val dSemi = -totalDy / v0.pxPerSemi
                                var label: String? = null
                                vm.modify { e, _ ->
                                    for ((idx, start) in startOffsets) {
                                        val n = e.notes.getOrNull(idx) ?: continue
                                        var target = n.center + shiftState.value + start + dSemi
                                        if (snapState.value) target = Scales.nearestAllowed(target, maskState.value).toDouble()
                                        n.offset = target - shiftState.value - n.center
                                        if (idx == box.index) label = pitchLabel(n.center + n.offset + shiftState.value)
                                    }
                                }
                                hint = label
                            }
                            Drag.TIME_LEFT, Drag.TIME_RIGHT, Drag.TIME_BODY -> {
                                val box = hitBox!!
                                if (!started) {
                                    started = true
                                    vm.beginGesture()
                                }
                                val dSamples = delta.x / v0.pxPerSec * sr
                                vm.modify { e, a ->
                                    val s = box.note.inStart(a)
                                    val en = box.note.inEnd(a)
                                    val keys = when (mode) {
                                        Drag.TIME_LEFT -> listOf(s)
                                        Drag.TIME_RIGHT -> listOf(en)
                                        else -> e.anchors.subMap(s, true, en, true).keys.toList()
                                    }
                                    e.moveAnchors(keys, dSamples.toDouble(), 0.01 * sr, a.length)
                                }
                            }
                            else -> {}
                        }
                        event.changes.forEach { it.consume() }
                    }
                    hint = null

                    if (!moved) {
                        when {
                            mode == Drag.SEEK -> {}
                            hitBox != null && toolState.value == Tool.SPLIT -> {
                                val tm = editState.value.timeMap(an.length)
                                val frame = (tm.toInput(geo0.sec(down.position.x) * sr) / an.hop).roundToInt()
                                vm.splitNote(hitBox.index, frame)
                            }
                            hitBox != null -> vm.toggleSelect(hitBox.index)
                            else -> vm.clearSelection()
                        }
                    }
                }
            },
    ) {
        @Suppress("UNUSED_VARIABLE") val redraw = tick
        @Suppress("UNUSED_VARIABLE") val version = vm.editVersion
        val v = view ?: return@Canvas
        val geo = Geo(v, gutter, ruler)
        val w = size.width
        val h = size.height
        val tm = edit.timeMap(an.length)

        drawRect(colors.surfaceContainerLow)

        // Pitch lanes.
        val botMidi = v.topMidi - (h - ruler) / v.pxPerSemi
        for (m in floor(botMidi).toInt() - 1..ceil(v.topMidi).toInt() + 1) {
            val top = geo.y(m + 0.5)
            val bottom = geo.y(m - 0.5)
            val allowed = Scales.isAllowed(mask, m)
            val black = Math.floorMod(m, 12) in intArrayOf(1, 3, 6, 8, 10)
            val tint = when {
                allowed && black -> colors.primary.copy(alpha = 0.16f)
                allowed -> colors.primary.copy(alpha = 0.10f)
                black -> colors.onSurface.copy(alpha = 0.07f)
                else -> Color.Transparent
            }
            if (tint != Color.Transparent) drawRect(tint, Offset(gutter, top), Size(w - gutter, bottom - top))
            if (Math.floorMod(m, 12) == 0) drawLine(colors.outlineVariant, Offset(gutter, bottom), Offset(w, bottom), 1f)
        }

        clipRect(left = gutter, top = ruler, right = w, bottom = h) {
            val boxes = layout(edit, an, tm, geo, sr)
            val loud = an.loudLevel
            val selection = vm.selection
            val pxPerFrame = v.pxPerSec * an.hop / sr
            val step = max(1, ceil(2.5f * dp1 / max(pxPerFrame, 0.01f)).toInt())

            for (box in boxes) {
                if (box.x1 < gutter - 20 || box.x0 > w + 20) continue
                val n = box.note
                val selected = box.index in selection
                val edited = n.offset != 0.0 || n.drift != 1f || n.mod != 1f || shift != 0.0

                val frames = ArrayList<Int>()
                var f = n.f0
                while (f < n.f1) { frames += f; f += step }
                frames += n.f1

                // Ghost of the original contour.
                if (edited) {
                    val ghost = Path()
                    var first = true
                    for (fr in frames) {
                        if (!an.voiced[fr]) continue
                        val x = geo.x(tm.toOutput((fr * an.hop).toDouble()) / sr)
                        val y = geo.y(an.midi(fr))
                        if (first) { ghost.moveTo(x, y); first = false } else ghost.lineTo(x, y)
                    }
                    drawPath(ghost, colors.onSurfaceVariant.copy(alpha = 0.55f), style = Stroke(width = 1.2f * dp1))
                }

                val top = ArrayList<Offset>(frames.size)
                val bottom = ArrayList<Offset>(frames.size)
                for (fr in frames) {
                    val x = geo.x(tm.toOutput((fr * an.hop).toDouble()) / sr)
                    val y = geo.y(n.pitchAt(fr.toDouble()) + shift)
                    val lv = (an.level[max(0, fr - 1)] + an.level[fr] + an.level[min(an.frames - 1, fr + 1)]) / 3f
                    val db = 20 * log10(lv / loud + 1e-6f)
                    val thick = (4f + 10f * ((db + 36f) / 36f).coerceIn(0f, 1f)) * dp1
                    top += Offset(x, y - thick / 2)
                    bottom += Offset(x, y + thick / 2)
                }
                val path = Path()
                path.moveTo(top.first().x, top.first().y)
                for (p in top) path.lineTo(p.x, p.y)
                for (p in bottom.asReversed()) path.lineTo(p.x, p.y)
                path.close()
                val fill = if (selected) colors.primary else colors.tertiaryContainer
                drawPath(path, fill)
                drawPath(path, if (selected) colors.onPrimaryContainer else colors.tertiary, style = Stroke(width = 1.2f * dp1))

                if (vm.tool == Tool.TIME) {
                    val hx = 3f * dp1
                    drawLine(colors.secondary, Offset(box.x0, geo.y(n.center + n.offset + shift) - 14 * dp1), Offset(box.x0, geo.y(n.center + n.offset + shift) + 14 * dp1), hx)
                    drawLine(colors.secondary, Offset(box.x1, geo.y(n.center + n.offset + shift) - 14 * dp1), Offset(box.x1, geo.y(n.center + n.offset + shift) + 14 * dp1), hx)
                }
            }

            // Playhead.
            val px = geo.x(vm.playheadSeconds.toDouble())
            if (px in gutter..w) drawLine(colors.error, Offset(px, ruler), Offset(px, h), 2f * dp1)
        }

        // Ruler.
        drawRect(colors.surfaceContainerHigh, Offset.Zero, Size(w, ruler))
        val steps = doubleArrayOf(0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0)
        val tickStep = steps.firstOrNull { it * v.pxPerSec >= 70f * dp1 } ?: 60.0
        var t = ceil(geo.sec(gutter) / tickStep) * tickStep
        while (geo.x(t) < w) {
            val x = geo.x(t)
            drawLine(colors.outline, Offset(x, ruler - 6 * dp1), Offset(x, ruler), 1f)
            drawText(measurer, formatTime(t), Offset(x + 3 * dp1, 2 * dp1), labelStyle)
            t += tickStep
        }

        // Key gutter drawn last so notes scrolled off the left edge are hidden.
        drawRect(colors.surfaceContainerHigh, Offset(0f, ruler), Size(gutter, h - ruler))
        for (m in floor(botMidi).toInt()..ceil(v.topMidi).toInt()) {
            val y = geo.y(m.toDouble())
            val isC = Math.floorMod(m, 12) == 0
            val allowed = Scales.isAllowed(mask, m)
            if (v.pxPerSemi > 11 * dp1 && (allowed || isC) || isC) {
                drawText(measurer, Scales.noteName(m), Offset(3 * dp1, y - 6 * dp1), labelStyle)
            }
        }

        hint?.let {
            drawText(measurer, it, Offset(gutter + 8 * dp1, ruler + 6 * dp1), TextStyle(fontSize = 16.sp, color = colors.primary))
        }
    }
}

private fun fitView(an: Analysis, edit: Edit, shift: Double, size: IntSize, gutter: Float, ruler: Float, dens: Float): RollView {
    val duration = max(0.5f, an.length.toFloat() / an.sampleRate)
    val pps = ((size.width - gutter) / duration).coerceIn(30f, 1500f)
    var lo = 48.0
    var hi = 72.0
    if (edit.notes.isNotEmpty()) {
        lo = edit.notes.minOf { it.center + it.offset } + shift - 3
        hi = edit.notes.maxOf { it.center + it.offset } + shift + 3
    }
    val semis = max(hi - lo, 14.0)
    val ps = ((size.height - ruler) / semis).toFloat().coerceIn(10f * dens, 36f * dens)
    val visible = (size.height - ruler) / ps
    val mid = (lo + hi) / 2
    return RollView(pps, 0f, ps, (mid + visible / 2).toFloat())
}

private fun layout(edit: Edit, an: Analysis, tm: TimeMap, geo: Geo, sr: Int): List<NoteBox> =
    edit.notes.mapIndexed { i, n ->
        NoteBox(i, n, geo.x(tm.toOutput(n.inStart(an).toDouble()) / sr), geo.x(tm.toOutput(n.inEnd(an).toDouble()) / sr))
    }

/** Finds the note under [p], allowing a generous touch margin around the drawn blob. */
private fun hitTest(boxes: List<NoteBox>, p: Offset, geo: Geo, an: Analysis, tm: TimeMap, shift: Double, dens: Float): NoteBox? {
    if (p.y < geo.ruler || p.x < geo.gutter) return null
    var best: NoteBox? = null
    var bestDist = Float.MAX_VALUE
    val slop = 16f * dens
    for (b in boxes) {
        if (p.x < b.x0 - slop / 2 || p.x > b.x1 + slop / 2) continue
        val frame = (tm.toInput(geo.sec(p.x) * an.sampleRate) / an.hop).coerceIn(b.note.f0.toDouble(), b.note.f1.toDouble())
        val y = geo.y(b.note.pitchAt(frame) + shift)
        val d = abs(p.y - y)
        if (d < slop + 10f * dens && d < bestDist) {
            best = b
            bestDist = d
        }
    }
    return best
}

private fun pitchLabel(midi: Double): String {
    val n = midi.roundToInt()
    val cents = ((midi - n) * 100).roundToInt()
    return "${Scales.noteName(n)} ${if (cents >= 0) "+" else ""}$cents cents"
}

private fun formatTime(t: Double): String {
    val m = (t / 60).toInt()
    val s = t - m * 60
    return if (m > 0) "%d:%04.1f".format(m, s) else "%.2fs".format(s)
}

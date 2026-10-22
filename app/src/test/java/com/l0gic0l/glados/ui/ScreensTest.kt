package com.l0gic0l.glados.ui

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import com.l0gic0l.glados.GladosApp
import com.l0gic0l.glados.MainViewModel
import com.l0gic0l.glados.Tool
import com.l0gic0l.glados.data.DarkMode
import com.l0gic0l.glados.data.ThemeSettings
import com.l0gic0l.glados.dsp.Audio
import com.l0gic0l.glados.dsp.NoteOps
import com.l0gic0l.glados.dsp.Wav
import com.l0gic0l.glados.model.Group
import com.l0gic0l.glados.model.PresetCategory
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.ui.editor.PianoRoll
import com.l0gic0l.glados.ui.editor.RollControls
import com.l0gic0l.glados.ui.theme.GladosTheme
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.round
import kotlin.math.sin

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-night-xxhdpi")
class ScreensTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private lateinit var vm: MainViewModel
    private val shotDir: File? = System.getenv("GLADOS_SHOT_DIR")?.takeIf { it.isNotEmpty() }?.let(::File)

    @Before
    fun setUp() {
        // Run view-model coroutines without waiting on the paused Robolectric main looper.
        Dispatchers.setMain(UnconfinedTestDispatcher())
        vm = MainViewModel(RuntimeEnvironment.getApplication() as GladosApp)
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    private fun shot(name: String) {
        val dir = shotDir ?: return
        dir.mkdirs()
        rule.onAllNodes(isRoot())[0].captureRoboImage(File(dir, "$name.png").path)
    }

    private fun showApp(dark: Boolean = true) {
        rule.setContent { GladosTheme(ThemeSettings(false, if (dark) DarkMode.DARK else DarkMode.LIGHT)) { MainScreen(vm) } }
    }

    private fun go(dest: String) {
        rule.onNodeWithTag("nav-$dest").performClick()
        rule.waitForIdle()
    }

    private fun speechClip(): Audio {
        System.getenv("GLADOS_TEST_WAV")?.takeIf { it.isNotEmpty() && File(it).exists() }?.let { return Wav.read(File(it).readBytes()) }
        val sr = 22050
        val hz = doubleArrayOf(130.0, 170.0, 110.0, 150.0)
        val x = FloatArray((sr * (0.2 + hz.size * 0.5)).toInt())
        var t0 = (0.1 * sr).toInt()
        for (f in hz) {
            val n = (0.4 * sr).toInt()
            var phase = 0.0
            for (i in 0 until n) {
                phase += 2 * PI * f / sr
                var s = 0.0
                for (k in 1..14) s += sin(k * phase) / (k * k.toDouble()) * (1.0 + 2.0 / (1 + ((k * f - 900) / 300).let { it * it }))
                val env = minOf(1f, i / 300f, (n - i) / 300f)
                x[t0 + i] = (0.3 * s * env).toFloat()
            }
            t0 += (0.5 * sr).toInt()
        }
        return Audio(x, sr)
    }

    private fun loadClip(autoCorrect: Boolean = false) {
        vm.loadAudio(speechClip(), "Test clip", autoCorrect = autoCorrect, autoplay = false)
        rule.waitUntil(timeoutMillis = 60_000) { vm.clip != null || (vm.busy == null && vm.message != null) }
        assertNotNull("loading failed: ${vm.message}", vm.clip)
        rule.waitUntil(timeoutMillis = 30_000) { vm.busy == null }
        rule.waitForIdle()
    }

    @Test
    fun voiceScreenHasAControlForEveryParameterAndPresetsApply() {
        showApp()
        go("voice")
        rule.onNodeWithTag("mode-advanced").performClick()
        rule.waitForIdle()
        while (rule.onAllNodesWithContentDescription("Expand").fetchSemanticsNodes().isNotEmpty()) {
            rule.onAllNodesWithContentDescription("Expand")[0].performScrollTo().performClick()
            rule.waitForIdle()
        }
        for (group in Group.entries) {
            for (spec in Specs.inGroup(group)) {
                rule.onNodeWithText(spec.label, substring = false).assertExists("missing control for ${spec.id}")
            }
        }
        shot("02-voice-advanced")
        rule.onNodeWithTag("preset-picker").performScrollTo().performClick()
        rule.waitForIdle()
        for (category in PresetCategory.entries) rule.onNodeWithText(category.title).assertExists()
        shot("02b-voice-presets")
        rule.onNodeWithText("Portal 2").performClick()
        rule.waitForIdle()
        assertEquals(70f, vm.params.value[Specs.RETUNE], 0f)
        rule.onNodeWithText("Announcer").performScrollTo().performClick()
        rule.waitForIdle()
        assertEquals("Announcer", vm.presetName(vm.params.value))
        rule.onNodeWithText("Portal 1").performScrollTo().performClick()
        rule.waitForIdle()
        assertEquals(Specs.RETUNE.default, vm.params.value[Specs.RETUNE], 0f)
        rule.onNodeWithText("Done").performClick()
        rule.waitForIdle()
        rule.onNodeWithText("Portal 1").assertExists()
    }

    @Test
    fun simpleModeSlidersMoveSeveralSettingsAtOnce() {
        showApp()
        go("voice")
        rule.onNodeWithText("Quick controls").assertExists()
        shot("02c-voice-simple")
        val before = vm.params.value
        rule.onNodeWithTag("macro-robotic").performScrollTo().performSemanticsAction(SemanticsActions.SetProgress) { it(100f) }
        rule.waitForIdle()
        val after = vm.params.value
        assertTrue("comb mix ${after[Specs.COMB_MIX]}", after[Specs.COMB_MIX] > 90f)
        assertTrue("ring mix ${after[Specs.RING_MIX]}", after[Specs.RING_MIX] > before[Specs.RING_MIX])
        assertTrue("crush mix ${after[Specs.CRUSH_MIX]}", after[Specs.CRUSH_MIX] > before[Specs.CRUSH_MIX])
        assertEquals(null, vm.presetName(after))
        rule.onNodeWithText("Custom settings").assertExists()
    }

    @Test
    fun aNewPresetRetunesAClipUntilItIsEdited() {
        loadClip(autoCorrect = true)
        vm.setParam(Specs.DENOISE, 85f)
        vm.applyPreset("Monotone")
        val p = vm.params.value
        assertEquals("presets keep the noise reduction setting", 85f, p[Specs.DENOISE], 0f)
        assertEquals("Monotone", vm.presetName(p))
        val an = vm.clip!!.analysis
        val shift = NoteOps.globalShift(an, p)
        val held = NoteOps.monotoneNote(an, p, shift)
        // Monotone holds the speaker's own average pitch rather than a fixed note.
        assertEquals(held.toDouble(), an.medianMidi(), 0.5)
        for (n in vm.edit!!.notes) assertEquals(held.toDouble(), n.center + n.offset + shift, 1e-6)
        vm.resetNotes() // a hand edit: from now on presets leave the notes alone
        vm.applyPreset("Portal 1")
        for (n in vm.edit!!.notes) assertEquals(0.0, n.offset, 0.0)
    }

    @Test
    fun speakScreenRenders() {
        showApp()
        go("speak")
        rule.onNodeWithTag("speak-button").assertExists()
        rule.onNodeWithText("What should GLaDOS say?").assertExists()
        shot("03-speak")
    }

    @Test
    fun editorStartsEmptyThenShowsNotes() {
        showApp()
        go("editor")
        rule.onNodeWithText("Note editor").assertIsDisplayed()
        shot("04-editor-empty")
        loadClip()
        rule.waitForIdle()
        assertNotNull(vm.edit)
        assertTrue("expected notes, got ${vm.edit!!.notes.size}", vm.edit!!.notes.isNotEmpty())
        rule.onNodeWithText("Selected notes").assertExists()
        shot("05-editor-notes")
        vm.selectOnly(1)
        rule.waitForIdle()
        shot("06-editor-selected")
    }

    @Test
    fun draggingANoteChangesItsPitchAndUndoRestoresIt() {
        loadClip()
        val controls = RollControls()
        rule.setContent {
            GladosTheme(ThemeSettings(false, DarkMode.DARK)) {
                Box(Modifier.size(400.dp, 300.dp).testTag("roll")) { PianoRoll(vm, controls, Modifier.fillMaxSize()) }
            }
        }
        rule.waitForIdle()
        vm.snapToScale = false
        vm.tool = Tool.PITCH
        val idx = 1
        val start = controls.noteCenter(idx) ?: error("note $idx not on screen")
        val before = vm.edit!!.notes[idx].offset
        rule.onNodeWithTag("roll").performTouchInput {
            down(start)
            moveTo(start.copy(y = start.y - 20f))
            moveTo(start.copy(y = start.y - 60f))
            moveTo(start.copy(y = start.y - 80f))
            up()
        }
        rule.waitForIdle()
        val after = vm.edit!!.notes[idx].offset
        println("drag offset $before -> $after")
        assertTrue("dragging up should raise pitch ($before -> $after)", after > before + 0.5)
        assertTrue(vm.canUndo)
        assertTrue(idx in vm.selection)
        shot("07-roll-dragged")
        vm.undo()
        assertEquals(before, vm.edit!!.notes[idx].offset, 1e-9)
    }

    @Test
    fun snappingLandsOnAllowedNotes() {
        loadClip()
        val controls = RollControls()
        rule.setContent {
            GladosTheme(ThemeSettings(false, DarkMode.DARK)) {
                Box(Modifier.size(400.dp, 300.dp).testTag("roll")) { PianoRoll(vm, controls, Modifier.fillMaxSize()) }
            }
        }
        rule.waitForIdle()
        vm.setParam(Specs.SCALE, 1f) // C major
        vm.setParam(Specs.AUTO_REG, 0f)
        vm.setParam(Specs.SHIFT, 0f)
        rule.waitForIdle() // let the roll pick up the new settings before dragging
        vm.snapToScale = true
        val idx = 0
        val start = controls.noteCenter(idx)!!
        rule.onNodeWithTag("roll").performTouchInput {
            down(start)
            moveTo(start.copy(y = start.y - 40f))
            moveTo(start.copy(y = start.y - 100f))
            moveTo(start.copy(y = start.y - 160f))
            up()
        }
        rule.waitForIdle()
        val n = vm.edit!!.notes[idx]
        val finalPitch = n.center + n.offset
        println("snapped pitch $finalPitch (offset ${n.offset}, selection ${vm.selection}, canUndo ${vm.canUndo}, start $start, frames ${n.f0}..${n.f1})")
        assertTrue("the drag should have moved the note", n.offset != 0.0)
        assertEquals(0.0, finalPitch - round(finalPitch), 1e-6)
        val pc = Math.floorMod(round(finalPitch).toInt(), 12)
        assertTrue("pitch class $pc is not in C major", pc in setOf(0, 2, 4, 5, 7, 9, 11))
    }

    @Test
    fun splitToolCutsANote() {
        loadClip()
        val controls = RollControls()
        rule.setContent {
            GladosTheme(ThemeSettings(false, DarkMode.DARK)) {
                Box(Modifier.size(400.dp, 300.dp).testTag("roll")) { PianoRoll(vm, controls, Modifier.fillMaxSize()) }
            }
        }
        rule.waitForIdle()
        vm.tool = Tool.SPLIT
        val before = vm.edit!!.notes.size
        val p = controls.noteCenter(1)!!
        rule.onNodeWithTag("roll").performTouchInput { click(p) }
        rule.waitForIdle()
        assertEquals(before + 1, vm.edit!!.notes.size)
        vm.undo()
        assertEquals(before, vm.edit!!.notes.size)
    }

    @Test
    fun lightThemeRenders() {
        loadClip()
        showApp(dark = false)
        shot("09-speak-light")
        go("editor")
        vm.selectOnly(1)
        rule.waitForIdle()
        shot("10-editor-light")
    }

    @Test
    fun themeDialogChangesMode() {
        showApp()
        rule.onNodeWithTag("theme-button").performClick()
        rule.onNodeWithText("Wallpaper colors").assertIsDisplayed()
        shot("08-theme")
        rule.onNodeWithText("Light").performClick()
        rule.waitForIdle()
        assertEquals(DarkMode.LIGHT, vm.theme.value.darkMode)
        rule.onNodeWithText("System").performClick()
    }

    @Test
    fun typedSpeechTextSurvivesSwitchingTabs() {
        showApp()
        go("speak")
        rule.onNode(hasSetTextAction()).performTextReplacement("Remember me")
        rule.waitForIdle()
        go("voice")
        rule.onNodeWithTag("mode-simple").assertExists()
        go("speak")
        rule.onNodeWithText("Remember me").assertExists()
    }

    @Test
    fun overlappingLoadsEndWithTheLastClip() {
        val first = speechClip()
        val second = Audio(first.samples.copyOf(first.samples.size / 2), first.sampleRate)
        vm.loadAudio(first, "first", autoCorrect = false, autoplay = false)
        vm.loadAudio(second, "second", autoCorrect = false, autoplay = false)
        rule.waitUntil(timeoutMillis = 60_000) { vm.clip != null && vm.busy == null }
        rule.waitForIdle()
        assertEquals("the older load must not win", "second", vm.clip!!.title)
        assertEquals(second.samples.size, vm.clip!!.audio.samples.size)
    }

    @Test
    fun aRefusedSplitLeavesUndoAndRedoAlone() {
        loadClip()
        vm.resetNotes()
        vm.undo()
        assertTrue(vm.canRedo)
        val note = vm.edit!!.notes[0]
        vm.splitNote(0, note.f0 + 1) // too close to the edge: refused
        assertTrue("a refused split wiped the redo history", vm.canRedo)
        assertTrue(vm.message != null)
        vm.redo()
        assertTrue(vm.canUndo)
    }

    @Test
    fun aUserPresetCannotShadowABuiltInOne() {
        vm.saveUserPreset("portal 1")
        assertTrue(vm.message != null)
        assertTrue(vm.userPresets.value.isEmpty())
        vm.saveUserPreset("Mine")
        assertTrue(vm.userPresets.value.containsKey("Mine"))
    }
}

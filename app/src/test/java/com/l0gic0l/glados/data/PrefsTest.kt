package com.l0gic0l.glados.data

import android.content.Context
import com.l0gic0l.glados.model.Specs
import com.l0gic0l.glados.model.VoiceParams
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PrefsTest {
    private val context: Context get() = RuntimeEnvironment.getApplication()

    @Test
    fun settingsFromBeforeNoiseReductionLoseTheOldRegisterDefault() {
        context.getSharedPreferences("glados", Context.MODE_PRIVATE).edit()
            .putString("params", """{"autoReg":1.0,"retuneMs":42.0}""").commit()
        val p = Prefs(context).loadParams()
        assertFalse(p.bool(Specs.AUTO_REG))
        assertEquals(42f, p[Specs.RETUNE], 0f)
    }

    @Test
    fun currentSettingsKeepRegisterMatchingWhenChosen() {
        val prefs = Prefs(context)
        prefs.saveParams(VoiceParams.defaults().with(Specs.AUTO_REG, 1f))
        assertTrue(prefs.loadParams().bool(Specs.AUTO_REG))
    }
}

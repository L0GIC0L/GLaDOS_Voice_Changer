package com.l0gic0l.glados.data

import android.content.Context
import androidx.core.content.edit
import com.l0gic0l.glados.model.VoiceParams
import org.json.JSONObject

enum class DarkMode { SYSTEM, LIGHT, DARK }

class ThemeSettings(val dynamicColor: Boolean = true, val darkMode: DarkMode = DarkMode.SYSTEM)

/** Small SharedPreferences store for settings, user presets and theme. */
class Prefs(context: Context) {
    private val sp = context.getSharedPreferences("glados", Context.MODE_PRIVATE)

    fun loadParams(): VoiceParams = readMap(sp.getString(KEY_PARAMS, null))?.let(VoiceParams::fromMap) ?: VoiceParams.defaults()

    fun saveParams(p: VoiceParams) {
        sp.edit { putString(KEY_PARAMS, JSONObject(p.toMap()).toString()) }
    }

    fun loadUserPresets(): Map<String, Map<String, Float>> {
        val raw = sp.getString(KEY_PRESETS, null) ?: return emptyMap()
        return try {
            val obj = JSONObject(raw)
            obj.keys().asSequence().associateWith { readMap(obj.getString(it)) ?: emptyMap() }
        } catch (e: Exception) {
            emptyMap()
        }
    }

    fun saveUserPresets(presets: Map<String, Map<String, Float>>) {
        val obj = JSONObject()
        for ((name, values) in presets) obj.put(name, JSONObject(values).toString())
        sp.edit { putString(KEY_PRESETS, obj.toString()) }
    }

    fun loadTheme(): ThemeSettings = ThemeSettings(
        dynamicColor = sp.getBoolean(KEY_DYNAMIC, true),
        darkMode = DarkMode.entries.getOrElse(sp.getInt(KEY_DARK, 0)) { DarkMode.SYSTEM },
    )

    fun saveTheme(t: ThemeSettings) {
        sp.edit {
            putBoolean(KEY_DYNAMIC, t.dynamicColor)
            putInt(KEY_DARK, t.darkMode.ordinal)
        }
    }

    var simpleMode: Boolean
        get() = sp.getBoolean(KEY_SIMPLE, true)
        set(v) = sp.edit { putBoolean(KEY_SIMPLE, v) }

    private fun readMap(json: String?): Map<String, Float>? {
        if (json == null) return null
        return try {
            val obj = JSONObject(json)
            obj.keys().asSequence().associateWith { obj.getDouble(it).toFloat() }
        } catch (e: Exception) {
            null
        }
    }

    private companion object {
        const val KEY_PARAMS = "params"
        const val KEY_PRESETS = "userPresets"
        const val KEY_DYNAMIC = "dynamicColor"
        const val KEY_DARK = "darkMode"
        const val KEY_SIMPLE = "simpleMode"
    }
}

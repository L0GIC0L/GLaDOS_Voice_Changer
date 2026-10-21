package com.l0gic0l.glados.model

import com.l0gic0l.glados.dsp.Scales
import kotlin.math.abs
import kotlin.math.roundToInt

enum class Group(val title: String, val blurb: String) {
    TUNING("Tuning", "Which notes the voice is pulled to, and how hard."),
    VOICE("Voice", "Register, formants and pitch detection."),
    ROBOT("Robot", "The metallic, digital character."),
    SPACE("Space", "Echo and the test chamber."),
    OUTPUT("EQ and output", "Final tone shaping and level."),
}

/** One user-adjustable setting. Every control in the app is generated from these. */
class ParamSpec(
    val id: String,
    val label: String,
    val group: Group,
    val min: Float,
    val max: Float,
    val default: Float,
    val step: Float = 0f,
    val unit: String = "",
    val help: String = "",
    val choices: List<String>? = null,
    val toggle: Boolean = false,
    val hidden: Boolean = false,
    val formatter: ((Float) -> String)? = null,
) {
    /** Position in [VoiceParams]; assigned by [Specs]. */
    var index: Int = -1
        internal set

    val isChoice: Boolean get() = choices != null

    fun clamp(v: Float): Float {
        val c = v.coerceIn(min, max)
        if (step <= 0f) return c
        // Snap to the step grid in double precision, then trim the representation error so
        // values like 0.1 * 120 - 12 come back as exactly 0.
        val snapped = min.toDouble() + Math.round((c - min).toDouble() / step) * step.toDouble()
        return (Math.round(snapped * 10000.0) / 10000.0).toFloat().coerceIn(min, max)
    }

    fun format(v: Float): String {
        formatter?.let { return it(v) }
        choices?.let { return it[v.roundToInt().coerceIn(0, it.size - 1)] }
        if (toggle) return if (v >= 0.5f) "On" else "Off"
        val text = when {
            step >= 1f || abs(v) >= 1000f -> v.roundToInt().toString()
            step >= 0.1f -> "%.1f".format(v)
            else -> "%.2f".format(v)
        }
        return if (unit.isEmpty()) text else "$text $unit"
    }
}

object Specs {
    private val list = ArrayList<ParamSpec>()

    private fun reg(spec: ParamSpec): ParamSpec {
        spec.index = list.size
        list += spec
        return spec
    }

    private fun slider(
        id: String, label: String, group: Group, min: Float, max: Float, default: Float,
        step: Float = 1f, unit: String = "", help: String = "",
        formatter: ((Float) -> String)? = null,
    ) = reg(ParamSpec(id, label, group, min, max, default, step, unit, help, formatter = formatter))

    private fun toggle(id: String, label: String, group: Group, default: Boolean, help: String = "") =
        reg(ParamSpec(id, label, group, 0f, 1f, if (default) 1f else 0f, 1f, help = help, toggle = true))

    private fun choice(id: String, label: String, group: Group, options: List<String>, default: Int, help: String = "") =
        reg(ParamSpec(id, label, group, 0f, (options.size - 1).toFloat(), default.toFloat(), 1f, help = help, choices = options))

    // ---- Tuning ----
    val KEY = choice("key", "Key", Group.TUNING, Scales.noteNames.toList(), 0, "Root note of the scale.")
    val SCALE = choice("scale", "Scale", Group.TUNING, Scales.all.map { it.name }, 0, "Notes the voice may land on.")
    val CUSTOM_MASK = reg(
        ParamSpec("customMask", "Custom notes", Group.TUNING, 0f, 4095f, 4095f, 1f, hidden = true),
    )
    val RETUNE = slider(
        "retuneMs", "Retune speed", Group.TUNING, 0f, 500f, 20f, 1f, "ms",
        "0 snaps instantly (hard robotic steps). Higher values let natural glides through.",
    )
    val STRENGTH = slider("strength", "Strength", Group.TUNING, 0f, 100f, 100f, 1f, "%", "How far toward the target note the pitch is pulled.")
    val HOLD = slider("holdCents", "Note hold", Group.TUNING, 0f, 100f, 25f, 1f, "cents", "Extra distance a note must be crossed before switching to the next. Stops flicker between notes.")
    val SHIFT = slider("shift", "Pitch shift", Group.TUNING, -12f, 12f, 0f, 0.1f, "st", "Transposes the voice before tuning.")
    val AUTO_REG = toggle("autoReg", "Match target register", Group.TUNING, true, "Measures your average pitch and shifts it to the target register below, so any voice lands near GLaDOS.")
    val REG_HZ = slider("regHz", "Target register", Group.TUNING, 120f, 400f, 215f, 5f, "Hz", "Average pitch the voice is moved to.")
    val MONOTONE = toggle("monotone", "Monotone", Group.TUNING, false, "Force every word onto a single note.")
    val MONO_NOTE = slider(
        "monoNote", "Monotone note", Group.TUNING, 36f, 84f, 57f, 1f,
        formatter = { Scales.noteName(it.roundToInt()) },
    )
    val VIB_DEPTH = slider("vibDepth", "Vibrato depth", Group.TUNING, 0f, 100f, 0f, 1f, "cents")
    val VIB_RATE = slider("vibRate", "Vibrato rate", Group.TUNING, 0.5f, 10f, 5.5f, 0.1f, "Hz")
    val REF_A4 = slider("refA4", "Reference A4", Group.TUNING, 415f, 466f, 440f, 1f, "Hz")

    // ---- Voice ----
    val FORMANT = slider("formant", "Formant shift", Group.VOICE, -12f, 12f, 1.5f, 0.1f, "st", "Moves the vocal tract character without changing pitch. Up is smaller and more feminine.")
    val FORMANT_FOLLOW = slider(
        "formantFollow", "Formants follow pitch", Group.VOICE, 0f, 100f, 35f, 1f, "%",
        "Moves formants a fraction of however far the pitch is shifted, so a deep voice raised to GLaDOS pitch also gets a smaller vocal tract.",
    )
    val FMIN = slider("fmin", "Lowest pitch", Group.VOICE, 50f, 300f, 85f, 5f, "Hz", "Lowest voice pitch to track. Lower values add latency.")
    val FMAX = slider("fmax", "Highest pitch", Group.VOICE, 300f, 1200f, 700f, 10f, "Hz")
    val GATE = slider("gateDb", "Noise gate", Group.VOICE, -80f, -20f, -58f, 1f, "dB", "Silences input quieter than this.")
    val VOICING = slider("voicing", "Voicing sensitivity", Group.VOICE, 0f, 100f, 55f, 1f, "%", "Higher treats more sound as pitched. Lower leaves breathy or noisy parts untouched.")
    val BREATH = slider("breath", "Breath and consonants", Group.VOICE, 0f, 200f, 80f, 1f, "%", "Level of unpitched sound such as s and t.")
    val DRY = slider("dry", "Dry voice", Group.VOICE, 0f, 100f, 0f, 1f, "%", "Blend in the unprocessed voice.")

    // ---- Robot ----
    val RING_MIX = slider("ringMix", "Ring mod mix", Group.ROBOT, 0f, 100f, 10f, 1f, "%")
    val RING_HZ = slider("ringHz", "Ring mod frequency", Group.ROBOT, 20f, 1000f, 85f, 1f, "Hz")
    val COMB_MIX = slider("combMix", "Metal comb mix", Group.ROBOT, 0f, 100f, 15f, 1f, "%", "Short feedback delay that gives a hollow metallic ring.")
    val COMB_MS = slider("combMs", "Comb delay", Group.ROBOT, 0.5f, 15f, 3.2f, 0.1f, "ms")
    val COMB_FB = slider("combFb", "Comb feedback", Group.ROBOT, 0f, 95f, 55f, 1f, "%")
    val COMB_MOD = slider("combMod", "Flange depth", Group.ROBOT, 0f, 100f, 15f, 1f, "%")
    val COMB_RATE = slider("combRate", "Flange rate", Group.ROBOT, 0.05f, 8f, 0.35f, 0.05f, "Hz")
    val CRUSH_MIX = slider("crushMix", "Bit crush mix", Group.ROBOT, 0f, 100f, 15f, 1f, "%")
    val CRUSH_BITS = slider("crushBits", "Bit depth", Group.ROBOT, 3f, 16f, 10f, 1f, "bit")
    val CRUSH_HOLD = slider("crushHold", "Sample hold", Group.ROBOT, 1f, 40f, 3f, 1f, "x", "Repeats each sample this many times, lowering the effective sample rate.")
    val GLITCH = slider("glitch", "Glitch rate", Group.ROBOT, 0f, 100f, 12f, 1f, "%", "How often the sound stutters like a failing chip.")
    val GLITCH_MS = slider("glitchMs", "Glitch length", Group.ROBOT, 15f, 250f, 70f, 1f, "ms")

    // ---- Space ----
    val ECHO_MIX = slider("echoMix", "Echo mix", Group.SPACE, 0f, 100f, 0f, 1f, "%")
    val ECHO_MS = slider("echoMs", "Echo time", Group.SPACE, 30f, 900f, 220f, 5f, "ms")
    val ECHO_FB = slider("echoFb", "Echo feedback", Group.SPACE, 0f, 90f, 35f, 1f, "%")
    val REVERB_MIX = slider("reverbMix", "Reverb mix", Group.SPACE, 0f, 100f, 10f, 1f, "%")
    val REVERB_SIZE = slider("reverbSize", "Room size", Group.SPACE, 0f, 100f, 55f, 1f, "%")
    val REVERB_DAMP = slider("reverbDamp", "Damping", Group.SPACE, 0f, 100f, 50f, 1f, "%")

    // ---- EQ and output ----
    val HPF = slider("hpf", "Low cut", Group.OUTPUT, 20f, 600f, 140f, 5f, "Hz")
    val LPF = slider("lpf", "High cut", Group.OUTPUT, 2000f, 20000f, 8000f, 100f, "Hz")
    val LOW_DB = slider("lowDb", "Low shelf (200 Hz)", Group.OUTPUT, -12f, 12f, -1f, 0.5f, "dB")
    val MID_DB = slider("midDb", "Presence gain", Group.OUTPUT, -12f, 12f, 3f, 0.5f, "dB")
    val MID_HZ = slider("midHz", "Presence frequency", Group.OUTPUT, 300f, 6000f, 2800f, 50f, "Hz")
    val HIGH_DB = slider("highDb", "High shelf (6 kHz)", Group.OUTPUT, -12f, 12f, 0f, 0.5f, "dB")
    val OUT_DB = slider("outDb", "Output level", Group.OUTPUT, -24f, 12f, 0f, 0.5f, "dB")

    val all: List<ParamSpec> = list
    val byId: Map<String, ParamSpec> = all.associateBy { it.id }

    fun inGroup(g: Group): List<ParamSpec> = all.filter { it.group == g && !it.hidden }
}

/** Immutable snapshot of every setting. Cheap to copy; safe to hand to the audio thread. */
class VoiceParams private constructor(private val v: FloatArray) {
    operator fun get(s: ParamSpec): Float = v[s.index]
    fun bool(s: ParamSpec): Boolean = v[s.index] >= 0.5f
    fun int(s: ParamSpec): Int = v[s.index].roundToInt()

    fun with(s: ParamSpec, value: Float): VoiceParams {
        val c = v.copyOf()
        c[s.index] = s.clamp(value)
        return VoiceParams(c)
    }

    fun withAll(values: Map<String, Float>): VoiceParams {
        val c = v.copyOf()
        for ((id, value) in values) Specs.byId[id]?.let { c[it.index] = it.clamp(value) }
        return VoiceParams(c)
    }

    fun toMap(): Map<String, Float> = Specs.all.associate { it.id to v[it.index] }

    override fun equals(other: Any?): Boolean = other is VoiceParams && v.contentEquals(other.v)
    override fun hashCode(): Int = v.contentHashCode()

    companion object {
        fun defaults(): VoiceParams = VoiceParams(FloatArray(Specs.all.size) { Specs.all[it].default })
        fun fromMap(m: Map<String, Float>): VoiceParams = defaults().withAll(m)
    }
}

class Preset(val name: String, val blurb: String, val overrides: Map<String, Float>) {
    fun apply(): VoiceParams = VoiceParams.defaults().withAll(overrides)
}

object Presets {
    /** The defaults in [Specs] are the Portal 1 voice. */
    val all: List<Preset> = listOf(
        Preset("Portal 1", "Stepped intonation, metallic buzz, digital grit.", emptyMap()),
        Preset(
            "Portal 2", "Smoother and more human, with chamber reverb.",
            mapOf(
                "retuneMs" to 70f, "strength" to 85f, "holdCents" to 35f, "formant" to 1f, "regHz" to 205f,
                "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f,
                "echoMix" to 6f, "echoMs" to 260f, "echoFb" to 30f,
                "reverbMix" to 22f, "reverbSize" to 70f, "reverbDamp" to 45f,
                "hpf" to 120f, "lpf" to 12000f, "midDb" to 2f,
            ),
        ),
        Preset(
            "PotatOS", "Low-voltage and lo-fi, a little drunk.",
            mapOf(
                "autoReg" to 0f, "shift" to -2f, "formant" to -1.5f, "retuneMs" to 45f,
                "ringMix" to 0f, "combMix" to 20f, "combMs" to 5f,
                "crushMix" to 70f, "crushBits" to 6f, "crushHold" to 8f, "glitch" to 22f,
                "reverbMix" to 5f, "hpf" to 280f, "lpf" to 3600f, "midDb" to 4f, "midHz" to 1800f,
            ),
        ),
        Preset(
            "Hard tune", "Clean snap-to-note autotune with no effects.",
            mapOf(
                "autoReg" to 0f, "formant" to 0f, "retuneMs" to 0f,
                "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f,
                "reverbMix" to 0f, "hpf" to 70f, "lpf" to 18000f, "lowDb" to 0f, "midDb" to 0f,
            ),
        ),
        Preset(
            "Natural", "Barely processed. Useful as a starting point.",
            mapOf(
                "autoReg" to 0f, "strength" to 0f, "formant" to 0f,
                "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f,
                "reverbMix" to 0f, "hpf" to 70f, "lpf" to 18000f, "lowDb" to 0f, "midDb" to 0f,
            ),
        ),
    )
}

package com.l0gic0l.glados.model

enum class PresetCategory(val title: String) {
    GLADOS("GLaDOS"),
    APERTURE("Aperture Science"),
    MACHINES("Robots and radios"),
    CHARACTERS("Characters"),
    UTILITY("Utility"),
}

class Preset(val name: String, val category: PresetCategory, val blurb: String, val overrides: Map<String, Float>) {
    private val params by lazy { VoiceParams.defaults().withAll(overrides) }
    fun apply(): VoiceParams = params
}

object Presets {
    /** Everything off: no tuning, no robot, no glitches, no room, flat EQ. Non-GLaDOS presets build on this. */
    private val CLEAN = mapOf(
        "autoReg" to 0f, "strength" to 0f, "formant" to 0f, "formantFollow" to 0f,
        "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f,
        "reverbMix" to 0f, "hpf" to 70f, "lpf" to 18000f, "lowDb" to 0f, "midDb" to 0f,
    )

    /** Built on the defaults in [Specs], which are the Portal 1 voice. */
    private fun glados(name: String, blurb: String, vararg kv: Pair<String, Float>) =
        Preset(name, PresetCategory.GLADOS, blurb, mapOf(*kv))

    private fun clean(name: String, category: PresetCategory, blurb: String, vararg kv: Pair<String, Float>) =
        Preset(name, category, blurb, CLEAN + kv)

    val all: List<Preset> = listOf(
        // ---- GLaDOS ----
        glados("Portal 1", "Stepped intonation, metallic buzz, digital grit."),
        glados(
            "Portal 2", "Smoother and more human, with chamber reverb.",
            "retuneMs" to 70f, "strength" to 85f, "formant" to 1f, "regHz" to 205f,
            "ringMix" to 0f, "combMix" to 0f, "crushMix" to 0f, "glitch" to 0f,
            "echoMix" to 6f, "echoMs" to 260f, "echoFb" to 30f,
            "reverbMix" to 22f, "reverbSize" to 70f, "reverbDamp" to 45f,
            "hpf" to 120f, "lpf" to 12000f, "midDb" to 2f,
        ),
        glados(
            "PotatOS", "Low-voltage and lo-fi, a little drunk.",
            "autoReg" to 0f, "shift" to -2f, "formant" to -1.5f, "retuneMs" to 45f,
            "ringMix" to 0f, "combMix" to 20f, "combMs" to 5f,
            "crushMix" to 70f, "crushBits" to 6f, "crushHold" to 8f, "glitch" to 22f,
            "reverbMix" to 5f, "hpf" to 280f, "lpf" to 3600f, "midDb" to 4f, "midHz" to 1800f,
        ),
        glados(
            "Corrupted", "Portal 1 coming apart: stutters, reversals, static and buzz.",
            "glitch" to 35f, "glitchReverse" to 15f, "glitchPitch" to 15f, "glitchDrop" to 15f, "glitchStatic" to 15f,
            "crushMix" to 35f, "crushBits" to 7f, "buzz" to 15f, "buzzHz" to 120f,
        ),
        glados(
            "Waking up", "Powering on: low, unsteady, dropping out and grinding to a halt.",
            "shift" to -3f, "formant" to 0f, "retuneMs" to 40f, "vibDepth" to 30f, "vibRate" to 1.5f,
            "glitch" to 15f, "glitchTape" to 30f, "glitchDrop" to 20f,
            "crushMix" to 25f, "buzz" to 20f, "buzzHz" to 60f,
        ),
        glados(
            "Singing", "Hard-tuned for songs, with a gentle vibrato and a little hall.",
            "retuneMs" to 5f, "vibDepth" to 25f, "vibRate" to 5.5f, "glitch" to 0f, "crushMix" to 5f,
            "reverbMix" to 20f, "reverbSize" to 65f, "echoMix" to 8f, "echoMs" to 300f,
        ),
        glados(
            "Monotone", "Every word on one note. Eerily calm.",
            "monotone" to 1f, "monoNote" to 57f, "retuneMs" to 10f, "glitch" to 5f,
        ),

        // ---- Aperture Science ----
        clean(
            "Announcer", PresetCategory.APERTURE, "The calm facility PA, ringing round a big hall.",
            "hpf" to 250f, "lpf" to 6000f, "midDb" to 4f, "midHz" to 2500f, "drive" to 15f,
            "echoMix" to 12f, "echoMs" to 140f, "echoFb" to 25f,
            "reverbMix" to 30f, "reverbSize" to 85f, "reverbDamp" to 60f,
        ),
        clean(
            "Emergency alert", PresetCategory.APERTURE, "Urgent, overdriven PA with a mains buzz.",
            "hpf" to 350f, "lpf" to 4500f, "midDb" to 6f, "midHz" to 2000f, "drive" to 35f,
            "buzz" to 15f, "buzzHz" to 120f,
            "echoMix" to 20f, "echoMs" to 300f, "echoFb" to 40f, "reverbMix" to 25f, "reverbSize" to 75f,
        ),
        clean(
            "Turret", PresetCategory.APERTURE, "Small, high and sweet. Are you still there?",
            "autoReg" to 1f, "regHz" to 330f, "formant" to 4f, "formantFollow" to 60f,
            "strength" to 100f, "retuneMs" to 15f,
            "combMix" to 25f, "combMs" to 1.5f, "combFb" to 50f, "crushMix" to 10f, "crushBits" to 12f,
            "hpf" to 250f, "lpf" to 9000f, "reverbMix" to 8f,
        ),
        clean(
            "Defective turret", PresetCategory.APERTURE, "A low, cracked turret with a failing chip.",
            "autoReg" to 1f, "regHz" to 165f, "formant" to 1f, "formantFollow" to 40f,
            "strength" to 100f, "retuneMs" to 25f,
            "combMix" to 20f, "combMs" to 2.5f, "crushMix" to 35f, "crushBits" to 7f,
            "glitch" to 30f, "glitchPitch" to 20f, "glitchJumpSt" to 12f,
            "hpf" to 200f, "lpf" to 7000f,
        ),
        clean(
            "Personality core", PresetCategory.APERTURE, "Friendly, a bit tinny, speaking in its own melody.",
            "formant" to 1f, "hpf" to 180f, "lpf" to 7000f, "midDb" to 3f,
            "combMix" to 12f, "combMs" to 2f, "combFb" to 40f, "ringMix" to 5f, "ringHz" to 60f, "reverbMix" to 6f,
        ),
        clean(
            "Space core", PresetCategory.APERTURE, "Overexcited, high and wobbling all over the place.",
            "autoReg" to 1f, "regHz" to 300f, "formant" to 3f, "formantFollow" to 40f,
            "vibDepth" to 40f, "vibRate" to 7f, "glitchPitch" to 25f, "glitchJumpSt" to 7f,
            "combMix" to 10f, "hpf" to 200f, "lpf" to 8000f,
        ),

        // ---- Robots and radios ----
        clean(
            "Classic robot", PresetCategory.MACHINES, "Flat, buzzy B-movie robot.",
            "monotone" to 1f, "monoNote" to 50f, "strength" to 100f, "retuneMs" to 0f,
            "ringMix" to 45f, "ringHz" to 50f, "combMix" to 40f, "combMs" to 5f, "combFb" to 70f, "combMod" to 0f,
            "hpf" to 100f, "lpf" to 6000f,
        ),
        clean(
            "Vocoder", PresetCategory.MACHINES, "Hard-snapped pitch with a ringing metal resonance.",
            "strength" to 100f, "retuneMs" to 0f, "ringMix" to 25f, "ringHz" to 110f,
            "combMix" to 50f, "combMs" to 4.5f, "combFb" to 85f, "combMod" to 0f,
            "hpf" to 120f, "lpf" to 9000f, "midDb" to 3f,
        ),
        clean(
            "8-bit computer", PresetCategory.MACHINES, "Crunchy, low-resolution computer speech.",
            "strength" to 100f, "retuneMs" to 0f, "crushMix" to 85f, "crushBits" to 4f, "crushHold" to 10f,
            "glitch" to 10f, "glitchMs" to 40f, "hpf" to 150f, "lpf" to 7000f,
        ),
        clean(
            "Low battery", PresetCategory.MACHINES, "Slow, droopy and running out of power.",
            "shift" to -2f, "vibDepth" to 30f, "vibRate" to 1.5f, "glitchTape" to 45f, "glitchDrop" to 10f,
            "crushMix" to 20f, "crushBits" to 8f, "buzz" to 10f, "buzzHz" to 50f, "hpf" to 120f, "lpf" to 7000f,
        ),
        clean(
            "Radio transmission", PresetCategory.MACHINES, "Distant walkie-talkie with crackles and drops.",
            "hpf" to 450f, "lpf" to 3000f, "midDb" to 5f, "midHz" to 1800f, "drive" to 40f,
            "glitchStatic" to 30f, "glitchDrop" to 10f, "crushMix" to 15f, "crushBits" to 8f, "buzz" to 5f,
        ),
        clean(
            "Telephone", PresetCategory.MACHINES, "A narrow phone line.",
            "hpf" to 330f, "lpf" to 3400f, "midDb" to 3f, "midHz" to 1500f, "drive" to 15f,
            "crushMix" to 20f, "crushBits" to 8f,
        ),
        clean(
            "Megaphone", PresetCategory.MACHINES, "A distorted bullhorn.",
            "hpf" to 500f, "lpf" to 4500f, "midDb" to 8f, "midHz" to 1600f, "drive" to 70f,
            "echoMix" to 10f, "echoMs" to 120f, "echoFb" to 20f, "reverbMix" to 10f, "reverbSize" to 50f,
        ),
        clean(
            "Stadium announcer", PresetCategory.MACHINES, "A booming voice echoing round an arena.",
            "hpf" to 120f, "lpf" to 8000f, "lowDb" to 3f, "midDb" to 3f, "drive" to 10f,
            "echoMix" to 35f, "echoMs" to 380f, "echoFb" to 45f,
            "reverbMix" to 35f, "reverbSize" to 95f, "reverbDamp" to 40f,
        ),

        // ---- Characters ----
        clean(
            "Chipmunk", PresetCategory.CHARACTERS, "Tiny and squeaky.",
            "shift" to 10f, "formant" to 8f, "hpf" to 150f,
        ),
        clean(
            "Giant", PresetCategory.CHARACTERS, "Huge, slow and booming.",
            "shift" to -9f, "formant" to -6f, "lowDb" to 4f, "hpf" to 40f, "lpf" to 9000f,
            "reverbMix" to 20f, "reverbSize" to 80f,
        ),
        clean(
            "Movie trailer", PresetCategory.CHARACTERS, "In a world... a deep, polished narrator.",
            "shift" to -4f, "formant" to -2f, "lowDb" to 4f, "midDb" to 2f, "drive" to 10f,
            "reverbMix" to 15f, "reverbSize" to 70f,
        ),
        clean(
            "Demon", PresetCategory.CHARACTERS, "Deep, distorted and doubled with your own voice.",
            "shift" to -12f, "formant" to -5f, "dry" to 35f, "drive" to 25f, "ringMix" to 15f, "ringHz" to 40f,
            "lowDb" to 5f, "hpf" to 30f, "reverbMix" to 25f, "reverbSize" to 85f,
        ),
        clean(
            "Alien", PresetCategory.CHARACTERS, "Warbly and otherworldly.",
            "shift" to 4f, "formant" to 3f, "vibDepth" to 60f, "vibRate" to 8f,
            "ringMix" to 40f, "ringHz" to 300f, "combMix" to 30f, "combMs" to 1.2f, "combMod" to 60f, "combRate" to 3f,
        ),
        clean(
            "Ghost", PresetCategory.CHARACTERS, "Breathy, faint and echoing down a corridor.",
            "shift" to 2f, "formant" to 2f, "breath" to 150f, "dry" to 20f,
            "combMix" to 20f, "combMod" to 80f, "combRate" to 0.2f,
            "echoMix" to 30f, "echoMs" to 450f, "echoFb" to 55f,
            "reverbMix" to 60f, "reverbSize" to 95f, "reverbDamp" to 70f,
            "hpf" to 250f, "lpf" to 6000f, "outDb" to -2f,
        ),

        // ---- Utility ----
        clean(
            "Hard tune", PresetCategory.UTILITY, "Clean snap-to-note autotune with no effects.",
            "strength" to 100f, "retuneMs" to 0f,
        ),
        clean("Natural", PresetCategory.UTILITY, "Barely processed. Useful as a starting point."),
    )

    fun byCategory(): Map<PresetCategory, List<Preset>> = all.groupBy { it.category }

    /** The built-in preset whose settings are exactly [p], if any. */
    fun matching(p: VoiceParams): Preset? = all.firstOrNull { it.apply() == p }
}

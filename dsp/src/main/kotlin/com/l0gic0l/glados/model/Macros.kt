package com.l0gic0l.glados.model

import kotlin.math.ln
import kotlin.math.pow

/**
 * A Simple-mode slider: one 0 to 100 knob that moves several settings at once.
 * [read] says where the current settings sit on the knob; [write] returns the settings moved to a knob position.
 */
class Macro(
    val id: String,
    val label: String,
    val help: String,
    val read: (VoiceParams) -> Float,
    val write: (VoiceParams, Float) -> VoiceParams,
)

object Macros {
    /**
     * Scales several zero-based amounts together so their balance is kept. The knob sits at the largest of them
     * as a fraction of its range; from all-zero they come in at [parts]' weights.
     */
    private fun blend(id: String, label: String, help: String, vararg parts: Pair<ParamSpec, Float>): Macro {
        fun level(p: VoiceParams) = parts.maxOf { (s, _) -> p[s] / s.max }
        return Macro(
            id, label, help,
            read = { p -> level(p) * 100f },
            write = { p, v ->
                val t = (v / 100f).coerceIn(0f, 1f)
                val cur = level(p)
                parts.fold(p) { acc, (s, w) -> acc.with(s, if (cur > 0.005f) p[s] * t / cur else s.max * w * t) }
            },
        )
    }

    val AUTOTUNE = Macro(
        "autotune", "Autotune", "Left keeps your own melody. The middle pulls gently to the scale. Right snaps hard like a robot.",
        read = { p ->
            if (p[Specs.STRENGTH] < 99.5f) p[Specs.STRENGTH] / 2f
            else 50f + 50f * (1f - (p[Specs.RETUNE] / 200f).coerceIn(0f, 1f))
        },
        write = { p, v ->
            if (v <= 50f) p.with(Specs.STRENGTH, v * 2f).with(Specs.RETUNE, 200f)
            else p.with(Specs.STRENGTH, 100f).with(Specs.RETUNE, 200f * (1f - (v - 50f) / 50f))
        },
    )

    val ROBOTIC = blend(
        "robotic", "Robotic", "Ring mod, metal comb and bit crush together.",
        Specs.RING_MIX to 0.5f, Specs.COMB_MIX to 1f, Specs.CRUSH_MIX to 0.8f,
    )

    val GLITCHES = blend(
        "glitches", "Glitches", "Every kind of random malfunction together.",
        Specs.GLITCH to 1f, Specs.GLITCH_REVERSE to 0.4f, Specs.GLITCH_TAPE to 0.25f,
        Specs.GLITCH_PITCH to 0.4f, Specs.GLITCH_DROP to 0.3f, Specs.GLITCH_STATIC to 0.3f,
    )

    private const val WIDE_LOW = 70f
    private const val WIDE_HIGH = 18000f
    private const val NARROW_LOW = 500f
    private const val NARROW_HIGH = 2500f

    val TONE = Macro(
        "tone", "Radio tone", "Left is full range. Right is a thin, tinny telephone.",
        read = { p -> (ln(WIDE_HIGH / p[Specs.LPF]) / ln(WIDE_HIGH / NARROW_HIGH)).coerceIn(0f, 1f) * 100f },
        write = { p, v ->
            val t = (v / 100f).coerceIn(0f, 1f)
            p.with(Specs.HPF, WIDE_LOW * (NARROW_LOW / WIDE_LOW).pow(t)).with(Specs.LPF, WIDE_HIGH * (NARROW_HIGH / WIDE_HIGH).pow(t))
        },
    )

    val ROOM = blend("room", "Room", "Reverb and echo together.", Specs.REVERB_MIX to 1f, Specs.ECHO_MIX to 0.3f)

    val all: List<Macro> = listOf(AUTOTUNE, ROBOTIC, GLITCHES, TONE, ROOM)
}

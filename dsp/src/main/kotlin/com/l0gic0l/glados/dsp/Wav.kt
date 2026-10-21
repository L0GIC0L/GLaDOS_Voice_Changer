package com.l0gic0l.glados.dsp

import java.io.ByteArrayOutputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** Mono floating-point audio with its sample rate. */
class Audio(val samples: FloatArray, val sampleRate: Int) {
    val seconds: Double get() = samples.size.toDouble() / sampleRate
}

/** Minimal RIFF/WAVE reader and writer: PCM 8/16/24/32-bit and float32 in, 16-bit mono out. */
object Wav {
    fun read(bytes: ByteArray): Audio {
        val b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        require(bytes.size >= 12 && tag(b, 0) == "RIFF" && tag(b, 8) == "WAVE") { "Not a WAV file" }
        var pos = 12
        var format = 1
        var channels = 1
        var rate = 44100
        var bits = 16
        var dataStart = -1
        var dataLen = 0
        while (pos + 8 <= bytes.size) {
            val id = tag(b, pos)
            var len = b.getInt(pos + 4)
            val body = pos + 8
            // Streaming writers (some Android TTS engines) leave the length at 0 or -1.
            if (len < 0 || body + len > bytes.size || (id == "data" && len == 0)) len = bytes.size - body
            when (id) {
                "fmt " -> {
                    format = b.getShort(body).toInt() and 0xFFFF
                    channels = b.getShort(body + 2).toInt()
                    rate = b.getInt(body + 4)
                    bits = b.getShort(body + 14).toInt()
                    if (format == 0xFFFE && len >= 26) format = b.getShort(body + 24).toInt() and 0xFFFF
                }
                "data" -> {
                    dataStart = body
                    dataLen = len
                }
            }
            if (dataStart >= 0) break
            pos = body + len + (len and 1)
        }
        require(dataStart >= 0) { "WAV has no data chunk" }
        require(format == 1 || format == 3) { "Unsupported WAV format $format" }
        val bytesPer = bits / 8
        val frames = dataLen / (bytesPer * channels)
        val out = FloatArray(frames)
        for (i in 0 until frames) {
            var sum = 0f
            for (c in 0 until channels) {
                val o = dataStart + (i * channels + c) * bytesPer
                sum += when {
                    format == 3 && bits == 32 -> b.getFloat(o)
                    bits == 8 -> ((b.get(o).toInt() and 0xFF) - 128) / 128f
                    bits == 16 -> b.getShort(o) / 32768f
                    bits == 24 -> ((b.get(o + 2).toInt() shl 16) or ((b.get(o + 1).toInt() and 0xFF) shl 8) or (b.get(o).toInt() and 0xFF)) / 8388608f
                    bits == 32 -> b.getInt(o) / 2147483648f
                    else -> throw IllegalArgumentException("Unsupported bit depth $bits")
                }
            }
            out[i] = sum / channels
        }
        return Audio(out, rate)
    }

    fun encode(audio: Audio): ByteArray = ByteArrayOutputStream().also { write(it, audio) }.toByteArray()

    fun write(out: OutputStream, audio: Audio) {
        val n = audio.samples.size
        val data = ByteBuffer.allocate(n * 2).order(ByteOrder.LITTLE_ENDIAN)
        for (v in audio.samples) {
            data.putShort((max(-1f, min(1f, v)) * 32767f).roundToInt().toShort())
        }
        val head = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        head.put("RIFF".toByteArray()).putInt(36 + n * 2).put("WAVE".toByteArray())
        head.put("fmt ".toByteArray()).putInt(16).putShort(1).putShort(1)
        head.putInt(audio.sampleRate).putInt(audio.sampleRate * 2).putShort(2).putShort(16)
        head.put("data".toByteArray()).putInt(n * 2)
        out.write(head.array())
        out.write(data.array())
    }

    private fun tag(b: ByteBuffer, at: Int): String =
        String(ByteArray(4) { b.get(at + it) }, Charsets.US_ASCII)
}

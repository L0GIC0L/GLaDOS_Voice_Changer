package com.l0gic0l.glados.audio

import android.content.Context
import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import android.os.Build
import com.l0gic0l.glados.dsp.Audio
import java.nio.ByteOrder

class DecodedAudio(val audio: Audio, val truncated: Boolean)

/** Decodes any audio file Android can play (wav, mp3, m4a, ogg, flac) to mono float samples. */
object AudioDecoder {
    const val MAX_SECONDS = 180

    fun decode(context: Context, uri: Uri): DecodedAudio {
        val extractor = MediaExtractor()
        var codec: MediaCodec? = null
        try {
            extractor.setDataSource(context, uri, null)
            var track = -1
            var format: MediaFormat? = null
            for (i in 0 until extractor.trackCount) {
                val f = extractor.getTrackFormat(i)
                if (f.getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true) {
                    track = i
                    format = f
                    break
                }
            }
            require(track >= 0 && format != null) { "No audio track in that file." }
            extractor.selectTrack(track)

            var sampleRate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
            var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
            var encoding = AudioFormat.ENCODING_PCM_16BIT
            val decoder = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME)!!)
            codec = decoder
            decoder.configure(format, null, null, 0)
            decoder.start()

            // Mono float samples are accumulated directly, with no intermediate copy of the raw PCM.
            var samples = FloatArray(sampleRate * 10)
            var count = 0
            var truncated = false
            val info = MediaCodec.BufferInfo()
            var inputDone = false
            var outputDone = false
            var idleLoops = 0
            while (!outputDone) {
                var progressed = false
                if (!inputDone) {
                    val idx = decoder.dequeueInputBuffer(10_000)
                    if (idx >= 0) {
                        progressed = true
                        val buf = decoder.getInputBuffer(idx)!!
                        val n = extractor.readSampleData(buf, 0)
                        if (n < 0) {
                            decoder.queueInputBuffer(idx, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                            inputDone = true
                        } else {
                            decoder.queueInputBuffer(idx, 0, n, extractor.sampleTime, 0)
                            extractor.advance()
                        }
                    }
                }
                val out = decoder.dequeueOutputBuffer(info, 10_000)
                when {
                    out >= 0 -> {
                        progressed = true
                        val buf = decoder.getOutputBuffer(out)!!
                        buf.position(info.offset)
                        buf.limit(info.offset + info.size)
                        val maxFrames = MAX_SECONDS * sampleRate
                        count = appendMono(buf.order(ByteOrder.nativeOrder()), channels, encoding, maxFrames, samples, count) { needed ->
                            samples = samples.copyOf(maxOf(needed, samples.size * 2).coerceAtMost(maxFrames))
                            samples
                        }
                        if (count >= maxFrames) {
                            truncated = true
                            outputDone = true
                        }
                        decoder.releaseOutputBuffer(out, false)
                        if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) outputDone = true
                    }
                    out == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        progressed = true
                        val f = decoder.outputFormat
                        sampleRate = f.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                        channels = f.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
                        if (Build.VERSION.SDK_INT >= 24 && f.containsKey(MediaFormat.KEY_PCM_ENCODING)) {
                            encoding = f.getInteger(MediaFormat.KEY_PCM_ENCODING)
                        }
                    }
                }
                // A decoder that never produces anything would otherwise spin here forever.
                idleLoops = if (progressed) 0 else idleLoops + 1
                check(idleLoops < 500) { "The decoder stopped responding." }
            }
            require(count > 0) { "That file contains no audio." }
            return DecodedAudio(Audio(samples.copyOf(count), sampleRate), truncated)
        } finally {
            runCatching { codec?.stop() }
            runCatching { codec?.release() }
            extractor.release()
        }
    }

    /** Downmixes one decoder output buffer to mono floats and appends it. Returns the new sample count. */
    private fun appendMono(
        buf: java.nio.ByteBuffer, channels: Int, encoding: Int, maxFrames: Int,
        into: FloatArray, count: Int, grow: (Int) -> FloatArray,
    ): Int {
        val bytesPerSample = when (encoding) {
            AudioFormat.ENCODING_PCM_FLOAT -> 4
            AudioFormat.ENCODING_PCM_8BIT -> 1
            else -> 2
        }
        val frames = buf.remaining() / (bytesPerSample * channels)
        var out = into
        var n = count
        val take = minOf(frames, maxFrames - n)
        if (take <= 0) return n
        if (n + take > out.size) out = grow(n + take)
        for (f in 0 until take) {
            var sum = 0f
            for (c in 0 until channels) {
                sum += when (encoding) {
                    AudioFormat.ENCODING_PCM_FLOAT -> buf.float
                    AudioFormat.ENCODING_PCM_8BIT -> ((buf.get().toInt() and 0xFF) - 128) / 128f
                    else -> buf.short / 32768f
                }
            }
            out[n++] = sum / channels
        }
        return n
    }
}

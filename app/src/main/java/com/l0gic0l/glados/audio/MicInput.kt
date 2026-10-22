package com.l0gic0l.glados.audio

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Build

/** Opens the microphone, preferring the low-latency voice-performance source and falling back to the plain mic. */
object MicInput {
    @SuppressLint("MissingPermission") // callers check RECORD_AUDIO first
    fun open(sampleRate: Int, bufferBytes: Int): AudioRecord? {
        val sources = buildList {
            if (Build.VERSION.SDK_INT >= 29) add(MediaRecorder.AudioSource.VOICE_PERFORMANCE)
            add(MediaRecorder.AudioSource.MIC)
        }
        for (source in sources) {
            val record = try {
                AudioRecord.Builder()
                    .setAudioSource(source)
                    .setAudioFormat(
                        AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setSampleRate(sampleRate)
                            .setChannelMask(AudioFormat.CHANNEL_IN_MONO).build(),
                    )
                    .setBufferSizeInBytes(bufferBytes)
                    .build()
            } catch (e: Exception) {
                null
            }
            if (record != null && record.state == AudioRecord.STATE_INITIALIZED) return record
            record?.release()
        }
        return null
    }
}

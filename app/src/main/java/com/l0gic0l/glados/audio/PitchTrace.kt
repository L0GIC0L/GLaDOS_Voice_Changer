package com.l0gic0l.glados.audio

/**
 * Rolling history of detected and corrected pitch for the live view. Written by the audio thread,
 * read by the UI; a torn read only costs one slightly wrong dot, so no locking.
 */
class PitchTrace(val size: Int = 400) {
    private val input = FloatArray(size)
    private val output = FloatArray(size)

    @Volatile
    private var head = 0

    fun push(inHz: Float, outHz: Float) {
        val h = head
        input[h % size] = inHz
        output[h % size] = outHz
        head = h + 1
    }

    fun clear() {
        input.fill(0f)
        output.fill(0f)
    }

    /** Copies the history, oldest first, into the given arrays (each of length [size]). */
    fun snapshot(inHz: FloatArray, outHz: FloatArray) {
        val h = head
        for (i in 0 until size) {
            val src = ((h + i) % size + size) % size
            inHz[i] = input[src]
            outHz[i] = output[src]
        }
    }
}

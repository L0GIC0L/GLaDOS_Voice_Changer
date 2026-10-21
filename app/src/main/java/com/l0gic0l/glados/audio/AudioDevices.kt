package com.l0gic0l.glados.audio

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager

class AudioDevice(val info: AudioDeviceInfo?, val label: String) {
    val id: Int get() = info?.id ?: 0
}

object AudioDevices {
    private fun typeName(type: Int): String = when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_MIC -> "Built-in mic"
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "Speaker"
        AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "Earpiece"
        AudioDeviceInfo.TYPE_WIRED_HEADSET -> "Wired headset"
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "Wired headphones"
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> "Bluetooth (call)"
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> "Bluetooth"
        AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_ACCESSORY -> "USB"
        else -> "Audio device"
    }

    private fun list(context: Context, flag: Int, allowed: Set<Int>): List<AudioDevice> {
        val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val found = am.getDevices(flag).filter { it.type in allowed }.map {
            val name = it.productName?.toString().orEmpty()
            val type = typeName(it.type)
            AudioDevice(it, if (name.isBlank() || name == type) type else "$type ($name)")
        }
        return listOf(AudioDevice(null, "Default")) + found
    }

    fun inputs(context: Context): List<AudioDevice> = list(
        context, AudioManager.GET_DEVICES_INPUTS,
        setOf(
            AudioDeviceInfo.TYPE_BUILTIN_MIC, AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
            AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_USB_HEADSET,
        ),
    )

    fun outputs(context: Context): List<AudioDevice> = list(
        context, AudioManager.GET_DEVICES_OUTPUTS,
        setOf(
            AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioDeviceInfo.TYPE_BUILTIN_EARPIECE, AudioDeviceInfo.TYPE_WIRED_HEADSET,
            AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
            AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_USB_HEADSET,
        ),
    )

    fun find(devices: List<AudioDevice>, id: Int): AudioDeviceInfo? = devices.firstOrNull { it.id == id }?.info
}

# GLaDOS Voice Changer

An Android app that turns your voice, or typed text, into GLaDOS. It has three ways to work and one set of controls behind all of them:

- **Live**: microphone in, GLaDOS out, in real time, with a scrolling pitch view.
- **Speak**: type text and have it spoken by the phone's text-to-speech voice, then pitch-corrected and processed like the live voice.
- **Editor**: a Melodyne-style note editor. Record, import or generate a clip and every syllable becomes a note you can drag to any pitch, stretch, retune to a scale, or change in formant and level.
- **Voice**: every setting in the app (46 of them), grouped, with presets. Nothing is hidden or fixed.

The UI is Jetpack Compose with Material 3. On Android 12+ it uses your wallpaper colors (Material You); otherwise, or if you switch it off, an Aperture-orange scheme. Light, dark and system modes are in the palette button at the top right.

## Build and run

Requirements: Android Studio (current stable) or JDK 17+ and the Android SDK with platform 36.

```
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Or open the folder in Android Studio and press Run. Minimum Android version is 8.0 (API 26).

## Using it

**Live.** Tap *Go live*, grant the microphone permission, and speak. Use headphones, otherwise the speaker feeds back into the mic. The service keeps running with the screen off. You can pick the microphone and output device, hear the unprocessed voice with *bypass*, and adjust key, scale, retune speed, strength, pitch and formants right on the screen. The grey dots in the pitch view are your voice; the bold line is the corrected pitch.

**Speak.** Type text, choose a voice from whatever speech engine your phone has, and tap *Speak*. By default each syllable is snapped to the Tuning settings, so the speech sounds like the live voice. Turn that off to keep the engine's own melody, or open the clip in the Editor to sing it.

**Editor.**
- *Pitch tool*: tap notes to select, drag up or down to change pitch. *Snap to scale* keeps notes in key.
- *Timing tool*: drag a note's edge to stretch it, or its middle to slide it.
- *Split tool*: tap inside a note to cut it. *Join* merges selected neighbours.
- Pinch to zoom time, drag empty space to scroll, tap the ruler to move the playhead.
- Per-note sliders: pitch, drift, vibrato and modulation, formant, level.
- *Correct pitch* is the Melodyne macro: pull notes toward the scale, remove drift, remove modulation. *Match live settings* applies the Tuning settings from the Voice tab.
- Undo and redo, record from the mic, import any audio file (wav, mp3, m4a, ogg, flac), save as WAV.

Effects are applied in real time during playback, so you can move sliders in the Voice tab while a clip plays.

**Voice.** The groups are Tuning (key, scale including custom notes, retune speed, strength, note hold, pitch shift, register matching, monotone, vibrato), Voice (formants, detection range, noise gate, breath and consonants, dry mix), Robot (ring mod, metallic comb and flange, bit crush, glitch), Space (echo, reverb) and EQ and output. Double-tap a slider's name to reset it. Presets: Portal 1, Portal 2, PotatOS, Hard tune, Natural. You can save your own.

## If it is not picking up your voice

The Live tab shows *Voice detected* when pitch is being tracked, and warns when the mic hears sound but no pitched voice. The Editor's *Voice detection* card shows how much of a clip was found. If detection is poor:

- Raise **Voicing sensitivity** (Voice tab). Higher means more sound counts as voice. The default suits normal speech; quiet, noisy, breathy or echoey recordings want it higher.
- Check **Lowest pitch** and **Highest pitch** cover your voice (defaults 85 and 700 Hz).
- Lower the **Noise gate** if the mic level is low.
- Speak closer to the mic and away from fans and traffic. Low rumble and hiss are filtered out for pitch tracking, but loud room echo is still hard on any pitch tracker.

In the Editor, changing these settings re-analyses the clip automatically as long as you have not edited it yet.

## How it works

The DSP is plain Kotlin in the `dsp` module, with no Android dependencies, so it is unit-tested on the JVM.

- **Pitch tracking**: YIN on a decimated copy of the signal, refined at full rate. The tracker listens to a band-passed copy (rumble and hiss removed) that is never heard, and keeps a note voiced through brief dips, so phone recordings work.
- **Pitch correction**: TD-PSOLA. Short windowed grains are cut from the input at pitch marks and laid down at the spacing of the *target* pitch. That changes pitch while keeping the original vocal tract, so voices stay natural instead of chipmunking. Grains can also be stretched or squeezed to move the formants independently.
- **Tuner**: snaps to the nearest allowed note with hysteresis. Retune speed smooths the *correction*, not the pitch, so 0 ms gives hard robotic steps and larger values let natural glides through.
- **Register matching**: measures your average pitch and shifts it to a target (default 215 Hz), and moves the formants part of the way with it, so a deep voice lands near GLaDOS without manual tuning.
- **Live engine**: the same algorithm run causally. Algorithmic latency is about 40 ms at the default voice range; the live screen shows the total including the audio device.
- **Editor engine**: analyses the whole clip once, splits it into notes at pitch steps and loudness dips, and re-renders in tens of milliseconds as you edit.
- **Effects**: ring mod, modulated comb, bit crusher, random stutter, EQ, echo, Freeverb reverb and a limiter, shared by live, playback and export.

Source layout:

```
dsp/    pure Kotlin: pitch tracking, PSOLA, tuner, analysis, note editing, effects, WAV, parameter schema
app/    Android: audio I/O, foreground service, text-to-speech, Compose UI, theme
```

All controls are generated from one parameter schema (`dsp/.../model/Params.kt`), so adding a parameter is a single entry there plus its use in the DSP.

## Tests

```
./gradlew :dsp:test                # DSP: pitch accuracy, hard tune, shifts, formants, effects, editor operations
./gradlew :app:testDebugUnitTest   # UI on the JVM (Robolectric): screens render, real touch drags edit notes
```

Optional environment variables for deeper checks: `GLADOS_TEST_DIR` (a folder of WAV speech files for the speech smoke tests, which write processed audio to `<dir>/out`), `GLADOS_SHOT_DIR` (where the UI tests save screenshots) and `GLADOS_TEST_WAV` (a clip for the editor screenshots). Without them those parts are skipped.

## Limits worth knowing

- Android does not let one app feed another app's microphone, so this cannot make your voice GLaDOS in a call or in a game. It plays the changed voice out of the phone (or a Bluetooth or USB audio device).
- Latency depends on the device. Phones with a low-latency audio path (most recent ones) are noticeably better. Raising *Lowest pitch* in the Voice tab lowers the algorithmic delay.
- Speech quality follows whichever text-to-speech engine and voice is installed.
- Pitch tracking works on one voice at a time. Noise, music behind the voice, or whispering will confuse it; the noise gate and voicing sensitivity help.
- The app has been built, linted and tested on a development machine, including the UI on a simulated phone. It has **not** been run on a physical device yet, so real-world latency and audio-route behaviour are unverified.

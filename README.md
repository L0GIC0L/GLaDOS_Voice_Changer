# GLaDOS Voice Changer

An Android app that turns a recording of your voice, or typed text, into GLaDOS (or one of 45 other voices). It has three tabs and one set of controls behind all of them:

- **Speak**: type text and have it spoken by the phone's text-to-speech voice, then pitch-corrected and processed.
- **Editor**: a Melodyne-style note editor. Record, import or generate a clip and every syllable becomes a note you can drag to any pitch, stretch, retune to a scale, or change in formant and level.
- **Voice**: a preset library, a *Simple* view with a handful of combined sliders, and an *Advanced* view with every setting (67 of them), grouped. Nothing is hidden or fixed.

The UI is Jetpack Compose with Material 3. On Android 12+ it uses your wallpaper colors (Material You); otherwise, or if you switch it off, an Aperture-orange scheme. Light, dark and system modes are in the palette button at the top right.

## Build and run

Requirements: Android Studio (current stable) or JDK 17+ and the Android SDK with platform 36.

```
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Or open the folder in Android Studio and press Run. Minimum Android version is 8.0 (API 26).

## Using it

**Speak.** Type text, choose a voice from whatever speech engine your phone has, and tap *Speak*. By default each syllable is snapped to the Tuning settings. Turn *Tune speech to the scale* off to keep the engine's own melody, or open the clip in the Editor to sing it.

**Editor.**
- *Pitch tool*: tap notes to select, drag up or down to change pitch. *Snap to scale* keeps notes in key.
- *Timing tool*: drag a note's edge to stretch it, or its middle to slide it.
- *Split tool*: tap inside a note to cut it. *Join* merges selected neighbours.
- Pinch to zoom time, drag empty space to scroll, tap the ruler to move the playhead.
- Per-note sliders: pitch, drift, vibrato and modulation, formant, level.
- *Correct pitch* is the Melodyne macro: pull notes toward the scale, remove drift, remove modulation. *Use Voice tab tuning* applies the Tuning settings from the Voice tab.
- Undo and redo, record from the mic, import any audio file (wav, mp3, m4a, ogg, flac), save as WAV.

**Noise reduction.** Recordings and imported files are cleaned of steady background noise (hiss, fans, hum) before the voice is processed. The *Noise reduction* slider is in Simple mode, in the Editor's *Clean-up and detection* card and in the Voice group; 50% is the default, 0% turns it off. It learns the noise from the quietest moments of the clip, so leave a second of silence before or after you speak, and follows the noise level as it slowly rises and falls (a fan, the phone's automatic gain). Generated speech is already clean and is skipped. Presets never change this setting, or the pitch range and voicing sensitivity, because those describe the recording rather than the voice.

Effects are applied in real time during playback, so you can change presets or move sliders while a clip plays. Until you edit a note by hand, a new preset also re-tunes the clip; after that your edits are kept and *Use Voice tab tuning* re-applies the preset's tuning on demand.

**Presets.** Tap the preset card (on every tab) to open the library. Tapping a preset applies it straight away and the dialog has a play button, so you can audition them one after another. *Save current...* stores your own.

| Category | Presets |
|---|---|
| GLaDOS | Portal 1, Portal 2, PotatOS, Corrupted, Waking up, Singing, Monotone, Malfunctioning, Final boss |
| Aperture Science | Announcer, Portal 2 announcer, Old Aperture tape, Turret choir, Hive mind, Emergency alert, Turret, Defective turret, Personality core, Space core |
| Robots and radios | Classic robot, Vocoder, 8-bit computer, Low battery, Jet robot, Drunk robot, Radio transmission, Telephone, Megaphone, Stadium announcer, Old radio broadcast, Broken record, Underwater |
| Characters | Chipmunk, Giant, Movie trailer, Demon, Alien, Ghost, Fast talker, Slow motion, Cartoon villain, Fairy, Haunted doll |
| Utility | Hard tune, Double track, Natural |

The GLaDOS presets keep your own pitch; their character comes from the tuning, formants and effects. Monotone holds the scale note nearest your own average pitch. Turret, Defective turret and Space core move the voice to a set register on purpose.

**Simple mode.** Noise reduction, Pitch shift, Formant shift, Speed, Random speed changes, Harmony voice and Output level, plus six sliders that each move several settings at once: *Autotune* (strength and retune speed), *Robotic* (ring mod, metal comb, bit crush), *Glitches* (every glitch type), *Movement* (chorus, phaser, tremolo, tape warble), *Radio tone* (low and high cut together) and *Room* (reverb and echo). The blend sliders keep the balance the preset set, so a preset's character survives turning it up or down.

**Advanced mode.** The groups are:
- *Tuning*: key, scale including custom notes, retune speed, strength, pitch shift, register matching (off unless a preset uses it), monotone (at your own pitch or a chosen note), vibrato.
- *Voice*: noise reduction, formants, detection range, voicing sensitivity, breath and consonants, rasp, dry blend.
- *Speed and harmony*: speed (50 to 200%, pitch unchanged), random speed changes, and a harmony voice at any interval.
- *Robot*: distortion, ring mod, metallic comb and flange, bit crush.
- *Glitches*: stutter (rate and length), reverse, tape stop, pitch jumps (rate and size), dropouts, static bursts, record crackle, and an electrical buzz with its own pitch.
- *Movement*: chorus, phaser (with rate), tremolo (with rate), tape warble.
- *Space*: echo and reverb.
- *EQ and output*.

Double-tap a slider's name to reset it.

## If it is not picking up your voice

The Editor's *Voice detection* card shows how much of a clip was found. If detection is poor:

- Raise **Voicing sensitivity**. Higher means more sound counts as voice. The default suits normal speech; quiet, noisy, breathy or echoey recordings want it higher.
- Check **Lowest pitch** and **Highest pitch** cover your voice (defaults 85 and 700 Hz).
- Record closer to the mic and away from fans and traffic. Low rumble and hiss are filtered out for pitch tracking, but loud room echo is still hard on any pitch tracker.

Changing these settings re-analyses the clip automatically as long as you have not edited it yet.

## How it works

The DSP is plain Kotlin in the `dsp` module, with no Android dependencies, so it is unit-tested on the JVM.

- **Pitch tracking**: YIN on a decimated copy of the signal, refined at full rate. The tracker listens to a band-passed copy (rumble and hiss removed) that is never heard, and keeps a note voiced through brief dips, so phone recordings work.
- **Pitch correction**: TD-PSOLA. Short windowed grains are cut from the input at pitch marks and laid down at the spacing of the *target* pitch. That changes pitch while keeping the original vocal tract, so voices stay natural instead of chipmunking. Grains can also be stretched or squeezed to move the formants independently.
- **Speed**: the renderer follows a speed map from the edited timeline to the output, so speed changes without changing pitch and no words are dropped. Random speed changes pick stretches of a third of a second to a second and glide them faster or slower; the map is the same every time the clip is rendered, and the editor's playhead goes through it so it stays on the notes.
- **Harmony**: the clip is rendered a second time at the chosen interval and mixed in.
- **Editor engine**: analyses the whole clip once, splits it into notes at pitch steps and loudness dips, and re-renders in tens of milliseconds as you edit. Retune speed controls how much of each note's own drift and wobble survives, so 0 ms gives hard robotic steps.
- **Register matching** (optional): measures your average pitch and shifts it to a target register, moving the formants part of the way with it. Off by default; the turret and core presets use it.
- **Noise reduction**: short-time spectra; a noise profile averaged from the clip's quietest frames, whose level is then followed through the clip by a speech-presence-probability tracker (Gerkmann and Hendriks) run forwards and backwards; and a decision-directed Wiener gain (Ephraim and Malah) refined by the second step of Plapous's two-step method, which removes noise without the warbling "musical noise" of plain spectral subtraction. The slider sets how far noise may be pushed down (up to 30 dB). On real speech with noise added at 5 dB SNR, 50% raises the SNR by about 5 to 7 dB, and a clean recording changes by less than -45 dB. `DenoiseBenchTest` measures this.
- **Effects**: ring mod, rasp, distortion, modulated comb, bit crusher, glitches, buzz, tape warble, crackle, EQ, chorus, phaser, tremolo, echo, Freeverb reverb and a limiter, shared by playback and export. Rasp makes the level flutter irregularly at 50 to 170 Hz, the cycle-to-cycle unsteadiness of a gravelly voice, with a little breath noise. The glitches are random events that fire only while there is voice, one at a time: a stutter loops a scrap, a reverse plays the last moment backwards, a tape stop slows the audio to a halt, a pitch jump reads the recent past faster or slower, a dropout mutes, and a static burst adds crackle.

Source layout:

```
dsp/    pure Kotlin: pitch tracking, PSOLA, analysis, note editing, effects, WAV, parameters, presets, Simple-mode sliders
app/    Android: audio I/O, text-to-speech, Compose UI, theme
```

All controls are generated from one parameter schema (`dsp/.../model/Params.kt`), so adding a parameter is a single entry there plus its use in the DSP. Presets are in `Presets.kt` and the Simple-mode sliders in `Macros.kt`.

## Tests

```
./gradlew :dsp:test                # DSP: pitch accuracy, hard tune, shifts, formants, effects, glitches, presets, editor operations
./gradlew :app:testDebugUnitTest   # UI on the JVM (Robolectric): screens render, real touch drags edit notes, presets and Simple mode
```

Optional environment variables for deeper checks: `GLADOS_TEST_DIR` (a folder of WAV speech files for the speech smoke test, which writes processed audio to `<dir>/out`), `GLADOS_REAL_DIR` (real recordings for the detection tests and the noise reduction benchmark, `DenoiseBenchTest`), `GLADOS_SHOT_DIR` (where the UI tests save screenshots) and `GLADOS_TEST_WAV` (a clip for the editor screenshots). Without them those parts are skipped.

## Limits worth knowing

- There is no real-time mode: record or import a clip, or type text, and it is processed after the fact.
- Speech quality follows whichever text-to-speech engine and voice is installed.
- Pitch tracking works on one voice at a time. Noise, music behind the voice, or whispering will confuse it; voicing sensitivity helps.
- Noise reduction handles steady noise and noise that slowly gets louder or quieter. Sudden sounds (a door, a keyboard, other voices) are left in, and a sudden jump in the background (a fan switching on mid-sentence) is only partly removed.

# GLaDOS Voice for Linux

The Linux version of the GLaDOS voice changer: Rust, GTK4 and libadwaita. It has everything the Android app has, plus live mode.

- **Live**: microphone in, GLaDOS out, in real time, with level meters, a scrolling pitch view, bypass and mute, and a choice of microphone and output device.
- **Speak**: type text, have it spoken by eSpeak NG (or Piper, if installed), then tuned and processed.
- **Editor**: the Melodyne-style note editor. Record, import (wav, mp3, flac, ogg, m4a and more) or generate a clip, then drag every syllable to any pitch, stretch it, split or join notes, retune to a scale, and change formant and level. Undo and redo, save as WAV.
- **Voice**: the 46 presets, a Simple view with combined sliders, and an Advanced view with every setting.

Settings are saved to `~/.config/glados-voice/settings.json`.

## Install and run

You need Rust (1.92 or newer; install with [rustup](https://rustup.rs)) and the GTK4, libadwaita and ALSA development packages. GTK 4.14 and libadwaita 1.5 or newer are required (Ubuntu 24.04, Debian 13, Fedora 40, Arch and later).

```
# Debian / Ubuntu
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libasound2-dev espeak-ng
# Fedora
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libadwaita-devel alsa-lib-devel espeak-ng
# Arch
sudo pacman -S base-devel gtk4 libadwaita alsa-lib espeak-ng

cd linux
cargo run --release
```

To add it to your applications menu: `cargo build --release && data/install.sh` (installs to `~/.local`). `glados-voice some-file.mp3`, or *Open with* in your file manager, opens a recording straight in the editor.

Speech uses `espeak-ng`, which sounds robotic and makes a good GLaDOS base. For natural voices, install [Piper](https://github.com/rhasspy/piper) so that `piper` is on your PATH, download a voice model (`.onnx` plus its `.onnx.json`), and pick it on the Speak tab.

## Live mode

1. Plug in headphones. Speakers would feed the processed voice straight back into the microphone.
2. On the Live tab, choose the microphone and output (System default follows your desktop's sound settings), then press **Go live**.
3. The meters show what comes in and goes out; the pitch view shows your voice as grey dots and the tuned pitch as a line. "Voice detected" appears while pitch is being tracked.

The app adds about 57 ms of delay at the default settings (shown on the Live tab): 42 ms for the pitch engine, which needs a few cycles of your lowest note to measure it, and 15 ms of microphone audio kept in hand. The sound system adds its own on top. Raising **Lowest pitch** (Voice tab, Voice group) lowers the delay: about 45 ms at 120 Hz, which suits most voices above a low bass. If the microphone's audio arrives unevenly the app keeps more in hand, a step at a time and only when a dropout happens, so the delay shown can grow a little. Speed, random speed changes, harmony and noise reduction work on recordings and speech only; live mode has a noise gate instead.

To use the GLaDOS voice in another app (a call, a game, OBS) on PipeWire or PulseAudio, create a virtual microphone and send the output there:

```
pactl load-module module-null-sink sink_name=glados sink_properties=device.description=GLaDOS
pactl load-module module-remap-source master=glados.monitor source_name=glados_mic source_properties=device.description=GLaDOS-Microphone
```

Then, while live, move the app's output to the GLaDOS sink (in pavucontrol, Helvum, or your desktop's sound settings) and pick "GLaDOS-Microphone" as the microphone in the other app. `pactl unload-module module-remap-source; pactl unload-module module-null-sink` removes them again.

## Layout

```
glados-dsp/     the sound engine, no dependencies: pitch tracking, PSOLA, notes, effects, noise reduction, live engine,
                settings schema and presets (ported from the Android app's Kotlin, with its tests)
glados-voice/   the app: GTK4/libadwaita UI, audio through cpal (ALSA, which reaches PipeWire and PulseAudio),
                decoding through symphonia, speech through espeak-ng or piper
data/           desktop entry, icon and install script
```

## Tests

```
cargo test                                   # 86 DSP tests and the app's tests; the end-to-end app test needs a display (Xvfb works)
cargo run --release -- --selftest-live 5 "Portal 2"   # five seconds of live mode on the default devices, no window
```

`GLADOS_VOICE_PAGE=speak|editor|voice` opens on another tab, and `GLADOS_VOICE_CONFIG_DIR` keeps settings somewhere else, which helps when taking screenshots or testing.

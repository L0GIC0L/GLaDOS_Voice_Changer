#!/bin/sh
# Installs GLaDOS Voice for the current user: the program, its menu entry and its icon.
# Run from the linux/ folder after `cargo build --release`, or point BIN at a downloaded binary.
set -e
here=$(cd "$(dirname "$0")" && pwd)
bin=${BIN:-"$here/../target/release/glados-voice"}
prefix=${PREFIX:-"$HOME/.local"}
install -Dm755 "$bin" "$prefix/bin/glados-voice"
install -Dm644 "$here/io.github.l0gic0l.GladosVoice.desktop" "$prefix/share/applications/io.github.l0gic0l.GladosVoice.desktop"
install -Dm644 "$here/io.github.l0gic0l.GladosVoice.svg" "$prefix/share/icons/hicolor/scalable/apps/io.github.l0gic0l.GladosVoice.svg"
echo "Installed to $prefix. Make sure $prefix/bin is on your PATH."

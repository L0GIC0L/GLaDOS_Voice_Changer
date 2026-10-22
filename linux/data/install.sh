#!/bin/sh
# Installs GLaDOS Voice for the current user: the program, its menu entry and its icon.
# Run from the linux/ folder after `cargo build --release`, from an unpacked download, or point BIN at a binary.
set -e
here=$(cd "$(dirname "$0")" && pwd)
bin=${BIN:-"$here/../target/release/glados-voice"}
if [ -z "$BIN" ] && [ ! -x "$bin" ] && [ -x "$here/../glados-voice" ]; then
    bin="$here/../glados-voice"
fi
prefix=${PREFIX:-"$HOME/.local"}
install -Dm755 "$bin" "$prefix/bin/glados-voice"
# The menu entry names the full path, so it works even where $prefix/bin is not on the desktop's PATH.
mkdir -p "$prefix/share/applications"
sed "s|^Exec=glados-voice|Exec=$prefix/bin/glados-voice|" "$here/io.github.l0gic0l.GladosVoice.desktop" \
    > "$prefix/share/applications/io.github.l0gic0l.GladosVoice.desktop"
install -Dm644 "$here/io.github.l0gic0l.GladosVoice.svg" "$prefix/share/icons/hicolor/scalable/apps/io.github.l0gic0l.GladosVoice.svg"
echo "Installed to $prefix. Make sure $prefix/bin is on your PATH."

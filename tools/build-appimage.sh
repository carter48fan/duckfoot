#!/usr/bin/env bash
# Package the desktop app as a Linux AppImage. CI's linux job runs this too.
#
#   tools/build-appimage.sh                  # cargo build --release, then package
#   tools/build-appimage.sh path/to/duckfoot # package an already-built binary
#
# → dist/duckfoot-linux-x86_64.AppImage
#
# The AppImage runs only on systems with this machine's glibc or newer: CI builds in
# ubuntu:22.04 so its AppImage reaches older distros; a local build is for local use.
# appimagetool is downloaded once into target/appimagetool and run extracted, so FUSE
# is not needed.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ $# -ge 1 ]]; then
    bin="$1"
else
    cargo build --release --locked -p duckfoot
    bin=target/release/duckfoot
fi

tool=target/appimagetool
if [[ ! -x "$tool/squashfs-root/AppRun" ]]; then
    mkdir -p "$tool"
    curl -sSfL -o "$tool/appimagetool" \
        https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$tool/appimagetool"
    (cd "$tool" && ./appimagetool --appimage-extract >/dev/null)
fi

appdir=target/AppDir
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/256x256/apps" dist
cp "$bin" "$appdir/usr/bin/duckfoot"
strip "$appdir/usr/bin/duckfoot"
cp brand/duckfoot-1024.png "$appdir/duckfoot.png"
cp brand/duckfoot-1024.png "$appdir/usr/share/icons/hicolor/256x256/apps/duckfoot.png"
cp brand/duckfoot.desktop "$appdir/"
cp brand/duckfoot.desktop "$appdir/usr/share/applications/"
cp brand/AppRun "$appdir/AppRun"
chmod +x "$appdir/AppRun"
cp THIRD_PARTY_LICENSES.txt "$appdir/"

ARCH=x86_64 "$tool/squashfs-root/AppRun" "$appdir" dist/duckfoot-linux-x86_64.AppImage
echo "Built dist/duckfoot-linux-x86_64.AppImage"

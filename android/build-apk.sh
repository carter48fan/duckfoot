#!/usr/bin/env bash
# Build the Rust library for Android and package it into an installable APK.
#
#   android/build-apk.sh            # → android/app/build/outputs/apk/debug/app-debug.apk
#
# Needs: rustup, an Android SDK (ANDROID_HOME) and NDK (ANDROID_NDK_HOME), JDK 17, and
# `cargo install cargo-ndk`. The Rust side is always an optimised build — a debug build
# of pixel code is unusably slow — whatever the APK's own build type.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-}}"
if [[ -z "$ANDROID_NDK_HOME" ]]; then
    echo "Set ANDROID_NDK_HOME to an Android NDK (r26 or newer)." >&2
    exit 1
fi
if ! command -v cargo-ndk >/dev/null; then
    echo "cargo-ndk is missing: cargo install cargo-ndk" >&2
    exit 1
fi

rustup target add aarch64-linux-android
cd "$root"
# API 30 matches minSdk in app/build.gradle.kts.
cargo ndk --target arm64-v8a --platform 30 \
    --output-dir android/app/src/main/jniLibs \
    build --release -p duckfoot-android

cd android
./gradlew --no-daemon assembleDebug
apk="$root/android/app/build/outputs/apk/debug/app-debug.apk"

# The library is packaged unstripped (AGP cannot find its pinned NDK to strip with), which
# keeps function names in crash tombstones while the port is young; ~29 MB, ~10 MB zipped.
#
# "BUILD SUCCESSFUL" does not mean the app can start: check the APK carries the library
# and that it exports the entry point NativeActivity looks up.
lib=lib/arm64-v8a/libduckfoot_android.so
nm=("$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin/llvm-nm)
if ! unzip -l "$apk" "$lib" >/dev/null; then
    echo "$apk has no $lib" >&2
    exit 1
fi
unzip -p "$apk" "$lib" > "$root/target/apk-check.so"
# Captured first: with pipefail, `nm | grep -q` fails when grep exits before nm is done.
exported="$("${nm[0]}" -D --defined-only "$root/target/apk-check.so")"
if ! grep -q ' ANativeActivity_onCreate$' <<<"$exported"; then
    echo "$lib does not export ANativeActivity_onCreate" >&2
    exit 1
fi
echo "APK: $apk ($(du -h "$apk" | cut -f1); $lib $(du -h "$root/target/apk-check.so" | cut -f1))"

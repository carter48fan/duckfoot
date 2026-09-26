#!/usr/bin/env bash
# Build the Rust library for Android and package it into an installable APK.
#
#   android/build-apk.sh            # → android/app/build/outputs/apk/debug/app-debug.apk
#   android/build-apk.sh release    # → android/app/build/outputs/apk/release/app-release.apk
#
# Needs: rustup, an Android SDK (ANDROID_HOME) and NDK (ANDROID_NDK_HOME), JDK 17, and
# `cargo install cargo-ndk`. The Rust side is always an optimised build — a debug build
# of pixel code is unusably slow — whatever the APK's own build type.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
# debug is debuggable (run-as, a debugger); release is what gets shipped. Both are signed
# with the same key; see app/build.gradle.kts.
variant="${1:-debug}"
case "$variant" in
    debug) task=assembleDebug ;;
    release) task=assembleRelease ;;
    *) echo "usage: build-apk.sh [debug|release]" >&2; exit 2 ;;
esac
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
# C dependencies (libwebp) are compiled by cc-rs, which looks for the NDK's clang under a
# name cargo-ndk does not always export. Name it, for the same API level as the link.
ndk_bin=("$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin)
export CC_aarch64_linux_android="${ndk_bin[0]}/aarch64-linux-android30-clang"
export CXX_aarch64_linux_android="${ndk_bin[0]}/aarch64-linux-android30-clang++"
export AR_aarch64_linux_android="${ndk_bin[0]}/llvm-ar"
cd "$root"
# API 30 matches minSdk in app/build.gradle.kts.
cargo ndk --target arm64-v8a --platform 30 \
    --output-dir android/app/src/main/jniLibs \
    build --release -p duckfoot-android

cd android
./gradlew --no-daemon "$task"
apk="$root/android/app/build/outputs/apk/$variant/app-$variant.apk"

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

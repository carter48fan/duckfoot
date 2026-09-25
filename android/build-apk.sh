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
echo "APK: $root/android/app/build/outputs/apk/debug/app-debug.apk"

# Duckfoot for Android

The Gradle project that packages `crates/duckfoot-android` as an APK. Everything the app
draws is the same Rust as the desktop; `DuckfootActivity.java` only handles the system
file picker, "Open with", and the system bars.

## Build

```bash
cargo install cargo-ndk
android/build-apk.sh            # needs ANDROID_HOME, ANDROID_NDK_HOME and JDK 17
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

Every push also builds a release APK, and a Linux binary, in the **Build** workflow
(`.github/workflows/build.yml`); they are attached to the run as the `duckfoot-android`
and `duckfoot-linux-x86_64` artifacts.

## Nightly

Every push to `main` replaces the **nightly** release with that commit's APK
(`duckfoot-nightly-android-arm64.apk`) and Linux build
(`duckfoot-nightly-linux-x86_64.tar.gz`). It is a draft, so only collaborators see it,
until the repository variable `PUBLIC_NIGHTLY` is set to `true` (**Settings → Secrets and
variables → Actions → Variables**); from then on it is a public pre-release.

## Release

Either run the workflow by hand — **Actions → Build → Run workflow**, pick the branch,
and type a version such as `v0.1.0-alpha.1` — or push a tag that starts with `v`:

```bash
git tag v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
```

Both build that commit and **draft** a pre-release named after the version, with
the APK and the Linux build attached. A draft is visible only to the repository's
collaborators, under **Releases**, where the APK downloads like any other file. Pressing
**Publish** makes it public (and, for a hand-run, creates the tag on the commit it was
built from), which the licence note in the top-level README says to hold off on for now.

The version sets the app's version name (`v0.1.0-alpha.1` → `0.1.0-alpha.1`); the workflow's
run number is its version code, so every release is newer than the one before.

## Test builds

Every push to a branch other than `main` publishes that branch's rolling pre-release,
**Test build: `<branch>`** on the repository's Releases page, with the APK as a plain
file: open the release on the phone, tap the `.apk`, install. Each push replaces it.

Android installs an update only over an app signed with the same key. Until the
repository has a key of its own (below), every build - CI's on any branch, and local
ones - is signed with the **testing key** checked in as `android/testing.jks`, so each
test build installs over the last without losing the app's library and edits. It is a
key for testing only: anyone with the repository can sign with it. `build-apk.sh` fails
a build whose APK is not signed with the key it should be.

If a build from before the testing key (signed with a one-off key) is on the phone,
uninstall it once.

## Signing key (for publishing)

Make a key and give it to the repository as four secrets (**Settings → Secrets and
variables → Actions → New repository secret**):

```bash
keytool -genkeypair -v -keystore duckfoot.jks -alias duckfoot \
        -keyalg RSA -keysize 4096 -validity 10000
base64 -w0 duckfoot.jks        # the value of DUCKFOOT_KEYSTORE_BASE64
```

| Secret | Value |
|---|---|
| `DUCKFOOT_KEYSTORE_BASE64` | the `base64` output above |
| `DUCKFOOT_KEYSTORE_PASSWORD` | the keystore password you chose |
| `DUCKFOOT_KEY_ALIAS` | `duckfoot` |
| `DUCKFOOT_KEY_PASSWORD` | the key password (the same as the keystore's, unless you chose another) |

Keep `duckfoot.jks` and its passwords somewhere safe and out of the repository: whoever
has them can sign an update to the app. Builds signed with the testing key are a
different app to Android, so uninstall the testing build once when switching.

To sign a local build with the same key, export `DUCKFOOT_KEYSTORE` (the `.jks` path) and
the three other variables above before running `build-apk.sh`.

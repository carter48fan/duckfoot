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

Every push also builds a release APK, and the desktop builds, in the **Build** workflow
(`.github/workflows/build.yml`); they are attached to the run as artifacts for a day.

## Release

Push a tag that starts with `v`, or run the workflow by hand (**Actions → Build → Run
workflow**) with a version such as `v0.4.0`, or `auto` for the next patch:

```bash
git tag v0.4.0
git push origin v0.4.0
```

That builds every platform, runs the tests on Linux, Windows and macOS, and publishes the
release with checksums and build-provenance attestations. It fails without the signing key
below, and for a version already released. The version names the app (`v0.4.0` → `0.4.0`)
and sets its version code (see `app/build.gradle.kts`).

## Test builds

Every push to a branch other than `main` updates that branch's rolling draft release,
**Test build: `<branch>`**, with the testing APK as a plain file: signed in to GitHub on
the phone, open the draft under Releases, tap the `.apk`, install. Each push replaces it.
Drafts are visible only to the repository's collaborators.

Android installs an update only over an app signed with the same key, and that app can
read the data of the one it replaces. The **testing key** checked in as
`android/testing.jks` is public (its password is in `app/build.gradle.kts`), so nothing
signed with it may be the real app. There are therefore two apps:

| | App id | Label | Signed with |
|---|---|---|---|
| A release | `com.walker.duckfoot` | Duckfoot | the repository's key (below) |
| Every other build (debug, CI branch builds, a local release without the key) | `com.walker.duckfoot.testing` | Duckfoot testing | the testing key |

They install side by side, each keeping its own library and edits. CI refuses to make a
release without the repository's key, and `build-apk.sh` fails a build signed with the
wrong key for what it is.

Releases up to v0.3.23 were the real app id signed with the testing key. Moving to the
repository's key means those installs must be uninstalled once (Android will not update
across keys); export or note anything wanted from them first.

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
has them can sign an update to the app, and losing them means no update can ever be
installed over the app again.

To sign a local build with the same key, export `DUCKFOOT_KEYSTORE` (the `.jks` path) and
the three other variables above before running `build-apk.sh`.

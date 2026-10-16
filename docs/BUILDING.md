# Building Duckfoot from source

This page is for people building or hacking on Duckfoot. If you just want to **use** it,
the [README](../README.md) has downloads and install steps for every platform.

Developer pages: [Testing](./TESTING.md) · [Status and roadmap](./STATUS.md) ·
[Architecture](./ARCHITECTURE.md) · [Troubleshooting](./TROUBLESHOOTING.md) ·
[Keyboard shortcuts](./SHORTCUTS.md) · [Legal](./LEGAL.md)

---

## Quick start

```bash
git clone <this repo> && cd duckfoot
cargo run --release -- ~/Pictures/some-photo.ARW
```

Or launch empty and use **Open RAW…** in the top bar:

```bash
cargo run --release
```

iPhone and Mac photos (HEIF/HEIC) open in a build with the `heif` feature. Release builds
turn it on; a build from source has it off unless asked for, because HEVC is patented (see
[Legal notice: HEVC patents](./LEGAL.md#legal-notice-hevc-patents)):

```bash
cargo run --release --features heif
```

**Use `--release`.** A debug build of pixel code is roughly two orders of magnitude slower
and will feel broken. `Cargo.toml` already forces `opt-level = 3` on dependencies even in
dev profile, but the workspace crates themselves are only lightly optimised in debug.

First build takes a few minutes (wgpu and winit are large). Rebuilds are a couple of seconds.

---

## Requirements

### Build

| | |
|---|---|
| Rust | stable, 2021 edition. Developed on 1.96.1 |
| Disk | ~3.5 GB for `target/` |

No Node, no pnpm, no system package manager steps on a normal desktop Linux install.

### Run

| | |
|---|---|
| GPU | Anything with a working **Vulkan** driver. Integrated graphics is fine — the numbers below are from Intel integrated |
| Display | Wayland or X11 |
| File dialog | `xdg-desktop-portal` plus a backend (`xdg-desktop-portal-gtk`, `-cosmic`, `-kde`…) |

The release binary dynamically links only `libc`, `libgcc_s` and `libm`. The Vulkan loader,
Wayland client and xkbcommon are all `dlopen`'d at runtime, which is why the binary is
portable across distros but needs those present on the machine that runs it.

### Debian/Ubuntu/Pop!_OS, if something is missing

```bash
sudo apt install build-essential libvulkan1 mesa-vulkan-drivers \
                 libwayland-client0 libxkbcommon0 xdg-desktop-portal-gtk
```

On Arch: `vulkan-icd-loader`, `libxkbcommon`, `xdg-desktop-portal-gtk`.

Check Vulkan is actually working before blaming Duckfoot:

```bash
vulkaninfo --summary
```

---

## Building

```bash
cargo build --release              # binary at target/release/duckfoot
cargo build --workspace            # everything including examples
```

The binary is self-contained apart from the runtime libraries listed above — copy
`target/release/duckfoot` anywhere and it runs.

```bash
strip target/release/duckfoot      # 21 MB → 17 MB, optional
```

---

## Running

```bash
# open empty, pick a file from the UI
cargo run --release

# open a file directly
cargo run --release -- ~/Pictures/DSC08450.ARW

# from the built binary
./target/release/duckfoot ~/Pictures/DSC08450.ARW

# see what the GPU and decoder are doing
RUST_LOG=debug ./target/release/duckfoot
```

You can also **drag a RAW file onto the window**.

### Supported files

`.cr2 .cr3 .nef .nrw .arw .srf .sr2 .dng .orf .rw2 .raf .pef .raw`

Decoding is `rawler`'s (~2,500 camera models), with `rawloader` as a fallback for anything
rawler refuses. Files neither can handle fail with
the filename and a reason in the status bar rather than silently rendering garbage.

---

## Deployment

### Linux — verified

```bash
cargo build --release
strip target/release/duckfoot
```

Ship `target/release/duckfoot`. Runtime needs a Vulkan ICD, Wayland or X11 client
libraries, xkbcommon, and a desktop portal for the file dialog.

For an AppImage, `tools/build-appimage.sh` builds and packages
`dist/duckfoot-linux-x86_64.AppImage` (CI runs the same script). It runs on systems with
the build machine's glibc or newer; CI builds in `ubuntu:22.04` for wider reach.

For a `.desktop` entry:

```ini
[Desktop Entry]
Type=Application
Name=Duckfoot
Exec=/opt/duckfoot/duckfoot %f
Icon=/opt/duckfoot/duckfoot.png
Categories=Graphics;Photography;
MimeType=image/x-sony-arw;image/x-canon-cr3;image/x-adobe-dng;
```

### Android — builds in CI, not yet verified on a device

`crates/duckfoot-android` is the entry point (`android_main`) and `android/` is the Gradle
project around it: a small Java `NativeActivity` subclass for the system file picker,
"Open with", and hiding the system bars. Everything drawn is the same Rust as the desktop.

```bash
# needs an Android SDK + NDK (ANDROID_HOME, ANDROID_NDK_HOME), JDK 17, cargo-ndk
cargo install cargo-ndk
android/build-apk.sh            # → android/app/build/outputs/apk/debug/app-debug.apk
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -s duckfoot          # the app's log
```

CI builds the APK too; see [Releases and CI](#releases-and-ci) below. A signing key in the
repository's secrets lets each APK install over the last — see
[android/README.md](../android/README.md).

How it differs from the desktop, for now:

- **One photo at a time.** A photo is picked when the app opens, edited, and replaced by
  the next one picked (＋ at the top left); its edits are saved on the way out. There is no
  shelf, no stepping between photos and no batch export, and the next launch starts empty.
  Android hands out documents, not paths, so a picked photo is copied into the app's
  private library and edited there, its sidecar beside the copy. Once a photo is
  exported its copy has done its job: it is deleted, sidecar and all, as soon as another
  photo replaces it (or on the next launch, if the app closed with it open), so the
  editor never turns into a second gallery. A photo never exported keeps its copy, and
  picking it again brings its edits back. **⋯ → Photo info & storage** shows what those
  copies take up and deletes them.
- **Exports go to `Pictures/Duckfoot`**, where the gallery shows them. There is no save
  dialog.
- **Back** closes the crop tool or an open dialog first, then sends the app to the
  background. Backgrounding keeps the photo, stack and undo history.
- **The phone layout**, after Snapseed. Below 900 × 560 points (phones either way up,
  tablets held upright) the stage fills the screen and the bottom bar holds four groups:
  - **Looks**: one-tap looks and their strength, snapshots, history, copy & paste.
  - **Tune**: every quick value, in three lists — light (exposure, highlights, shadows,
    whites, blacks, dehaze, vignette), colour (temperature, tint, vibrance, saturation)
    and detail (clarity, texture, sharpen, noise, colour noise). The chosen value sits
    over a ruler; swipe up or down, on the photo or on the ruler, to walk its list (up
    for the next value), and sideways to change it. The list that floats over the photo
    has **Light · Colour · Detail** as tabs: drift the same finger left or right to
    switch. Under the ruler the same three, with a dot per value (amber once changed),
    are tabs too: tap one, or swipe the bar for the next.
    **AUTO** beside a value is auto tone for light, auto white balance for colour.
  - **Crop**: the crop tool, then straighten, perspective and lens distortion on the
    ruler, with auto straighten / upright.
  - **Tools**: a grid of the tools that are not one value — curves, colour mixer, colour
    grade, brush & masks, healing, **remove background**, LUT, filmic, scope. A tile
    opens its tool; back returns to the grid.

  Double-tap the ruler or the photo to put a value back; hold the photo for the original;
  the ⋯ beside a value opens all its tool's settings. Turned sideways, the bar becomes a
  rail on the right. **Export** is at the top right; before / after, zoom, **fine**, the
  clipping view, reset and the tour are in the top bar's ⋯ menu. Controls are drawn at
  touch size (chips 34 pt, buttons 40, icons 44).
- Requires Android 11+ and Vulkan 1.1. Built for `arm64-v8a` only.

### Windows and macOS — built in CI, lightly tested

**Built in CI, not yet smoke-tested by hand on many machines.** The release builds are
**unsigned**, so Windows SmartScreen and macOS Gatekeeper warn on first run (the README's
install steps say how to get past them). The macOS build is **Apple Silicon (arm64) only**;
there is no Intel Mac build. The shell in `crates/duckfoot/src/lib.rs` holds no
Linux-specific code.

```bash
# Windows (from Linux)
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu

# macOS — must build on a Mac; no cross-compilation
cargo build --release --target aarch64-apple-darwin
```

## Releases and CI

The **Build** workflow (`.github/workflows/build.yml`) builds the Android APK, the Linux
AppImage and tarball, the Windows zip and the macOS `.dmg` / `.zip` on every push.

- **Push to `main`** auto-increments the patch version, builds all four platforms, and
  publishes a full release (marked *Latest*) with the files below attached.
- **A `v…` tag**, or a manual run given a version, publishes a release with that version.
- **Any other branch** keeps one rolling *test build* pre-release holding just the APK.
- **A manual run with no version** builds only; the files stay on the workflow run for a day.

Release files are named `duckfoot-<tag>-<platform>.<ext>`, so the file name changes with
every release; link to the [latest release page](https://github.com/carter48fan/duckfoot/releases/latest)
rather than to a file.

| File | What it is |
|---|---|
| `duckfoot-<tag>-windows-x86_64.zip` | Windows portable build (`duckfoot.exe`) |
| `duckfoot-<tag>-macos.dmg` / `.zip` | `Duckfoot.app`, Apple Silicon |
| `duckfoot-<tag>-linux-x86_64.AppImage` | Linux, one file |
| `duckfoot-<tag>-linux-x86_64.tar.gz` | Linux binary plus licences |
| `duckfoot-<tag>-android-arm64.apk` | Android 11+, arm64 |
| `duckfoot-<tag>-source.tar.gz` | Complete corresponding source (LGPL-2.1 §6) |

Desktop tests on `windows-latest` and `macos-latest` run on manual builds and tags (macOS
runner minutes cost ten times Linux's, so not every push pays for them); GPU tests skip
themselves where the runner has no GPU.

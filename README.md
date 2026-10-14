# 🦆 Duckfoot

**A local-first FOSS creative suite: focused Photo, Audio and Video editing.**

> **Status: v0.3.0 (Pre-Release / Alpha). Barrel 1 (Photo Studio & RAW processor) feature-complete.
> Linux verified; Android arm64 APK builds in CI; Windows and macOS run test suites in CI.
> Audio and Video are architectural designs for Barrels 2 & 3 — see [DESIGN.md](./DESIGN.md).**
>
> The legacy TypeScript codebase (`apps/` and `packages/`) has been completely removed. Duckfoot is native Rust on wgpu. Three dependencies compile non-Rust code: libwebp (C, for WebP export), tract-linalg's assembly matrix kernels (the cutout model), and wayland-backend on Linux.


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
[Legal notice: HEVC patents](#legal-notice-hevc-patents)):

```bash
cargo run --release --features heif
```

**Use `--release`.** A debug build of pixel code is roughly two orders of magnitude slower
and will feel broken. `Cargo.toml` already forces `opt-level = 3` on dependencies even in
dev profile, but the workspace crates themselves are only lightly optimised in debug.

First build takes a few minutes (wgpu and winit are large). Rebuilds are a couple of seconds.

---

## The idea

Traditional editors force every creative task through one monolithic timeline. Slicing a
podcast or cropping a product photo shouldn't require a 500 MB NLE with twenty video tracks
and a cloud account.

Duckfoot mounts three dedicated studios onto one shared asset shelf — like a duckfoot pistol
firing distinct barrels from a single stock.

```
                    ┌────────────────────────┐
                    │      Asset Shelf       │
                    │   (local-first stock)  │
                    └───────────┬────────────┘
             ┌──────────────────┼──────────────────┐
             ▼                  ▼                  ▼
      [ 📷 Photo ]        [ 🎵 Audio ]       [ 🎬 Video ]
       RAW processor       stem stack         3-track timeline
       IN PROGRESS         design only        design only
```

The design brief — what each barrel deliberately refuses to do, and why the pipeline is
shaped the way it is — is in [DESIGN.md](./DESIGN.md). **Read that before contributing.**

### Principles

1. **Focused simplicity.** Dedicated views for dedicated jobs. No timeline outside the
   video editor.
2. **Local-first, zero-cloud.** No logins, no database, no analytics. Your media never
   leaves your device.
3. **Native, on the GPU.** The image is uploaded once. A slider drag writes a uniform
   buffer, not a frame.

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

### Controls

<!-- keymap:start (generated from crates/photo-app/src/keymap.rs) -->
| | |
|---|---|
| **Photos** | |
| `←` / `→` | previous / next photo, in the filmstrip's order |
| `Ctrl+S` | save now (edits also save on their own) |
| `Ctrl+E` | export this photo: size, format, metadata, then where |
| `Ctrl+Shift+E` | batch export the selection |
| **Editing** | |
| `Alt+↑` / `Alt+↓`, `Alt+←` / `Alt+→` | choose a value of the card last worked on, then turn it (as a right-drag on the photo does) |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo, per photo |
| `Ctrl+C` / `Ctrl+V` | copy / paste the adjustments (crop and rotation stay put) |
| `Ctrl+Shift+V` | choose what paste carries |
| `Ctrl+'` | virtual copy: another edit of the same photo |
| `Ctrl+Alt+V` | the same edits as the photo open before this one |
| `Esc` with a mask chosen or healing | put the mask's handles away / stop healing |
| `C` | crop tool on / off |
| `Enter` / `Esc` | apply / cancel the crop |
| `X` / `O` while cropping | turn the crop's shape / next guide |
| `Shift` while resizing a crop | keep its shape |
| `[` / `]` | rotate 90° left / right |
| `H` / `V` | mirror / flip |
| `Esc` while dragging | put the control back where the drag started |
| `W`, then click | white balance from something that should be grey |
| **View** | |
| `\` | before / after split |
| `S` | soft proof: the preview in the export's colour space |
| `Ctrl+0` / `Ctrl+1` / `Z` | fit / 100% / toggle between them |
| `?` or `F1` | this list of shortcuts |
| **Library** | |
| `0`–`5` | star rating for the selection (`0` clears) |
| `6` / `7` / `8` / `9` | red / yellow / green / blue label (again takes it off) |
| `P` / `X` / `U` | flag as pick / reject / clear the flag |
| `Ctrl+A` | select every photo on the shelf |
| `Shift`+click / `Ctrl`+click | select a range / add to the selection |
| **Mouse and touch** | |
| Scroll or pinch over the photo | zoom about the pointer |
| Drag the photo | pan, once zoomed in |
| Drag a slider | live; one undo step per gesture |
| `Shift` while dragging a slider | ten times finer |
| Double-click a slider or its label | back to its default |
| Drag in the curve | move the nearest point; the ends keep their place |
| Drag a 3D view | orbit it; `Ctrl`+scroll zooms, double-click resets |
| Click a module's header | open it and close the others; `Shift`+click opens it alongside |
| Touch a control in a module that's off | turns the module on |
| Click a History row | jump to that step |
| Drag the split divider | move it; double-click centres it |
| Hold the scope's clip readout | show clipping while held |
| Drop files on the window | add them to the shelf and open the first |
<!-- keymap:end -->

Press `?` in the app for the same list. On a Mac, `Ctrl` is `⌘`.

Hotkeys deliberately do not fire while a text field has focus.

### Supported files

`.cr2 .cr3 .nef .nrw .arw .srf .sr2 .dng .orf .rw2 .raf .pef .raw`

Decoding is `rawler`'s (~2,500 camera models), with `rawloader` as a fallback for anything
rawler refuses. Files neither can handle fail with
the filename and a reason in the status bar rather than silently rendering garbage.

---

## Testing and verification

### Unit tests

```bash
cargo test --workspace
```

Over 300 tests across the workspace covering the colour-matrix maths (inversion, neutral-maps-to-neutral,
singular-matrix fallback), white-balance normalisation, RCD and X-Trans demosaicing,
pure-Rust ONNX inference (`u2netp.onnx`), monotone tone curve, banded 100MP+ export,
GPU histogram computation, non-destructive geometry, sidecar persistence round-tripping,
zero-allocation drag invariants (`tests/drag.rs`), 45 headless UI lifecycle tests (`tests/headless.rs`),
and 48 real-GPU lavapipe pixel truth tests (`tests/gpu.rs`).

### Smoke test in BOTH profiles

```bash
cargo run              # debug — do not skip this
cargo run --release
```

**A release-only smoke test is not sufficient.** egui, epaint and wgpu validate heavily
through `debug_assert!`, which is compiled out in release. A bug that panics loudly in
debug can degrade silently in release — an unapplied `TexturesDelta` crashes a debug build
and merely loses the font atlas in a release one. Run debug first.

### Lint and format

```bash
cargo clippy --workspace --all-targets     # clean, zero warnings
cargo fmt --all --check
```

### Inspect what the decoder extracted from a file

```bash
cargo run --release --example probe -- ~/Pictures/DSC08450.ARW
```

```
── /home/carterw/Pictures/DSC08450.ARW
   camera     SONY ILCE-7M3
   cropped    6048×4024  (24.3 MP)
   cfa 2x2    [0 1 / 1 2]   (0=R 1=G 2=B)
   black      [512.0, 512.0, 512.0]
   white      [16300.0, 16300.0, 16300.0]
   wb mult    [2.0273, 1.0000, 1.8008]
   cam→sRGB   [[1.1939, -0.1153, -0.0786], [-0.5488, 2.4177, -0.8689], ...]
   decode     84 ms
```

It asserts that every colour-matrix row sums to 1.0 and that the sample count matches the
cropped dimensions, so a bad decode fails rather than printing nonsense.

**What to look for:** `cfa 2x2` should vary by camera. A Sony A7 III reads `[0 1 / 1 2]`
(RGGB); a Pixel 8 Pro DNG reads `[1 2 / 0 1]` (GBRG). The previous build hardcoded RGGB and
would have swapped red and blue on the second one — this is the regression check.

### End-to-end GPU benchmark with pixel readback

```bash
cargo run --release --example bench -- ~/Pictures/DSC08450.ARW
```

```
adapter      Intel(R) Graphics (RPL-S) (Vulkan)
max texture  16384 px
decode       81 ms   6048×4024 (24.3 MP)
upload+demosaic 215 ms
slider drag  18.27 ms/frame over 60 frames at FULL resolution
centre patch mean RGB [187.0, 186.7, 185.8]   fully-black pixels 0/65536

OK — real pixels, all three channels live.
```

This runs headless, blocks on the GPU so the timings are execution and not queueing, then
copies a 256×256 patch back and **fails** if the pipeline produced a black frame or a dead
channel. "The window opened without crashing" is not evidence the pipeline is correct;
this is.

### Where the time goes when a photo opens

```bash
cargo run --release -p photo-engine --example open_profile -- ~/Pictures/DSC08450.ARW
```

Times each stage of what the app does on open (read, rawler decode, the full `decode_raw`
the loader runs) and the embedded-preview path thumbnails and the stage stand-in use. Each
stage runs five times and the fastest is reported, so the first cold read does not
dominate. Like `probe` and `bench`, it goes through the decoder the app actually uses.

No camera files to hand? `synth_raw` writes a camera-sized DNG with a clipped highlight
and an embedded preview, so every stage has real work to do:

```bash
cargo run --release -p photo-engine --example synth_raw -- /tmp/a7iv.dng                    # 33 MP, uncompressed, ~66 MB
cargo run --release -p photo-engine --example synth_raw -- /tmp/big.dng 9728 6656 --lossless # 65 MP, LJ92, ~56 MB
cargo run --release -p photo-engine --example synth_raw -- /tmp/cam.jpg 6000 4000            # 24 MP JPEG
```

### See a layout without its device

```bash
cargo run --release -p photo-app --example screenshot -- phone.png 412x892 photo.dng edit exposure
```

Renders the whole app to a PNG at any size (in points, drawn at 2× like a phone), with a
photo open and, for the phone layout, a sheet and module chosen. Needs a Vulkan driver;
Mesa's software one (`mesa-vulkan-drivers`, lavapipe) is enough, so it works on a
headless machine.

### Stub audit

```bash
grep -rn '@stub' crates/
```

DESIGN.md invariant 8. The previous audit only grepped the TypeScript packages, which was
precisely where the unfinished code wasn't.

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

Every push builds an optimised Linux binary and a release APK in the **Build** workflow
(artifacts `duckfoot-linux-x86_64` and `duckfoot-android`). Every push to `main` also
refreshes the **nightly** release with both; a `v…` tag, or a hand-run given a version,
drafts a versioned one. A signing key in the repository's secrets lets each APK install
over the last — see [android/README.md](./android/README.md).

How it differs from the desktop, for now:

- **Files are imported.** Android hands out documents, not paths, so a picked photo is
  copied into the app's private library and edited there; its sidecar lives beside the
  copy. Picking the same photo again reuses the copy, so its edits come back. The shelf's
  **Library** bookmark lists everything imported.
- **Exports go to `Pictures/Duckfoot`**, where the gallery shows them. There is no save
  dialog.
- **Back** closes the crop tool or an open dialog first, then sends the app to the
  background. Backgrounding keeps the photo, stack and undo history.
- **The phone layout.** Below 900 × 560 points (phones either way up, tablets held
  upright) the stage fills the screen and a navigation bar opens one sheet at a time:
  **Library · Edit · Scopes · History · Export**. Edit shows one module's sliders at a
  time, picked from a scroller. Turned sideways, the bar becomes a rail on the right.
  What a keyboard does on the desktop has a touch control: select mode (Ctrl/Shift-click),
  select all, a **fine** toggle (Shift-drag), rating and flag chips, and a sideways swipe
  across the photo for the next or previous one.
- Requires Android 11+ and Vulkan 1.1. Built for `arm64-v8a` only.

### Windows, macOS — experimental

**Built and tested in CI, not yet smoke-tested by hand; no claim is made that they work.**
Every push builds a Windows zip. A manual run of the Build workflow, and every tag, also
runs the workspace tests on `windows-latest` and `macos-latest` (the GPU tests skip
themselves where the runner has no GPU) and builds an unsigned macOS `.dmg`, attached to
the run; macOS runner minutes cost ten times Linux's, so not every push pays for them. The shell in `crates/duckfoot/src/lib.rs` holds no
Linux-specific code.

```bash
# Windows (from Linux)
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu

# macOS — must build on a Mac; no cross-compilation
cargo build --release --target aarch64-apple-darwin
```

**Do not distribute a binary yet** — see the licence note at the bottom.

---

## Repository layout

```
duckfoot/
├── DESIGN.md                    # the design brief — start here
├── Cargo.toml                   # workspace root
├── android/                     # Gradle project for the APK; build-apk.sh builds it
└── crates/
    ├── photo-engine/            # wgpu device, textures, shaders, RAW decode
    │   ├── src/decode.rs        #   rawler/rawloader → cropped CFA mosaic + metadata
    │   ├── src/preview.rs       #   embedded JPEG previews: found, then decoded scaled
    │   ├── src/engine.rs        #   textures, pipelines, uniforms, GPU dispatch
    │   ├── src/curve.rs         #   monotone cubic tone curve → LUT
    │   ├── src/params.rs        #   the adjustment stack (plain data + serde)
    │   ├── src/sidecar.rs       #   non-destructive .duckfoot companion files
    │   ├── src/xmp.rs           #   XMP read, export packets, safe sidecar merge
    │   ├── src/proof.rs         #   printer and monitor ICC profiles baked to 3D tables
    │   ├── src/shaders/         #   demosaic, downsample (preview mips), adjust, histogram
    │   └── examples/            #   probe.rs, bench.rs, open_profile.rs, synth_raw.rs
    ├── photo-app/               # egui UI — no windowing dependency
    │   ├── src/lib.rs           #   layout: shelf, stage, filmstrip, inspector, undo
    │   ├── src/inspector_map_3d.rs # 3D rotatable inspector map & scopes
    │   ├── src/tonemap_3d.rs    #   interactive 3D filmic tonemap & RGB cube
    │   ├── src/theme.rs         #   pitch-black palette, fonts, egui visuals
    │   ├── src/widgets.rs       #   painted controls: chips, rails, module cards
    │   ├── src/loader.rs        #   background decode: opening photos, thumbnails
    │   ├── src/platform.rs      #   file pickers and storage places, supplied by the shell
    │   ├── src/compact.rs       #   the phone layout: navigation bar, one sheet at a time
    │   └── assets/fonts/        #   JetBrains Mono, Space Grotesk, Hanken Grotesk (OFL)
    ├── duckfoot/                # the winit shell (lib.rs) and the desktop main
    └── duckfoot-android/        # android_main and the Java bridge; see android/
```

---

## Current state, honestly

A symbol appears here only once it works end to end, verified against a real file — see
DESIGN.md invariant 8.

| Area | Symbol | Status | Notes |
|---|---|---|---|
| **Decode** | `DefaultRawDecoder` | Working | rawler, then rawloader, each tried once. Applies `crops`, real `cfa`, `wb_coeffs`, `xyz_to_cam`, per-channel levels |
| **Decode** | measured white level | Working | Finds the real clip plateau in the data; Canon's per-ISO saturation no longer turns highlights magenta |
| **Decode** | DNG `AsShotWhiteXY` | Working | Pixel-style DNGs; matrices interpolated by colour temperature per the DNG spec |
| **Decode** | EXIF orientation | Working | Opens upright; the camera's turn is the starting geometry, not an edit |
| **Decode** | `RawDecoder` trait | Working | `rawler` is a drop-in behind it |
| **Decode** | non-mosaic (`cpp != 1`) | `@stub` | Refuses loudly rather than mis-indexing |
| **Decode** | float-encoded RAW | `@stub` | Some DNGs; refuses loudly |
| **Decode** | 4-colour CFA (CYGM) | `@stub` | 2×2 Bayer and 6×6 X-Trans RGB patterns only |
| **Engine** | `Engine::load` | Working | Upload + demosaic, once per image |
| **Engine** | `Engine::render` | Working | Fused adjustment pass, one uniform write per change |
| **Engine** | demosaic | Working | 4-pass RCD (Residual Color Difference) + 6×6 Fujifilm X-Trans in WGSL |
| **Params** | white balance, exposure | Working | Including 5 one-click Kelvin presets + As-Shot reset + TAT / Eyedropper |
| **Params** | tone curve | Working | Monotone cubic, 1024-entry LUT (Display & Linear domain support) |
| **Params** | filmic, colour balance | Working | Log-domain sigmoid mapping, ratio-preserving |
| **Params** | highlights & shadows (tonal) | Working | High dynamic range shadow & highlight balancing in shader |
| **Params** | vibrance & saturation | Working | Non-linear skin-safe saturation boost |
| **Params** | capture sharpen, vignette | Working | Resolution-independent convolution & crop-centered radial falloff |
| **Params** | `Geometry` (crop / rotate / mirror) | Working | 90° CCW/CW, 180°, H/V mirror, normalized crop box with 8 handles & aspect lock |
| **Params** | Denoise (NR) | Working | Cached compute pre-pass for luma + inline bilateral filtering for chroma |
| **Params** | Lens Correction | Working | Distortion, TCA, vignetting via bundled Lensfun database and DNG opcodes |
| **Params** | Perspective / Keystone | Working | Vertical/horizontal homography and Auto Upright line detection |
| **Params** | Local Adjustments | Working | Up to 8 linear gradient, radial, or sensor-space brush masks |
| **Params** | Spot Heal, Clone & Remove | Working | Up to 32 circular sensor-space spots: heal/clone from a draggable source, or remove (content-aware PatchMatch fill from the vendored `lc-inpaint`, up to 16 at once; a spot of radius over ~42 px is filled at a coarser level of the sensor, so softer) |
| **Params** | Look (`.cube`) | Working | A 3D or 1D `.cube` LUT (Resolve/Adobe) applied to the photo as an sRGB screen shows it, mixed in by amount; above-white and out-of-sRGB values pass through unclipped; read by the engine so batch export applies it too |
| **AI Studio** | `CutoutStudio` (rembg) | Working | Pure-Rust `tract-onnx` neural inference (`u2netp.onnx`), alpha matting, defringing & touch-ups |
| **Colour** | Wide-gamut working space | Working | Linear Rec.2020 working space; sRGB, Display P3, and Adobe RGB exports with embedded ICC profiles |
| **Engine** | highlight reconstruction | Working | Unclipped color ratio propagation over mips; blown speculars stay neutral |
| **Export** | `Engine::export_at`, `ExportSettings` | Working | JPEG, PNG, TIFF, WebP, AVIF (sRGB, via rav1e) and JPEG XL (via jixel; 16-bit, lossless at q100 or in Adobe RGB); 8 or 16 bits; rendered and read back in bands (at most 64 MB of GPU memory, however large the photo); EXIF, XMP, ICC profiles (AVIF: EXIF only, tagged sRGB). JPEG can carry an HDR gain map (ISO 21496-1 / Ultra HDR, with Adobe and Apple XMP and an MPF index): an HDR rendition two stops brighter than the export's white, measured in bands at a quarter size. Untested so far on a real HDR screen |
| **Sidecar** | `save_sidecar`, `load_sidecar` | Working | Non-destructive `.duckfoot` JSON v2 sidecars, atomic write, persistent undo history |
| **HEIF** | `heif::open`, feature `heif` | Working (opt-in) | iPhone/Mac HEIC stills through the vendored pure-Rust `lc-heif` (from LightCraft): grids, 8–10 bit, ICC or `nclx` P3, container rotation/crop, embedded thumbnails. 4:2:2/4:4:4 camera HIF and HDR (PQ/HLG) are refused |
| **XMP** | `xmp::seed_metadata`, `xmp::packet`, `xmp::merge` | Working | Ratings, labels and keywords read from Lightroom/darktable `.xmp` sidecars and embedded XMP, written into every export, and (opt-in, Customize) kept in step with an existing `.xmp` sidecar without touching the rest of it |
| **App** | inspector, undo/redo | Working | Snapshot undo, per-photo persistent history, one checkpoint per gesture |
| **App** | inspector map 3D | Working | Rotatable 3D Ribbons, 3D Surface, 3D Scatter, 2D Plot |
| **App** | filmic 3D tonemap | Working | 3D transfer surface, RGB cube deformation, handles, presets |
| **App** | inspector sidebar | Working | Module cards with live values, Customize tab (show/hide, reorder), width toggle |
| **App** | history snapshots | Working | Named stack snapshots with instant recall |
| **App** | history list | Working | Every undo step labelled by what changed; click a row to jump to it |
| **App** | file open, drag-and-drop | Working | Multi-file XDG portal ingest + drag & drop; decoded and uploaded off the UI thread |
| **App** | filmstrip thumbnails | Working | LRU-managed tile cache (1,500 desktop, 400 phone) decoded on background workers |
| **App** | open stand-in | Working | Embedded preview on stage in ~20–35 ms while full decode runs |
| **App** | the shelf | Working | Collapsible asset manager, switch photo, copy/paste stack, virtual copies |
| **App** | before / after split view | Working | Draggable divider (`\`), raw baseline vs active stack |
| **App** | crop tool overlay | Working | 8-handle crop framing, rule-of-thirds, golden ratio, aspect ratios (`C` / `Esc`) |
| **App** | export | Working | Banded export dialog (Ctrl+E) and phone Export sheet sharing unified recipes |
| **App** | zoom and pan | Working | Scroll to zoom about cursor, drag to pan, Cmd+0 fit, Cmd+1 100% |
| **Engine** | `Histogram`, `poll_histogram` | Working | GPU compute + atomics, 4 KB async readback |
| **Engine** | preview at screen resolution | Working | Sensor mip chain; preview renders at coarsest level as dense as screen. Export renders full res |
| **Platform** | Windows / macOS / Android | Built in CI | Linux verified; Android arm64 APK builds in CI; Windows and macOS run test suites in CI |

### Measured

Sony A7 III (ILCE-7M3), 24.3 MP, **Intel integrated graphics** (RPL-S, Vulkan):

```
decode              77 ms    6048×4024
upload + demosaic  220 ms    once per image
slider drag        6.1 ms/frame at FULL sensor resolution
export readback    61 ms     92 MB
png encode+write  205 ms
```

### Known gaps and caveats

- **Fuji X-Trans demosaicing** is implemented in WGSL. Linear DNGs (Apple ProRAW, HDR merges; integer or float, range above white kept) open without demosaicing. Still refused loudly with `@stub`: float mosaics, Canon sRAW/mRAW (its white balance through rawler's YCbCr conversion is unverified), and four-colour or unknown CFAs. JPEG XL-compressed DNG 1.7 (newer ProRAW) goes through rawler's own jxl-oxide decoder; no real ProRAW file has been tried yet, only synthetic linear and float DNGs.
- **Soft proofing** proofs against the export's colour space, and through a printer's ICC profile (CMYK or RGB, LUT-based, via `moxcms`) chosen from the stage toolbar's `printer…` chip, relative colorimetric or perceptual, optionally simulating the paper's white. Profiles moxcms misreads (v4 lut16 tables over an XYZ connection space, such as Ghostscript's `ps_cmyk.icc`) are refused rather than proofed wrong.
- **Display colour management** supports sRGB, Display P3, Adobe RGB, and the monitor's own ICC profile via settings, matrix or LUT-based (DisplayCAL's default), applied exactly through a baked 3D table rather than a fitted gamma. Or "system": the profile the OS has for the monitor the window is on (colord on Linux, Windows colour management, ColorSync on macOS; `crates/display-profile`), looked up again when the window moves to another monitor. Only the Linux path has run, against a colord with no displays registered; the Windows and macOS paths are type-checked, not yet run. The phone draws in sRGB.

---

## Development plan

Checked items are done **and verified**, not merely written.

### Stage 0 — Audit and demolition ✅

- [x] Audit the previous TypeScript/Tauri build
- [x] Establish that the webview IPC boundary was structurally unfixable
- [x] Rewrite DESIGN.md and README around the native architecture
- [x] Workspace scaffold, CI moved from pnpm to cargo

### Stage 1 — Engine core ✅

- [x] Crate split: engine / app / bootstrap, no windowing in the first two
- [x] RAW decode honouring `crops`, real `cfa`, `wb_coeffs`, `xyz_to_cam`, `cpp`, per-channel levels
- [x] `RawDecoder` trait so `rawler` stays a one-module swap
- [x] Demosaic pass → immutable linear camera-RGB texture
- [x] Fused adjustment shader: WB, colour matrix, exposure, curve, filmic, colour balance
- [x] Monotone cubic tone curve with LUT upload only on change
- [x] egui shell themed to DESIGN.md; inspector, curve editor, snapshot undo
- [x] winit bootstrap with Android-shaped surface lifecycle
- [x] **Benchmark on a real 24 MP file with pixel readback** — the PoC's pass/fail

### Stage 2 — Finish the PoC ◀ current

- [ ] **Look at a real photo and confirm the colour is right.** Pixels are proven live and
      near-neutral, but nobody has visually confirmed a decoded image yet. Blocks everything else.
- [x] Preview at fit-to-window scale; full resolution reserved for export
- [x] JPEG/PNG/TIFF/WebP export through the same shader — the real test of invariant 2
- [x] Live histogram via a compute pass and a 4 KB readback
- [x] 3D Inspector Map (Ribbons, Surface, Cube, 2D) & 3D Filmic Tonemap
- [x] Crop, rotate (90° CCW/CW, 180°) and flip (H/V mirror)
- [x] Before/after split view with draggable divider

### Stage 3 — Platforms

- [ ] Windows build and smoke test
- [ ] macOS build and smoke test
- [ ] `crates/duckfoot-android/` bootstrap on `android-activity`
- [ ] NDK + `cargo-ndk` + gradle harness, signed debug APK
- [ ] Verify on a real device: texture limits, memory ceiling on a 24 MP file, suspend/resume
- [x] Settle the LGPL-2.1 distribution question (complete source ships with every release)

### Stage 4 — Barrel 1 proper

- [x] Extended pipeline modules (highlights & shadows, vibrance, capture sharpen, vignette)
- [x] Local contrast (clarity, texture) and lens correction (lensfun profiles, DNG warp)
- [ ] Remaining modules (tone equaliser, profiled denoise)
- [x] Better demosaic (RCD for Bayer, two-pass X-Trans)
- [x] The Shelf — asset list shared across barrels with multi-file ingest
- [x] Copy stack / paste across photos
- [x] Sidecar persistence (`.duckfoot` companion JSON files)
- [x] Customizer drawer, module reordering, smooth scrolling, and sidebar expansion
- [x] Swap to `rawler` for camera coverage (rawloader kept as a fallback)
- [x] Delete `apps/` and `packages/`

### Stage 5 — The other barrels

- [ ] Barrel 2 — Audio stem stack
- [ ] Barrel 3 — 3-track video timeline

---

## Troubleshooting

**`could not create a window or GPU device`** — no usable Vulkan adapter. Check
`vulkaninfo --summary`; install `mesa-vulkan-drivers` or your vendor driver.

**Everything is painfully slow** — you built in debug. Use `--release`.

**The Open RAW… button does nothing** — no XDG desktop portal backend. Install
`xdg-desktop-portal-gtk` (or `-cosmic` / `-kde`) and relog. Passing the file as a CLI
argument or dragging it onto the window bypasses the dialog entirely.

**The picker greys out files that are obviously RAW** — the filter list does not cover that
extension. XDG portal globs are case-sensitive, so the filter lists every extension in both
cases; if your camera writes something not in `RAW_EXTENSIONS`, switch the dialog to
**All files**, or pass the path on the command line. Add the extension to
`crates/photo-engine/src/lib.rs` if it decodes.

**A file fails to open** — the status bar gives the filename and the reason. `@stub`
reasons (non-mosaic, float-encoded, 4-colour CFA) are unimplemented, not broken. Run
`--example probe` on the file for detail.

**The image looks green** — that would mean white balance isn't being applied. Check
`--example probe` output: `wb mult` should not be `[1.0, 1.0, 1.0]` for a daylight shot.

**Magenta or green speckle on fine detail** — wire mesh, strings, fabric. That is demosaic
false colour, not a highlight or white-balance fault, and it needs a better demosaic kernel.
Magenta on *blown speculars* specifically is a different thing and is fixed.

**The image looks flat and grey with filmic on** — `latitude` is a percentage of the log
range. Above roughly 60 it compresses everything toward mid grey. The default is 20.

---

## Licence

MIT — see [LICENSE](./LICENSE).

Duckfoot uses `rawler` and `rawloader` for RAW format decoding, which are licensed under the **GNU LGPL-2.1**. In compliance with LGPL-2.1 §6, complete corresponding source archives (`duckfoot-<version>-source.tar.gz`) are provided with every release to enable inspection, modification, and relinking.

The UI bundles three typefaces under the **SIL Open Font License 1.1** — JetBrains Mono, Space Grotesk and Hanken Grotesk (`crates/photo-app/assets/fonts/`). Lens correction data is adapted from the Lensfun database (**CC BY-SA 3.0**, `crates/photo-engine/data/LENSFUN-NOTICE.md`).

Duckfoot contains code adapted from [LightCraft](https://github.com/storytold/lightcraft) (MIT OR Apache-2.0, used under MIT): its XMP reader and merge, HEIF decoder and content-aware fill; see `THIRD_PARTY_LICENSES.md` and each `crates/vendor/*/VENDORED.md`.

### Legal notice: HEVC patents

**This is not legal advice.** If it matters to you, ask a lawyer who knows the countries
you work in.

Duckfoot's release builds (the AppImage, tarball, Windows, macOS and Android downloads)
include a software HEVC decoder (`heic-rs`, through `crates/vendor/lc-heif`), so that they
can open HEIF/HEIC photos. HEVC (H.265) is covered by patents held by many companies, in
many countries, and licensed among others through the Access Advance and Via LA patent
pools.

- **No patent licence comes with Duckfoot.** Its MIT licence, and its dependencies'
  open-source licences, grant rights in their code (copyright), not in anyone's patents.
  Duckfoot's authors hold no HEVC patents and cannot license them to you.
- **If you distribute Duckfoot with HEIF enabled, you may be obliged to obtain a patent
  licence, and obtaining it is your responsibility, not the authors'.** That applies most
  plainly if you ship it commercially, in volume, preinstalled on or bundled with a device,
  or as part of a product, in a country where the patents are in force. Whether the pools'
  terms reach you, and what they would cost, depends on what you ship, how many, and where.
- **Using it may be affected too** in some places.
- **To avoid HEVC entirely, build without it** (the default for a source build):
  `cargo build --release -p duckfoot`. Such a build refuses HEIF/HEIC files by name and
  contains no HEVC code. Redistributors who are unsure should ship that build.

The other formats Duckfoot reads and writes (JPEG, PNG, TIFF, WebP, AVIF, JPEG XL, the RAW
formats) are not known to need a patent licence for this use: AV1, which AVIF is built on,
is licensed royalty-free by the Alliance for Open Media.

Duckfoot previously contained code derived from [OpenCut](https://github.com/OpenCut-app/OpenCut) (MIT); that code is superseded, and their copyright notice is retained in git history.


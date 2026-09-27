# 🦆 Duckfoot

**A local-first FOSS creative suite: focused Photo, Audio and Video editing.**

> **Status: pre-alpha. Barrel 1 (Photo / RAW) proof of concept. Linux verified; Android
> and Windows build in CI (Android is untested on devices); Windows and macOS run the
> test suite in CI and are experimental: not yet smoke-tested by hand.**
>
> Duckfoot is a native Rust application on wgpu. The only thing under construction is the
> **Photo Studio and RAW processor**. Audio and Video are design, not code — see
> [DESIGN.md](./DESIGN.md).
>
> The previous TypeScript build under `apps/` and `packages/` is superseded and scheduled
> for removal. It is not referenced by anything in `crates/`, and it remains in git history
> at `b4054aa4` regardless.

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

31 tests covering the colour-matrix maths (inversion, neutral-maps-to-neutral,
singular-matrix fallback), white-balance normalisation including garbage input, the
monotone tone curve (identity, no reversal between close nodes, passes through control
points, degenerate input), export row-padding/unpadding, GPU histogram computation,
non-destructive geometry (quarter-turn swapping, crop clamping and pixel bounds),
and JSON sidecar persistence round-tripping.

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

`apps/` and `packages/` are the superseded TypeScript build, pending removal.

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
| **Engine** | demosaic | Working | Hamilton-Adams: directional green + colour-difference R/B |
| **Params** | white balance, exposure | Working | Including 5 one-click Kelvin presets + As-Shot reset |
| **Params** | tone curve | Working | Monotone cubic, 1024-entry LUT |
| **Params** | filmic, colour balance | Working | |
| **Params** | highlights & shadows (tonal) | Working | High dynamic range shadow & highlight balancing in shader |
| **Params** | vibrance & saturation | Working | Non-linear skin-safe saturation boost |
| **Params** | capture sharpen, vignette | Working | Edge-preserving convolution & radial falloff |
| **Params** | `Geometry` (crop / rotate / mirror) | Working | 90° CCW/CW, 180°, H/V mirror, normalized crop box |
| **Engine** | highlight reconstruction | Working | Blown speculars stay neutral instead of going magenta |
| **Export** | `Engine::export_at`, `ExportSettings` | Working | JPEG, PNG, TIFF, WebP; 8 or 16 bits (PNG, TIFF); resize, output sharpening; EXIF and an sRGB profile embedded; cropped & oriented, encoded off the UI thread |
| **Sidecar** | `save_sidecar`, `load_sidecar` | Working | Non-destructive `.duckfoot` JSON sidecars, auto-load/save |
| **App** | inspector, undo/redo | Working | Snapshot undo, one checkpoint per gesture |
| **App** | inspector map 3D | Working | Rotatable 3D Ribbons, 3D Surface, 3D Cube, 2D Plot |
| **App** | filmic 3D tonemap | Working | 3D transfer surface, RGB cube deformation, handles, presets |
| **App** | inspector sidebar | Working | Module cards with live values, Customize tab (show/hide, reorder), width toggle |
| **App** | history snapshots | Working | Named stack snapshots with instant recall |
| **App** | history list | Working | Every undo step labelled by what changed; click a row to jump to it |
| **App** | file open, drag-and-drop | Working | Multi-file XDG portal ingest + drag & drop; decoded and uploaded to the GPU off the UI thread |
| **App** | filmstrip thumbnails | Working | The camera's embedded preview, decoded at reduced size on a worker per spare core; from the mosaic only when a file has none. The open photo shows its live render. Only tiles in view are laid out, so a 2000-photo roll costs the same per frame as a 20-photo one |
| **App** | open stand-in | Working | The embedded preview is on the stage in ~20–35 ms while the full decode runs |
| **App** | the shelf | Working | Collapsible asset manager, switch photo, copy/paste stack |
| **App** | before / after split view | Working | Draggable divider (`\`), raw baseline vs active stack |
| **App** | crop tool overlay | Working | Rule-of-thirds grid, aspect ratio presets (`C` / `Esc`) |
| **App** | export | Working | Export dialog (Ctrl+E) and the phone's Export sheet share one settings panel with the batch recipes |
| **App** | zoom and pan | Working | Scroll to zoom about the cursor, drag to pan, `0`/`1` |
| **Engine** | `Histogram`, `poll_histogram` | Working | GPU compute + atomics, 4 KB async readback |
| **Engine** | preview at screen resolution | Working | Sensor mip chain; the preview renders at the coarsest level still as dense as the screen. 24 MP at fit: 3.5 ms/tick vs 28.6 ms at full res (Intel iGPU). Export always renders full res |
| **Platform** | Windows / macOS / Android | Built in CI | Android untested on devices; Windows and macOS experimental (tests run in CI, no manual smoke test yet) |

### Measured

Sony A7 III (ILCE-7M3), 24.3 MP, **Intel integrated graphics** (RPL-S, Vulkan):

```
decode              77 ms    6048×4024
upload + demosaic  220 ms    once per image
slider drag        6.1 ms/frame at FULL sensor resolution
export readback    61 ms     92 MB
png encode+write  205 ms
```

These are the **pessimistic** numbers — preview renders at full sensor resolution.
Fit-to-window preview is not implemented and should cut the per-frame cost further. Frame
time varies with how much of the chain is enabled; a bare stack is ~6 ms and everything on
is ~19 ms, both on integrated graphics.

The bench also verifies correctness, not just speed. Each assertion exists because a bug
got past a weaker one:

| Check | Catches |
|---|---|
| export equals preview, byte for byte | a second render path drifting (invariant 2) |
| full-frame contrast (stddev > 8) | a module crushing the tonal range — this is how the broken filmic hid |
| clipped highlight stays neutral | white balance gains colouring blown speculars |
| neutral stripes gain no colour | the demosaic inventing chroma on fine detail |
| a flat frame fills exactly one histogram bin | scopes wired to the wrong texture, or showing a stale readback |

The last two use synthetic ground truth rather than a photograph, and both were verified to
**fail** when the fix they guard is removed — a test that only passes proves nothing.

### Known gaps and caveats

- **Pixel 8 Pro DNG reports white balance `[1.0, 1.0, 1.0]`.** Almost certainly rawloader
  not reading that file's as-shot neutral rather than a genuinely neutral shot. The Sony
  reads correctly at `[2.03, 1.00, 1.80]`. Worth re-checking against `rawler`.
- **Demosaic is Hamilton-Adams**, not RCD or AHD. It resolves fine detail without the
  purple edge fringing the old box filter produced, and scores zero false colour on a
  ground-truth neutral test pattern. RCD would do better on diagonal and near-Nyquist
  detail; this is a drop-in replacement point and nothing downstream depends on it.
- **Highlight reconstruction recovers colour, not detail.** A blown specular is rendered
  neutral rather than magenta, but texture lost at capture stays lost.
- **No colour management beyond sRGB.** Output is sRGB; display profiles are ignored.
- **`Engine::render` submits its own command buffer** rather than sharing the frame's
  encoder. Costs an extra submit per frame; irrelevant at current scale, worth revisiting
  if frame pacing ever matters.

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
- [ ] Settle the LGPL-2.1 distribution question **before** any binary ships

### Stage 4 — Barrel 1 proper

- [x] Extended pipeline modules (highlights & shadows, vibrance, capture sharpen, vignette)
- [ ] Remaining modules (tone equaliser, local contrast, profiled denoise, lens correction)
- [ ] Better demosaic (RCD or AHD)
- [x] The Shelf — asset list shared across barrels with multi-file ingest
- [x] Copy stack / paste across photos
- [x] Sidecar persistence (`.duckfoot` companion JSON files)
- [x] Customizer drawer, module reordering, smooth scrolling, and sidebar expansion
- [ ] Swap to `rawler` for camera coverage
- [ ] Delete `apps/` and `packages/`

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

**Unresolved, and it blocks distribution.** RAW decoding uses `rawloader`, which is
**LGPL-2.1**. Static linking into an MIT binary carries a relink obligation that must be
settled before any binary is distributed. This does not block local development or building
from source. The alternative, `rawler`, is LGPL-2.1 as well — so this is a question about
distribution, not about which crate to pick.

The UI bundles three typefaces under the **SIL Open Font License 1.1** — JetBrains Mono,
Space Grotesk and Hanken Grotesk. Their licences sit beside the files in
`crates/photo-app/assets/fonts/`.

Duckfoot previously contained code derived from
[OpenCut](https://github.com/OpenCut-app/OpenCut) (MIT); that code is superseded, and their
copyright notice is retained in git history.

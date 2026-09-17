# 🦆 Duckfoot

**A local-first FOSS creative suite: focused Photo, Audio and Video editing.**

> **Status: pre-alpha. Barrel 1 (Photo / RAW) proof of concept. Linux verified; other
> platforms not yet built.**
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

| | |
|---|---|
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| Drag a slider | live re-render; one undo step per gesture, not per frame |
| Drag in the curve box | move the nearest node; endpoints keep their x |
| Checkbox by a module name | enable/disable that stage of the chain |

Hotkeys deliberately do not fire while a text field has focus.

### Supported files

`.cr2 .cr3 .nef .nrw .arw .srf .sr2 .dng .orf .rw2 .raf .pef .raw`

Decoding is `rawloader`'s — a few hundred camera models. Files it cannot handle fail with
the filename and a reason in the status bar rather than silently rendering garbage.

---

## Testing and verification

### Unit tests

```bash
cargo test --workspace
```

12 tests covering the colour-matrix maths (inversion, neutral-maps-to-neutral,
singular-matrix fallback), white-balance normalisation including garbage input, and the
monotone tone curve (identity, no reversal between close nodes, passes through control
points, degenerate input).

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

### Windows, macOS, Android — not yet built

**These have not been attempted and no claim is made that they work.** The architecture is
arranged so they should be a new bootstrap rather than a port — `photo-engine` and
`photo-app` hold no window handle — but that is a design intention, not a tested result.

Expected shape when we get there:

```bash
# Windows (from Linux)
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu

# macOS — must build on a Mac; no cross-compilation
cargo build --release --target aarch64-apple-darwin

# Android — needs NDK + cargo-ndk + a gradle harness, and a new
# crates/duckfoot-android/ bootstrap using android-activity
```

The Android lifecycle work is already written into `crates/duckfoot/src/main.rs`:
`suspended` drops the surface and everything bound to it, `resumed` rebuilds. That is the
classic Android wgpu crash and it is handled. The NDK toolchain and APK packaging are not.

**Do not distribute a binary yet** — see the licence note at the bottom.

---

## Repository layout

```
duckfoot/
├── DESIGN.md                    # the design brief — start here
├── Cargo.toml                   # workspace root
└── crates/
    ├── photo-engine/            # wgpu device, textures, shaders, RAW decode
    │   ├── src/decode.rs        #   rawloader → cropped CFA mosaic + metadata
    │   ├── src/engine.rs        #   textures, pipelines, uniforms
    │   ├── src/curve.rs         #   monotone cubic tone curve → LUT
    │   ├── src/params.rs        #   the adjustment stack (plain data)
    │   ├── src/shaders/         #   demosaic.wgsl, adjust.wgsl
    │   └── examples/            #   probe.rs, bench.rs
    ├── photo-app/               # egui UI — no windowing dependency
    │   ├── src/lib.rs           #   inspector, undo, curve editor
    │   └── src/theme.rs         #   DESIGN.md look and feel
    └── duckfoot/                # bootstrap: winit. The only platform crate
```

`apps/` and `packages/` are the superseded TypeScript build, pending removal.

---

## Current state, honestly

A symbol appears here only once it works end to end, verified against a real file — see
DESIGN.md invariant 8.

| Area | Symbol | Status | Notes |
|---|---|---|---|
| **Decode** | `RawloaderBackend` | Working | Applies `crops`, real `cfa`, `wb_coeffs`, `xyz_to_cam`, per-channel levels |
| **Decode** | `RawDecoder` trait | Working | `rawler` is a drop-in behind it |
| **Decode** | non-mosaic (`cpp != 1`) | `@stub` | Refuses loudly rather than mis-indexing |
| **Decode** | float-encoded RAW | `@stub` | Some DNGs; refuses loudly |
| **Decode** | 4-colour CFA (CYGM / X-Trans) | `@stub` | Only 2×2 RGB patterns |
| **Engine** | `Engine::load` | Working | Upload + demosaic, once per image |
| **Engine** | `Engine::render` | Working | Fused adjustment pass, one uniform write per change |
| **Engine** | demosaic | Working | 3×3 neighbourhood, general over any 2×2 CFA |
| **Params** | white balance, exposure | Working | |
| **Params** | tone curve | Working | Monotone cubic, 1024-entry LUT |
| **Params** | filmic, colour balance | Working | |
| **Engine** | highlight reconstruction | Working | Blown speculars stay neutral instead of going magenta |
| **Export** | `Engine::export`, `ExportedImage` | Working | Full-res PNG/JPEG, encoded off the UI thread |
| **App** | inspector, undo/redo | Working | Snapshot undo, one checkpoint per gesture |
| **App** | file open, drag-and-drop | Working | XDG portal dialog |
| **App** | export | Working | Export… button, format from the filename |
| **App** | preview at fit-to-window scale | Not built | Renders at full sensor res today |
| **App** | histogram / scopes | Not built | |
| **App** | crop / rotate | Not built | |
| **Platform** | Windows / macOS / Android | Not built | Bootstrap only exists for Linux |

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

The bench also verifies correctness, not just speed: export matches the preview byte for
byte, the full frame has real contrast, and a synthetically clipped highlight must come out
neutral. Each of those assertions exists because a bug got past a weaker one.

### Known gaps and caveats

- **Pixel 8 Pro DNG reports white balance `[1.0, 1.0, 1.0]`.** Almost certainly rawloader
  not reading that file's as-shot neutral rather than a genuinely neutral shot. The Sony
  reads correctly at `[2.03, 1.00, 1.80]`. Worth re-checking against `rawler`.
- **Demosaic is a 3×3 neighbourhood average.** Correct for every 2×2 Bayer layout, but
  softer than RCD or AHD. It produces visible **false colour on fine periodic detail** —
  wire shelving, guitar strings, fabric — as magenta/green speckle. This is the largest
  remaining image-quality defect and the reason a better kernel is the next engine job. It
  is a drop-in replacement point; nothing downstream depends on it.
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
- [ ] Preview at fit-to-window scale; full resolution reserved for export
- [ ] PNG/JPEG export through the same shader — the real test of invariant 2
- [ ] Live histogram via a compute pass and a 4 KB readback
- [ ] Crop and rotate
- [ ] Before/after split view

### Stage 3 — Platforms

- [ ] Windows build and smoke test
- [ ] macOS build and smoke test
- [ ] `crates/duckfoot-android/` bootstrap on `android-activity`
- [ ] NDK + `cargo-ndk` + gradle harness, signed debug APK
- [ ] Verify on a real device: texture limits, memory ceiling on a 24 MP file, suspend/resume
- [ ] Settle the LGPL-2.1 distribution question **before** any binary ships

### Stage 4 — Barrel 1 proper

- [ ] Remaining modules toward the full fixed-order stack (tone equaliser, local contrast,
      capture sharpen, profiled denoise, lens correction)
- [ ] Better demosaic (RCD or AHD)
- [ ] The Shelf — asset list shared across barrels
- [ ] Copy stack / paste to selection
- [ ] Sidecar persistence so edits survive a restart
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

Duckfoot previously contained code derived from
[OpenCut](https://github.com/OpenCut-app/OpenCut) (MIT); that code is superseded, and their
copyright notice is retained in git history.

# Status and roadmap

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

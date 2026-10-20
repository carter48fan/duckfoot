# Architecture

The design brief itself is [DESIGN.md](../DESIGN.md); read it before contributing.

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
       SHIPPING (v0.3)     design only        design only
```

The design brief — what each barrel deliberately refuses to do, and why the pipeline is
shaped the way it is — is in [DESIGN.md](../DESIGN.md). **Read that before contributing.**

### Principles

1. **Focused simplicity.** Dedicated views for dedicated jobs. No timeline outside the
   video editor.
2. **Local-first, zero-cloud.** No logins, no database, no analytics. Your media never
   leaves your device.
3. **Native, on the GPU.** The image is uploaded once. A slider drag writes a uniform
   buffer, not a frame.

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
    │   ├── src/heif.rs          #   opt-in HEIC/HEIF still decode via vendored lc-heif
    │   ├── src/engine.rs        #   textures, pipelines, uniforms, GPU dispatch
    │   ├── src/curve.rs         #   monotone cubic tone curve → LUT
    │   ├── src/params.rs        #   the adjustment stack (plain data + serde)
    │   ├── src/lensfun.rs       #   bundled Lensfun database + DNG opcode lens correction
    │   ├── src/keystone.rs      #   perspective homography + Auto Upright line detection
    │   ├── src/local.rs         #   up to 8 gradient, radial, or sensor-space brush masks
    │   ├── src/heal.rs          #   spot heal, clone, and PatchMatch remove (lc-inpaint)
    │   ├── src/lut3d.rs         #   1D/3D .cube LUT parser and 33³ GPU table baker
    │   ├── src/proof.rs         #   printer and monitor ICC profiles baked to 3D tables
    │   ├── src/gainmap.rs       #   ISO 21496-1 / Ultra HDR gain-map measurement & encoding
    │   ├── src/rembg.rs         #   pure-Rust tract-onnx background cutout & alpha matting
    │   ├── src/sidecar.rs       #   non-destructive .duckfoot companion files
    │   ├── src/xmp.rs           #   XMP read, export packets, safe sidecar merge
    │   ├── src/shaders/         #   demosaic, downsample, denoise, adjust, histogram
    │   └── examples/            #   probe.rs, bench.rs, open_profile.rs, synth_raw.rs
    ├── photo-app/               # egui UI — no windowing dependency
    │   ├── src/lib.rs           #   layout: shelf, stage, filmstrip, inspector, undo
    │   ├── src/shelf.rs         #   asset shelf, ratings/flags, filter bar, virtual copies
    │   ├── src/session.rs       #   opening, switching, sidecar/XMP persistence, copy/paste
    │   ├── src/export_flow.rs   #   single & batch export dialogs and background workers
    │   ├── src/inspector/       #   right-hand module cards, scopes, history, customize
    │   ├── src/on_photo.rs      #   direct-on-canvas TAT, crop, heal, and mask overlays
    │   ├── src/presets.rs       #   built-in Looks and user preset library
    │   ├── src/cutout_studio.rs #   interactive AI background removal & touch-up brush
    │   ├── src/inspector_map_3d.rs # 3D rotatable inspector map & scopes
    │   ├── src/tonemap_3d.rs    #   interactive 3D filmic tonemap & RGB cube
    │   ├── src/theme.rs         #   pitch-black palette, fonts, egui visuals
    │   ├── src/widgets.rs       #   painted controls: chips, rails, module cards
    │   ├── src/loader.rs        #   background decode: opening photos, thumbnails
    │   ├── src/platform.rs      #   file pickers and storage places, supplied by the shell
    │   ├── src/compact.rs       #   the phone layout: navigation bar, one sheet at a time
    │   └── assets/fonts/        #   JetBrains Mono, Space Grotesk, Hanken Grotesk (OFL)
    ├── display-profile/         # OS monitor ICC profile lookup (colord / Win32 / ColorSync)
    ├── vendor/                  # vendored pure-Rust crates (lc-heif, lc-inpaint)
    ├── duckfoot/                # the winit shell (lib.rs) and the desktop main
    └── duckfoot-android/        # android_main and the Java bridge; see android/
```

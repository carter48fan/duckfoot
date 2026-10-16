# Testing and verification

Back to [building](./BUILDING.md) · [docs index](./README.md)

## Unit tests

```bash
cargo test --workspace
```

Over 300 tests across the workspace covering the colour-matrix maths (inversion, neutral-maps-to-neutral,
singular-matrix fallback), white-balance normalisation, RCD and X-Trans demosaicing,
pure-Rust ONNX inference (`u2netp.onnx`), monotone tone curve, banded 100MP+ export,
GPU histogram computation, non-destructive geometry, sidecar persistence round-tripping,
zero-allocation drag invariants (`tests/drag.rs`), 45 headless UI lifecycle tests (`tests/headless.rs`),
and 48 real-GPU lavapipe pixel truth tests (`tests/gpu.rs`).

## Smoke test in BOTH profiles

```bash
cargo run              # debug — do not skip this
cargo run --release
```

**A release-only smoke test is not sufficient.** egui, epaint and wgpu validate heavily
through `debug_assert!`, which is compiled out in release. A bug that panics loudly in
debug can degrade silently in release — an unapplied `TexturesDelta` crashes a debug build
and merely loses the font atlas in a release one. Run debug first.

## Lint and format

```bash
cargo clippy --workspace --all-targets     # clean, zero warnings
cargo fmt --all --check
```

## Inspect what the decoder extracted from a file

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

## End-to-end GPU benchmark with pixel readback

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

## Where the time goes when a photo opens

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

## See a layout without its device

```bash
cargo run --release -p photo-app --example screenshot -- phone.png 412x892 photo.dng edit exposure
```

Renders the whole app to a PNG at any size (in points, drawn at 2× like a phone), with a
photo open and, for the phone layout, a sheet and module chosen. Needs a Vulkan driver;
Mesa's software one (`mesa-vulkan-drivers`, lavapipe) is enough, so it works on a
headless machine.

## Stub audit

```bash
grep -rn '@stub' crates/
```

DESIGN.md invariant 8. The previous audit only grepped the TypeScript packages, which was
precisely where the unfinished code wasn't.

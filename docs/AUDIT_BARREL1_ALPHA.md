# Barrel 1 (Photo) — pre-release audit, alpha

Audited at `0ab2f42` (main after PR #5), 2026-09-26. Scope: everything under `crates/`,
`android/`, `.github/workflows/`, `brand/`, plus the leftover `apps/` + `packages/` tree.
Every line of the Rust, WGSL, Java and workflow sources was read.

**How to read this.** Each finding was checked against the source after it was found. Items
marked ✔ were independently re-checked a second time, line by line, while this report was
consolidated. *(unverified)* means the code points that way but it needs a device or a real
file to confirm. Line numbers are at `0ab2f42`.

**Tooling baseline (all green):** `cargo clippy --workspace --all-targets` clean (0
warnings), `cargo fmt --check` clean, `cargo test --workspace` **85/85 pass** (61 engine, 15
app, 9 headless). But note: the headless suite runs on wgpu's **no-op adapter**, so no
automated test ever executes a shader or checks a pixel. The pixel-level checks in `bench`
run only by hand on real hardware.

---

## Verdict

**Not ready to call finished.** The editing core is genuinely good — the fused one-pass
pipeline, highlight reconstruction, async histogram, background decode with stand-in
previews, and one-undo-step-per-gesture are all well built. But there are **ten release
blockers**. Three of them destroy user work, three ship wrong pixels, two crash the app,
and two are legal/distribution problems that are live *right now*.

| # | Blocker | Class |
|---|---|---|
| B1 | Edits lost on quit, Android backgrounding, or crash — no autosave, and no Save button on phone | data loss |
| B2 | Rating/flagging an unopened photo (or batch-exporting it) writes a sidecar that **drops EXIF rotation**; an unreadable sidecar is silently overwritten with defaults | data loss |
| B3 | While the next photo decodes, sliders/hotkeys **edit the previous photo invisibly**, and that edit is autosaved | data loss |
| B4 | Public binaries are being published despite the LGPL hold — the nightly visibility check is inverted | legal |
| B5 | No third-party licence notices (LGPL text / relink offer / MIT/Apache notices) in any artifact | legal |
| B6 | A corrupt or hostile RAW that panics the decoder **freezes all future opens and thumbnails** for the session; no wgpu error handler, so any validation/OOM error is a panic | crash |
| B7 | Export of anything over ~67 MP **crashes** (single readback buffer > 256 MiB limit) | crash |
| B8 | Batch export of a rotated RAW produces the wrong frame (black bars, wrong crop) | wrong pixels |
| B9 | Filmic double-encodes the transfer function: blacks lifted to ~69/255, mid-grey 184/255 | wrong pixels |
| B10 | Fuji X-Trans RAFs are silently demosaiced as Bayer (maze artefacts, wrong colour) | wrong pixels |

Fix order, and a sketch of each fix, is at the end.

---

## 1. Release blockers — detail

### B1 ✔ Edits are lost on quit, Android process death, and crash
- `crates/duckfoot/src/lib.rs:476` `CloseRequested => event_loop.exit()` — nothing saves.
- `crates/duckfoot/src/lib.rs:454` `suspended()` only drops the window. Android's low-memory
  killer routinely kills a backgrounded photo editor; the stack and undo history live only in
  RAM. (`docs/ANDROID_AUDIT.md` B4 already flags this.)
- The only automatic save is in `finish_open` (`crates/photo-app/src/lib.rs:1346`), i.e. when the
  *next* photo finishes decoding. Quitting mid-decode loses the current photo too.
- The phone layout has **no Save/Revert at all** — `compact.rs` never calls
  `save_current_sidecar`.
- DESIGN.md forbids "are you sure" dialogs, which makes autosave mandatory, not optional.

**Fix:** `PhotoApp::flush()` that writes the sidecar when `stack != saved_stack`. Call it on
`CloseRequested`, `suspended`, `WindowEvent::Destroyed`, and winit `memory_warning`. Also
autosave ~1 s after each undo checkpoint settles.

### B2 ✔ Sidecar writes clobber orientation and unreadable sidecars
- `set_rating_for_path` / `set_flag_for_path` (`crates/photo-app/src/lib.rs:1083-1112`) fall back to
  `load_sidecar(p).unwrap_or(None).unwrap_or_default()`. `PhotoStack::default()` has
  `quarter_turns = 0`, but the real baseline for a portrait RAW carries the EXIF turn
  (`lib.rs:~1395`). The same fallback is used by batch export (`lib.rs:903-909`).
  - Scenario: press `3` on an unopened portrait photo in the filmstrip. It now opens and
    exports sideways, permanently.
- `unwrap_or(None)` treats a sidecar that **failed to parse** (corrupt, or written by a newer
  build) as absent. Then the next rating, or edit-and-switch, overwrites the user's real edits
  with defaults. Parse errors are never shown (`lib.rs:1373-1380`).
- Write failures are swallowed: `let _ = save_sidecar_full(..)` (`lib.rs:1094, 1109`). On a
  read-only card the stars show, but they are never stored.

**Fix:** make `SidecarData.stack` an `Option` so metadata-only sidecars don't carry a stack.
Never write over a sidecar that failed to parse; surface the error with the filename. Report
write failures in the status bar.

### B3 ✔ The inspector edits the wrong photo during a decode
- While photo N+1 decodes, the stage shows N+1's embedded preview (`lib.rs:3202-3206`). But the
  inspector and hotkeys stay live against `self.stack` — still photo N, because
  `engine.image()` is still N (`lib.rs:3815`).
- Press `→`, immediately drag Exposure. Nothing visible changes. Photo N is modified and
  autosaved on `finish_open`.

**Fix:** disable the inspector and the stack hotkeys while `self.loading.is_some()`. Or
keep N's render on stage until N+1 lands.

### B4 ✔ Public binaries despite "Do not distribute a binary yet"
- `.github/workflows/build.yml:272` — `if [ "$PUBLIC_NIGHTLY" = "false" ]; then --draft else
  --prerelease`. With the variable **unset** (the default), the nightly is **public**.
  `android/README.md:23-25` documents the opposite.
- `build.yml:301` — versioned releases use `--prerelease` with no `--draft`, so a `v*` tag
  publishes immediately. The README says it "drafts".
- At audit time the `nightly` release was live and public. A draft with the tag `"v0.1.0 "`
  (trailing space) also exists, because tag validation only checks the `v` prefix.

**Fix:** `[ "$PUBLIC_NIGHTLY" = "true" ] || visibility=--draft`. Add `--draft` to the versioned
release. Validate tags against a semver regex. Unpublish the current nightly until B5 is
settled.

### B5 No licence notices in shipped artifacts
Artifacts bundle only `LICENSE` and the OFL font texts (`build.yml` packaging steps). Missing:
- the LGPL-2.1 text and a relink offer, for rawler **and** rawloader — both are statically
  linked (see §6);
- the MIT/Apache notices for egui, wgpu, winit, image and the rest of the tree.

**Fix:** generate `THIRD_PARTY_LICENSES` with `cargo about` into every artifact. Decide the
LGPL route: offer object files for relinking, move the decoder into a separately replaceable
shared library, or accept source-only distribution.

### B6 A decoder panic wedges the loader; there is no GPU error handler
- `crates/photo-app/src/loader.rs:122-196` — the open worker has no `catch_unwind`. rawler and
  rawloader have ~100 `unwrap`/`expect` sites reachable from file bytes. rawler 0.8's
  image-size cap is commented out (`pixarray.rs:597`), so a header claiming 65535² tries an
  ~8.6 GB allocation and **aborts**.
- A panic kills `duckfoot-open` with `gate.set_opening(true)` still set. After that, every
  thumbnail worker blocks forever in `wait_until_idle` (`loader.rs:221, 228`), and every later
  open spins "loading" forever.
- On Android, `AndroidManifest.xml:50-60` exports VIEW/SEND for `image/*`, so any app can hand
  Duckfoot a crafted file.
- The batch thread (`batch/queue.rs:88-160`) has the same flaw: a panic leaves `is_running()`
  true forever.
- No crate calls `on_uncaptured_error` or `push_error_scope`. wgpu's default handler
  **panics**, so any validation or out-of-memory error (see B7) kills the app, and with B1
  the edits go with it.

**Fix:** wrap every decode, thumbnail and batch job in `catch_unwind(AssertUnwindSafe(..))`,
with a drop guard that resets the gate. Read dimensions from the header first and reject
anything above a pixel cap or `max_texture_dimension_2d` *before* decoding. Install
`device.on_uncaptured_error` to log and surface the error. Wrap the big allocations in error
scopes.

### B7 ✔ Export crashes on large images
- `crates/photo-engine/src/engine.rs:~1000` allocates one readback buffer of `stride × height`.
  The device requests `Limits::downlevel_defaults()` (`crates/duckfoot/src/lib.rs:79`), so
  `max_buffer_size` is **256 MiB**.
- A GFX100 (11648×8736) needs ~407 MB. The result is a validation error, then a panic (B6).
  Any raster over ~67 MP does the same, including 108/200 MP phone JPEGs, which the app
  explicitly opens.
- CPU peak is also high: `export.rs:49` clones the whole RGBA buffer, JPEG adds `to_rgb8`, and
  batch adds `img.clone()` again (`queue.rs:300, 320`). That is ~11 B/px, about 1.1 GB at
  100 MP — a phone will be killed.

**Fix:** render and read back in horizontal bands under `max_buffer_size`, reusing one
buffer. Encode by streaming rows. Drop the clones (`into_rgb8`). Request
`adapter.limits()` rather than downlevel defaults where the adapter allows.

### B8 ✔ Batch export ignores the stack's geometry for output size
- `Engine::export` sizes the target and the crop from `image.width/height`
  (`engine.rs:978, 998`). Only `set_geometry` updates those. RAWs load with
  `Geometry::default()` (`engine.rs:354`).
- `batch/queue.rs:239-249` calls `load` then `export` and never calls `set_geometry`. So
  for any RAW with `quarter_turns = 1/3` the shader rotates into a landscape-sized target. The
  result is cut top and bottom, has black side bars, and the crop lands in the wrong place.

**Fix:** in `export`, compute `stack.geometry.output_size(sensor_w, sensor_h)` and use it
for the target and the crop. Also drop the redundant `crop` parameter; it duplicates
`stack.geometry.crop`.

### B9 ✔ Filmic output is sRGB-encoded twice
- `filmic_channel` (`adjust.wgsl:101-111`) returns a log-domain sigmoid in [0,1], which is
  already display-referred. The chain then treats it as linear, and `linear_to_srgb`
  (`adjust.wgsl:516`) encodes it again.
- With the default parameters (`params.rs:456-464`), scene 0.18 → 0.474 → **184/255**
  (118/255 with filmic off). Scene at `black_ev` → ~**69/255**. That is the "flat and grey"
  look the README's troubleshooting section blames on `latitude`.
- Filmic is also per-channel, so saturated highlights hue-shift toward the primaries.

**Fix:** either invert the OETF after the sigmoid, or design the curve so mid-grey maps to
0.18 linear, black to 0 and white to 1. Then consider applying it to a luminance/ratio model
rather than per channel.

### B10 ✔ X-Trans RAFs are demosaiced as Bayer
- `decode.rs:277-294` and `487-503` check only `is_valid()` and "no fourth colour". They never
  check that the pattern is 2×2. For a 6×6 X-Trans pattern the top-left 2×2 is taken as if it
  were Bayer.
- `.raf` is in `RAW_EXTENSIONS`, so every modern Fuji body opens with maze artefacts and
  wrong colour, and no error is shown.

**Fix (immediate):** `bail!("… X-Trans not supported yet (@stub)")` unless `cfa.width == 2 &&
cfa.height == 2`, plus a test. **Fix (later):** a Markesteijn or 3-pass X-Trans demosaic. Fuji
is a large share of enthusiast RAW shooters.

---

## 2. High

| # | Finding | Where |
|---|---|---|
| H1 ✔ | **JPEG export is always quality 75.** `save_with_format(Jpeg)` uses `JpegEncoder::new`, which defaults to 75. Also, `.tif` or any unknown extension silently writes **PNG bytes** under that name | `photo-engine/src/export.rs:61-70`, `:173` |
| H2 ✔ | **Batch recipes advertise things that don't happen.** `color_profile: AdobeRgb` is never read anywhere, so the "Master Print TIFF (Adobe RGB)" recipe writes untagged 8-bit sRGB. WebP from `image` 0.25 is lossless only, so "Social WebP q82" produces huge lossless files. This violates DESIGN invariant 8 | `batch/recipe.rs:128-146`, `queue.rs:318` |
| H3 ✔ | **Batch "finished" re-fires every frame.** `poll_batch_export` runs every frame, and the queue stays `Finished`. `set_status` refreshes forever and hides every later notice; after any batch error, `self.error` is re-set every frame until restart | `lib.rs:629-650`, `queue.rs:177-218` |
| H4 ✔ | **"Paste to all N" is destructive and not undoable.** It overwrites every shelf sidecar, *including geometry*, so rotation and crop are copied onto every photo. For the open photo it updates `saved_stack` but not `stack`, so the paste never shows, and switching away autosaves the old stack over it. Plain paste also carries crop and rotation | `lib.rs:1192-1229`, button at `:3176-3189` |
| H5 ✔ | **`0`–`5` set a rating; the docs say `0`/`1` are fit/100%.** README:162 and the tooltips ("Fit (0)", "Actual pixels (1)") are wrong. Pressing `1` to zoom rates the photo 1★ *and* writes a sidecar. Fit/100% is really Cmd+0 / Cmd+1 | `lib.rs:1621-1658` |
| H6 | **Batch renamer.** `{date}` falls back to a hard-coded `"20260924"`, because the caller passes `date_override: None`. `{camera}` is always "Camera". `{seq}` follows `HashSet` order, not shelf order. Collisions are checked only against disk, not within the plan, so two sources that map to one name silently overwrite each other (`File::create`) | `batch/renamer.rs:273, 310-343`, `lib.rs:872-928`, `queue.rs:295` |
| H7 | **Histogram and clip % go stale after a drag.** While a readback is pending, measuring is skipped and never retried, and `map_async` doesn't wake egui. On touch (no hover events) the scope routinely shows an intermediate state | `engine.rs:1144`, `lib.rs:1496` |
| H8 | **Clarity/Texture can produce NaN.** `log2(luma(scene(..)) + 0.002)`, and `scene()` applies a camera matrix with negative coefficients, so saturated blue or violet gives negative luma, hence NaN, hence black specks (LEDs, stage lights, flowers) | `adjust.wgsl:164-166, 221-227` |
| H9 | **Preview ≠ export for Sharpen and Texture/Clarity.** The sharpen kernel steps one texel of the *preview* mip, and texture uses `fine_level = max(2, lod+1)`. At fit (lod 2-3) the effect is 4-8× wider than in the export. `bench` compares only at lod 0, so it can't catch this. This breaks the PoC's "export matches the screen" | `adjust.wgsl:218, 416` |
| H10 | **CI signing hygiene.** The keystore sits in **job-level** env (`build.yml:40`), so every step can read it, including third-party actions pinned only to a major tag. The key passwords reach every `build.rs` and proc-macro via `build-apk.sh`'s env. There is no top-level `permissions:` block, the `on: push` trigger fires on every branch, and `appimagetool` is fetched from a moving tag with no checksum | `.github/workflows/build.yml` |
| H11 | **Android library grows without bound.** Every pick is copied into `files/library`. Nothing ever deletes the copies (not even "remove from shelf"), and nothing checks free space first. Two 300-photo imports can take 15-20 GB, reclaimable only via Clear Data, which also deletes the edits | `DuckfootActivity.java:351-375` |
| H12 | **A second `android_main` in the same process gives a black screen.** winit 0.30 refuses to recreate an event loop in one process. After an activity-only destroy ("Don't keep activities", or a system activity kill), `run()` errors and returns. `PROXY` and `Bridge.activity` also still point at the dead activity *(unverified on device)* | `duckfoot-android/src/lib.rs:55-57` |

## 3. Medium

**Correctness / state**
- **Reset-to-default is not saved on switch.** The autosave checks `stack != baseline`, not
  `stack != saved_stack`, so a reset photo reverts to its old sidecar edits (`lib.rs:1346`).
- **Snapshots are global and in-memory.** Recalling snapshot "A" on photo B applies A's
  whole stack, crop included. Make them per photo and persist them in the sidecar
  (`lib.rs:351, 2512`).
- **Undo history is discarded on every photo switch** (`lib.rs:1401`). DESIGN says "Ctrl+Z
  undoes, everywhere, always". Keep a per-path history map.
- **The History list shows "no change" for Clarity, Dehaze and Colour EQ edits**, because
  `describe_change` misses them (`lib.rs:5119-5160`). This is a symptom of the parallel-table
  problem in §7.
- **Crop mode:** Ctrl+Z undoes the previous committed step while the crop is mid-edit.
  Switching photos mid-crop autosaves the unapplied crop. Split view keeps rendering while
  its divider is hidden in crop mode.
- **Stepping ←/→ and swiping ignore the active filter and sort.** Every open also wipes a
  multi-selection built for batch export (`lib.rs:1256-1273, 1393`).
- **Batch export progress freezes.** Nothing requests a repaint while it runs, and the app
  uses `ControlFlow::Wait`. Cancel acts only between jobs, while the UI flips to Idle at once.
- **Exports write in place.** Neither single nor batch export uses temp-then-rename, so a
  crash or full disk leaves a truncated file under the final name, possibly over a good one.
- **No Esc-to-cancel on slider, curve or 3D-handle drags.** DESIGN: "Drag has a live preview
  and a cancel".
- **On touch, tapping a slider jumps its value** (absolute, not relative), so a tap to stop a
  fling-scroll edits a slider (`widgets.rs:330-359`).
- **The shelf row's size badge is a live Remove button, labelled only on hover.** On touch a
  tap removes the photo, with no undo, and it stays in the batch selection.
- **Dropping several files keeps only one.** Each `DroppedFile` calls `open_path`, which
  supersedes the previous one; drops should go through `add_to_shelf`
  (`duckfoot/src/lib.rs:480`).
- **The phone status line sticks on "exported …" forever.** `Export::Done` never returns to
  Idle.
- **The sidecar has no schema version, and nothing on load is validated.** An older build
  drops fields it doesn't know about and then rewrites the file without them.
  `quarter_turns` up to 255 overflows `u8` in debug builds; `ev = 1e39` becomes Inf/NaN in
  the shader; the curve node count is unbounded. The temp name is fixed (`X.ARW.tmp`) and
  there is no fsync before the rename.
- **The raster path flattens to 8-bit sRGB and ignores embedded ICC**
  (`decode.rs:81-85`). 16-bit TIFFs lose precision; Display-P3 phone JPEGs render wrong.
- **The vignette is centred on the uncropped frame**, so off-centre crops get a lopsided
  vignette (`adjust.wgsl:509`).
- **The tone curve operates on linear scene values** while the histogram is display-encoded,
  so the curve doesn't line up with the scope drawn under it (`curve.rs`, `adjust.wgsl:475`).
- **The working space is linear sRGB.** DESIGN describes camera→XYZ→Bradford→working; in
  code it is one `cam_to_srgb` matrix, then a per-channel clamp, which causes hue shifts on
  saturated colours (and H8). DNG ForwardMatrix and dual-illuminant data are used only when
  WB is missing.

**Android**
- **JNI:** `new_string` local refs leak on the permanently attached thread, and
  `exception_check`/`exception_clear` is never called after a failed call. CheckJNI aborts
  debuggable builds (`duckfoot-android/src/lib.rs:113, 143-203, 309-321`).
- **Single export on phone is always a full-resolution PNG** (50-70 MB per photo into the
  gallery). Default to high-quality JPEG.
- **`allowBackup` defaults to true.** The private RAW library goes into device-to-device and
  cloud backup; past the 25 MB quota the whole backup, settings included, is skipped.
  Exclude `files/library`.
- **No `onTrimMemory` / `memory_warning` handling.** The spare textures kept for reuse
  (scratch green plane, pooled mosaic, retired sensor) hold ~400 MB of VRAM at 100 MP
  permanently.
- **The bottom swipe bar and the landscape right rail sit on system gesture edges**, and no
  exclusion rects are set *(unverified)*.

**Platform / CI**
- **When no adapter is found, `resumed()` exits with no visible error.** On Windows,
  `windows_subsystem = "windows"` hides stderr too. Write a log file and show a native
  message box.
- **Tests, fmt and clippy run on Linux only.** Windows is built but never tested (the
  `folder_tree` bookmark test would fail there because `HOME` is unset). There is no macOS
  job, `ci.yml` builds without `--locked`, there is no `cargo deny`/`cargo audit`, no Gradle
  wrapper validation, and `ci.yml` and `build.yml` both build on every push.
- **The docs disagree with CI.** The README says Windows, macOS and Android are "Not built"
  and "Do not distribute a binary", while CI builds and publishes Windows zips, AppImages and
  APKs.

## 4. Low (abbreviated)

- **`main.rs`:**
  - `std::env::args()` panics on non-UTF-8 paths; use `args_os`.
  - Only the first argument is opened, although the `.desktop` file passes `%F`.
- **`duckfoot.desktop`:** `MimeType` omits DNG, and a 1024 px icon is installed into
  `hicolor/256x256`.
- **Settings:** `settings.rs` writes non-atomically; reuse the sidecar's temp-and-rename.
- **Renamer:**
  - It sanitises only space and `/`. That becomes a Windows path-traversal problem once
    `{camera}` is read from EXIF.
  - `{seq:999999999}` allocates ~1 GB.
- **Before/after:** the split's "before" half is sharpened, and the histogram measures the
  mixed frame.
- **Colour EQ:** the band weights don't sum to 1; hue 15° gets 125% and 90°/210° about 50%.
- **Decode:**
  - A failed decode runs up to three times (rawler mmap → rawloader → rawler bytes), and only
    the last error is shown.
  - The rawloader DNG fallback reads black levels by CFA position, so blue gets G2's level.
- **Demosaic:**
  - Edges clamp coordinates, which breaks the CFA phase in the outer 1-2 px (reflect by 2
    instead).
  - Green imbalance between G1 and G2 is not handled.
- **Downsample:** `scaled()` uses `div_ceil` while wgpu mips floor, giving sub-pixel
  misregistration. The comment in `downsample.wgsl:24` is wrong.
- **Thumbnails:** CR3/RAF thumbnails aren't rotated (`tiff_orientation` returns None for
  non-TIFF containers).
- **Hostile input:**
  - The TIFF walker is O(n²) over IFDs.
  - The mmap reader gets SIGBUS if a file shrinks underneath it (SD cards, sync clients).
- **Slider path allocations:** "a slider drag does not allocate" is almost true, but the
  histogram bind group is recreated every measured frame and curve edits clone a `Vec` and
  re-upload the LUT.
- **Hotkeys:**
  - Rotate/flip/paste push an undo step with no image loaded.
  - Hotkeys fire behind the batch dialog.
  - The text-focus guard also swallows hotkeys when a Tab-focused *button* has focus.
- **Desktop:** the status notice doesn't repaint to expire.
- **Dead code:**
  - `Exposure.extended_range`
  - `histogram_plot`
  - `widgets::{star_rating, flag_badge, tab}`
  - `AssetMetadata.color_label`, which has no UI
- **Gradle:** `versionCode` is the CI run number, and local builds get `1`, so a local APK
  can't install over a CI one without an uninstall, which wipes the library.

---

## 5. Performance

The interactive path is in good shape:
- A slider writes one uniform buffer.
- The preview renders at the coarsest mip still as dense as the screen.
- The histogram readback is async, 4 KB, and epoch-guarded.
- Idle cost is zero (`ControlFlow::Wait`).
- The filmstrip layout is virtualised.

The problems are at the edges, and mostly bite on phones:

1. **Export memory and size** — see B7. Banded render/readback fixes both the crash and the
   peak.
2. **Export blocks the UI thread.** `Engine::export` does `poll(Wait, timeout: None)` from
   `lib.rs:549`. A 24-100 MP render and readback stalls the frame, and a device loss hangs
   forever. Submit, map asynchronously, and poll from `update`.
3. **Batch export builds a second `Engine`** with its own full-resolution sensor texture on
   the same device. At 60 MP that is two full sensor textures in VRAM on a phone.
4. **Thumbnails:**
   - Every photo is requested on add, not on visibility.
   - Textures are **never evicted**, even on remove: ~175 KB each, so ~350 MB at 2,000
     photos.
   - Files with no embedded preview fall back to a full mosaic decode on up to 8 workers at
     once (~200 MB each at 60 MP).

   **Fix:** request on visibility, cap with an LRU, and allow one mosaic fallback at a time.
5. **Synchronous disk I/O on the UI thread:**
   - `ShelfItem::new` reads and parses a sidecar and stats the file for every added photo.
   - The folder browser runs `read_dir` + stat **every frame** while open (`lib.rs:2243`).
   - `scan_supported_images` is synchronous.

   On SD cards, NAS shares or Android storage this freezes the UI. Move all of it to the
   loader.
6. **Per-frame waste (minor):**
   - ~5 `PhotoStack` clones per frame, each with a heap curve `Vec`.
   - The History panel rebuilds up to 257 formatted rows per frame.
   - Search lower-cases every filename twice per frame.
   - The curve LUT for the editor preview is rebuilt every frame.
   - The 3D ribbons allocate ~250 `Vec`s per frame.

   None of these is urgent; all are cheap to cache.

## 6. Security

It's a local-first app with no network code, so the attack surface is **untrusted files**
and the **build pipeline**.

- **Hostile media (B6):** nothing catches decoder panics, rawler has no size cap, dimensions
  are checked only after the full CPU decode, and the TIFF walker has no budget. Android
  exposes this to any app through VIEW/SEND intents. Add `catch_unwind`, a header-first size
  check, and ideally a `cargo fuzz` target for the TIFF walker, JPEG scanner and
  `decode_bytes`.
- **Two LGPL RAW parsers are linked** (rawloader first, rawler as fallback). That doubles
  the untrusted-input surface *and* the LGPL surface. rawloader 0.37 is effectively
  unmaintained. Finish the move to rawler and drop rawloader.
- **Sidecar trust:** values are deserialised without clamping (see §3), which can drive NaN
  or Inf into the shader or overflow `u8` in debug builds. Sanitise once on load.
- **CI / supply chain (H10):**
  - Scope the signing key to a protected `environment:` job that signs the finished APK with
    `apksigner` *after* the cargo build.
  - Pin third-party actions by SHA.
  - Add `permissions: contents: read`.
  - Checksum-pin `appimagetool`.
  - Add `cargo deny` (advisories + licences), which also gives an automated LGPL/licence
    inventory for B5.
- **Android:**
  - Exclude the library from backup.
  - Keep `safeName` (it is good).
  - Check free space before import.
- **Path handling:**
  - A user rename pattern with `../` or an absolute path escapes the output directory
    (self-inflicted, low).
  - `{camera}` needs full sanitising once it comes from EXIF.
- **Dead TypeScript tree:** `apps/` + `packages/` + `pnpm-lock.yaml` (151 tracked files, a
  second Rust lockfile under `apps/web/src-tauri`). Nothing references them, but they will
  attract Dependabot noise and confuse audits. Delete them, as Stage 4 already plans.

## 7. DRY and structure

`crates/photo-app/src/lib.rs` is **6,401 lines**; `compact.rs` is another 1,744.

1. **Seven parallel per-module tables that already disagree:**
   - `title` (`lib.rs:79`), `module_enabled` (`:4895`), `module_on` (`:4914`),
     `summarise` (`:4935`) and `describe_change` (`:5119`, which misses 3 modules);
   - `reset_module` and `module_short_name` in `compact.rs`;
   - `presets::apply`'s field list;
   - `PhotoStack::bitmask` in the engine, plus 8 hand-written `eq_*` uniform blocks.

   Replace them with **one module registry**: id, titles, enabled accessor, reset, summary,
   diff name, uniform packer, UI body. Adding a module should touch one place. Today it
   touches about nine, and the History bug shows what happens when one is missed.
2. **Mutations aren't funnelled through one place.**
   - Checkpoint logic is duplicated in `pipeline_body` and `compact::single_module`.
   - Rotate/flip is written three times; undo/redo buttons three times; the rating/flag
     context menu twice; `set_rating_*` and `set_flag_*` are copy-paste pairs.

   A `Command` enum with one `apply()` that always checkpoints would enforce DESIGN
   invariant 3 by construction rather than by review. It would also be the natural place to
   hang autosave (B1).
3. **Two export encoders.** `ExportedImage::write` (q75, PNG/JPEG) and
   `batch::encode_and_save` (quality, TIFF, WebP) differ in behaviour. Single export should
   be a one-job batch through the same encoder and recipe.
4. **Decoder duplication:**
   - `decode_reader` and `decode_reader_rawler` repeat the cpp, float, CFA, black/white and
     WB logic.
   - `orientation_geometry` and `orientation_geometry_rawler` are identical.
   - The DNG WB fallback appears twice.
5. **Three "unused filename" routines** with different suffix styles: Rust `" (2)"`, Java
   `" (2)"`, renamer `"_1"`.
6. **Shader boilerplate:** the fullscreen-triangle `vs()` is in all four shaders, and
   `Demosaic`/`sample_at` is in both demosaic shaders. Use a shared WGSL prelude via
   `include_str!` concatenation.
7. **The 3D scaffolding** (floor, axes, projection) is copy-pasted between
   `inspector_map_3d.rs` and `tonemap_3d.rs`.

**Suggested split of `lib.rs`:**
- `app.rs`: the struct, `new`, `update`
- `commands.rs`
- `history.rs`: plus per-photo snapshots
- `session.rs`: open, autosave, sidecar, rating, paste
- `keys.rs`
- `modules/`: the registry, plus one file per module body
- `curve_editor.rs`
- `desktop/`: top bar, shelf, filmstrip, stage, inspector, batch dialog
- `export.rs`
- `icons.rs`

## 8. Design, UX and accessibility

- **Departures from DESIGN.md's own look and feel:**
  - Gradients: the WB slider rails (`widgets.rs:402-416`) and the look tiles directly under
    the photo (`compact.rs:1554`). `theme.rs:5` itself says "no gradients anywhere".
  - A glow ring on the tonemap handles.
  - Permanent HUD pills over the image.
  - Red for non-destructive clipping warnings.
  - A decorative lime colour.
  - The phone tour auto-starts, against "no onboarding".

  Either bring these back in line or amend DESIGN.md. Right now the doc and the app disagree.
- **Scopes that present invented data as measurement:**
  - The "RGB gamut cube" plots histogram *bin counts* as colour coordinates, with corners
    labelled Red/Yellow/Cyan (`inspector_map_3d.rs:716-747`).
  - The tonemap "Spectrum" axis is a made-up ±20% gain.

  In a colour tool this misleads; relabel or remove them.
- **Accessibility:**
  - `TEXT_FAINT` #555 on black is ~2.8:1 and carries instructions.
  - Touch targets are 28-34 pt (chips, buttons, bar icons); aim for ≥44.
  - Landscape tablets get the desktop layout: 14 px checkboxes, 22-24 px rows.
  - Labels are 7.5-9.5 pt, with no font-scale setting.
  - Painter-drawn widgets have no focus ring and no AccessKit labels.
- **Layout parity:**
  - The phone has no Save/Revert, copy/paste, snapshots or History list.
  - The desktop has **no presets UI** (`presets::LOOKS` is used only on phone).
- **Persistence:** only `tour_done` and the phone module list survive a restart. Lost each
  time:
  - desktop module order and visibility
  - panel widths
  - shelf open/closed
  - export recipes
  - the rename pattern
  - the last folder
  - **the shelf itself**
- **Discoverability:** there is no shortcut help overlay, and the README's Controls table is
  out of date (see H5). It also omits P/X/U, 0-5, Z, Ctrl+A, Ctrl+Shift+E and Enter.

## 9. Missing features

Checked by grepping the source for each. Present already, and good:
- straighten (±15°), 8-band HSL (Colour EQ), clarity/texture/dehaze, tonal
  highlights/shadows/whites/blacks, vibrance, vignette, sharpen, lift/gamma/gain
- ratings, flags, filter, sort and search
- batch export with recipes and renaming
- before/after split, snapshots, a jump-to-step History list
- copy/paste stack, phone presets, pinch zoom

### Critical — expected in any "finished" photo editor, even an alpha

1. **Autosave and crash recovery** (B1). Non-negotiable given the no-confirmations design law.
2. **Export options on single export:** JPEG quality, long-edge resize, format (at least
   JPEG/PNG/**16-bit TIFF**), and **metadata** — copy EXIF (capture date, camera, lens,
   GPS, with an option to strip) and **embed an sRGB ICC profile**. Today exports carry no
   metadata at all, so they sort wrongly in every gallery and photo manager, and they are
   8-bit only.
3. **Noise reduction.** None exists. Any phone RAW or high-ISO file will look worse than the
   camera JPEG. Even a simple chroma + luma bilateral/guided filter in the fused pass would
   close most of the gap.
4. **Lens correction**: at minimum lateral chromatic aberration and profile vignetting,
   ideally distortion via lensfun data. Wide-angle and phone RAWs look visibly wrong without
   it. Straighten without **auto-crop** also leaves transparent or black corners.
5. **White-balance eyedropper and Auto tone / Auto WB.** They are the most-used one-click
   fixes, and the histogram and readback plumbing already exist.
6. **On-image clipping overlay.** Today there is only a % badge; users expect to see which
   pixels are clipped.
7. **Crop aspect lock and edge/corner handles.** Today a crop can only be redrawn or moved.
8. **X-Trans support** (B10), or at least a loud refusal. Also 16-bit and ICC-aware raster
   input.
9. **Session persistence**: reopen the shelf, and the last photo, on launch.
10. **An EXIF/metadata panel**: shutter, aperture, ISO, focal length, lens, date. The loader
    already parses TIFF.

### Worth considering — high value, fits the "focused, fixed-order" thesis

- **Local adjustments**, a small fixed set: linear gradient, radial and brush masks driving
  exposure/WB/saturation. This is the most-requested feature class in every RAW editor.
  Kept to a fixed set of mask types, it doesn't become a node graph.
- **Spot heal / clone** for sensor dust, a single-purpose tool.
- **Selective paste / sync settings** (per module, excluding geometry by default) and
  **user presets** (save, import, export).
- **Virtual copies**, several stacks per file, which snapshots nearly are already.
- **Per-photo persistent undo and snapshots** in the sidecar.
- **Perspective / keystone correction.**
- **Better demosaic** (RCD) and **highlight *detail* recovery** (inpaint from unclipped
  channels), both already on the roadmap.
- **Wide-gamut working space** (linear Rec.2020 or ProPhoto) with a gamut-mapped output
  transform. It fixes the saturated-colour clamping and hue shifts, and enables Display-P3
  and Adobe RGB export for real.
- **Soft proofing / output sharpening** for print.
- **Colour-label UI** (the data field already exists), keyword tags, a recursive folder
  import.
- **Shortcut overlay (`?`)**, a desktop presets panel, a font-scale setting.
- **Share sheet on Android** (export, then share), and **delete from library**.
- **Localisation scaffolding.** Cheap now, expensive later.

---

## 10. Doc drift

- **README "Current state":**
  - It says "Fit-to-window preview is not implemented", then marks it working two rows
    later.
  - It says the platforms are "Not built" while CI publishes them.
  - It says "31 tests" (there are 85).
  - It says decoding is `rawloader`'s, but rawler is the default (`decode.rs:169`).
- **README Controls:** `0`/`1` are wrong (H5), and 10 hotkeys are missing.
- **DESIGN.md:**
  - "A slider drag writes 32 bytes": the uniform is 480 bytes, and `engine.rs:66` says 176.
  - The Colour section describes an XYZ/Bradford working space that the code doesn't have.
- **`android/README.md`:** the nightly visibility is inverted relative to the workflow (B4).
- **`curve.rs`:** the comment says values above 1 use the curve's top slope; the shader uses
  slope 1.
- **Versioning:** the workspace says `0.1.0`, a commit says "v0.2.0", there are no real tags
  (just a draft `"v0.1.0 "`), and there is no CHANGELOG.

## 11. Done well

- **The architecture holds up.**
  - The engine and app crates never touch a window.
  - The shell is shared between desktop and Android through a small `Platform` trait.
  - Suspend drops only the surface.
- **The fused single-pass adjustment shader, uniform-only updates, mip-selected preview and
  immutable sensor texture.** The invariants hold in code, not just in prose.
- **Colour and highlights:**
  - WB is applied before the matrix, with dcraw-style normalisation tested against real
    numbers.
  - Highlight reconstruction is split into rgb and neutral parts, so it survives the mips.
    The fix and its test are both excellent.
  - The Kelvin/tint maths is an exact inverse pair, with tests.
- **Loading:**
  - Opens are coalesced with generation counters.
  - The embedded-preview stand-in is on stage in ~20-35 ms.
  - Thumbnail work is gated behind opens, with a smaller worker pool on Android.
- **Undo:** one undo step per gesture, with a crop session as a single step, and a History
  list you can jump through.
- **Safety details:** the Java import's `safeName`, `.part`→rename, and re-pick dedup; atomic
  sidecar writes; the IFD-cycle guard; `unsafe` confined to one documented mmap; no
  `pull_request_target` in CI.
- **Candour:** the README's "Current state, honestly" and `ANDROID_AUDIT.md` are unusually
  honest documents. They're why this audit could be precise.

---

## Suggested order of work

1. **Stop the bleeding (a day):**
   - Invert the nightly check and draft versioned releases (B4).
   - Unpublish the current nightly.
   - Add `catch_unwind` around decode and batch jobs, plus `on_uncaptured_error` (B6).
   - Refuse non-2×2 CFAs (B10).
   - JPEG quality 92 (H1).
   - Fix the batch status loop (H3).
   - Fix the README's `0`/`1` (H5).
2. **Never lose work (2-3 days):**
   - `flush()` on close, suspend and memory warning, plus debounced autosave (B1).
   - Metadata-only sidecars; never overwrite an unparseable sidecar (B2).
   - Lock the inspector during a decode (B3).
   - Make paste-to-all exclude geometry and be undoable (H4).
   - Save/Revert on phone.
3. **Correct pixels (2-4 days):**
   - Filmic transfer (B9).
   - Size export from the stack's geometry (B8).
   - Banded export under `max_buffer_size` (B7).
   - Clarity NaN guard (H8).
   - Level-independent sharpen and texture radii (H9).
   - Add lavapipe to CI so a real-GPU pixel test (preview == export at lod > 0) runs on
     every push.
4. **Ship legally:**
   - `cargo about` notices and an LGPL decision (B5).
   - Drop rawloader.
   - Harden signing in CI (H10).
5. **Alpha feature floor:**
   - Export options + EXIF/ICC.
   - Noise reduction.
   - CA and vignette lens correction.
   - WB picker + auto tone.
   - Clipping overlay.
   - Crop handles and aspect lock.
   - Session persistence.
6. **Refactor as you go:** the module registry and the `Command` enum. Do them *before*
   adding local adjustments, which would otherwise multiply the parallel tables.

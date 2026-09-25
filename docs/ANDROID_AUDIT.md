# Android release-candidate audit

Audited at `d39083c` (2026-09-25). Scope: cross-compilation, and everything except the
phone UI layout that stands between the current tree and an Android release candidate. The
last section maps every current feature onto the proposed bottom-navigation layout so none
is lost.

## Progress

| # | State |
|---|---|
| B1 | **Done.** `photo-app` no longer depends on winit |
| B2 | **Done, interim.** `rfd` is desktop-only behind `photo_app::platform::Platform`; Android pickers are SAF intents in `DuckfootActivity` |
| B3 | **Interim.** Picked documents are *imported* (copied) into app-private storage, so paths and sidecars work unchanged; exports go to `Pictures/Duckfoot`. The `AssetId` / `Storage` model below is still the goal |
| B4 | **Done.** Suspend drops only the window surface; the device and `PhotoApp` survive. Back closes crop/dialogs, then backgrounds the app. Process death still loses unsaved edits |
| B5 | **Done, pending a device.** `crates/duckfoot-android`, `android/` (Gradle, NativeActivity subclass), `android/build-apk.sh`, and the Android workflow |

| UI | **Done.** `photo-app/src/compact.rs`: the navigation-bar layout below 900 × 560 points, both orientations; touch sizes for every control. The touch-parity list below is covered except item 6 (the shelf row's hover-only remove badge, which select mode's "Remove from shelf" replaces) and item 8 (Shift-click for a second open module, which the one-at-a-time sheet makes moot) |

NativeActivity was chosen over GameActivity: the GameActivity library must match
android-activity's bundled C sources exactly, and a Java subclass of NativeActivity can
receive picker results just as well. The cost is a weaker soft keyboard (R6).

## Verdict

The engine is ready. `photo-engine` compiles for `aarch64-linux-android` unchanged, and
downlevel limits, the non-filterable float path and the preview mip levels were designed with
mobile GPUs in mind. **Five blockers remain outside the UI**, and two of them are trivial:

| # | Blocker | Size |
|---|---|---|
| B1 | `photo-app` depends on `winit`, which fails to compile for Android | trivial, verified fix |
| B2 | `rfd` file dialogs have no Android backend and do not compile | small once B3 is designed |
| B3 | All storage is `PathBuf` + `std::fs`, which Android scoped storage does not allow | **the big one** |
| B4 | Backgrounding the app destroys the whole `PhotoApp`: edits, undo, shelf | medium |
| B5 | No Android entry point, manifest, packaging or CI job | medium, well-trodden |

Plus the README's own distribution blocker (LGPL `rawloader`/`rawler`), which applies to an
APK exactly as it does to a Linux binary.

## What was actually verified

| Check | Result |
|---|---|
| Toolchain | egui 0.36.2 needs **rustc ≥ 1.95**; 1.94 fails at resolve. Add `rust-version` to the workspace |
| `cargo check --target aarch64-linux-android -p photo-engine --all-targets` | ✅ as is |
| same, `photo-app` | ❌ B1 (`android-activity` has no backend selected), then ❌ B2 (`rfd`) |
| `photo-app` + `photo-engine` `cargo build --release --target aarch64-linux-android` with B1 fixed and rfd stubbed | ✅ both rlibs build |
| `duckfoot` bootstrap + a stub `android_main` cdylib, `cargo check` for Android | ✅ |
| Linking the cdylib | ❌ no NDK in the audit sandbox (only the linker is missing, not a code problem) |
| Desktop build + `cargo test` with the B1 fix applied | ✅ 65 tests pass |
| `cargo check --workspace` for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` | ✅ both, unchanged |

**Not verified:** linking, APK packaging, and anything on a real device or GPU (Adreno, Mali).
GitHub's `ubuntu-latest` runners ship an NDK (`$ANDROID_NDK_LATEST_HOME`), so the link step can
be proven in CI (see B5).

---

## Blockers

### B1: `photo-app` pulls in winit, contradicting DESIGN.md

DESIGN.md says `photo-app` "knows nothing about winit". The code agrees, since nothing in
`crates/photo-app/src` names winit, but the manifests do not:

- `crates/photo-app/Cargo.toml` lists `winit.workspace = true`, which is unused.
- The workspace `egui-wgpu` enables `features = ["winit"]`, and nothing uses `egui_wgpu::winit`.

Built for Android on its own, `photo-app` therefore drags in `android-activity` with neither
`native-activity` nor `game-activity` enabled, which is a hard `compile_error!`. **Fix:** delete
both. Desktop builds and all tests still pass with this change.

### B2: `rfd` has no Android backend

There are five call sites: `pick_and_open` (lib.rs:841), `pick_and_shelve` (:856),
`pick_and_export` (:872), and the two folder pickers (:608, :1915). The API shape is also wrong
for Android, not only the crate: `rfd` blocks and returns `Option<PathBuf>`, whereas the Android
pickers (`ACTION_OPEN_DOCUMENT`, `ACTION_OPEN_DOCUMENT_TREE`, `ACTION_CREATE_DOCUMENT`) are
asynchronous activities whose result arrives in `onActivityResult` as a `content://` URI.

**Fix:** move `rfd` to `[target.'cfg(not(target_os = "android"))'.dependencies]`, and have the
bootstrap hand `PhotoApp` a picker trait, the same way it already hands over `Gpu`. The picker
starts a request and the app polls for its result, like `Loader` does today. The desktop
implementation wraps `rfd` and resolves immediately; the Android one goes through JNI.

`NativeActivity` cannot receive activity results without a Java subclass, so **use
`GameActivity`** (`winit` feature `android-game-activity`) with a small Kotlin activity that owns
the picker intents. That shim also brings a working soft keyboard (R6) and predictive back (B4).

### B3: the storage model assumes a POSIX filesystem

`PathBuf` is the identity of an asset everywhere: the shelf, `thumbs: HashMap<PathBuf, _>`,
selection, the loader, sidecars and batch jobs. Under scoped storage (Android 10+, enforced from
11) this breaks in five ways:

| Feature | Today | Why it fails on Android | Replacement |
|---|---|---|---|
| Open / add to shelf | `fs::read(path)`, rawler/`memmap2` mmap | pickers return `content://` URIs, not paths | `ContentResolver.openFileDescriptor` → raw fd → `std::fs::File`. mmap works on that fd, and `/proc/self/fd/N` works where a path is required |
| Sidecars, including ratings and flags | `<raw>.duckfoot` written beside the RAW (`sidecar.rs`) | a single-document grant gives no sibling access, and non-media files cannot be written into `DCIM/` | a sidecar store trait: app-private `files/sidecars/<key>.duckfoot` keyed by document URI or content hash, and beside the RAW only when a tree grant allows it |
| Folder sessions, bookmarks | `$HOME`, `$HOME/Pictures`, `read_dir` (`dam/folder_tree.rs`) | `$HOME` means nothing, and `read_dir` outside app storage is denied | `OPEN_DOCUMENT_TREE` + `DocumentsContract` child queries, with persistable URI grants as bookmarks; optionally MediaStore with `READ_MEDIA_IMAGES` |
| Export, batch export | save dialog → path; `batch_output_dir`; `fs::create_dir_all` (`batch/queue.rs:272`) | same as above | `CREATE_DOCUMENT`, a tree grant, or a MediaStore insert into `Pictures/Duckfoot` |
| Launch with a file, drag and drop | `argv[1]`, `WindowEvent::DroppedFile` | neither exists | handle `ACTION_VIEW` / `ACTION_SEND` intents ("Open with Duckfoot" from the gallery) |

**Recommended shape:** an opaque `AssetId` (a path on desktop, a URI on Android) plus a
`Storage` trait (`open_read`, `list_folder`, sidecar `load` / `save`, `create_output`) in
`photo-app`, with the implementation supplied by the bootstrap. The decode, thumbnail and export
code already works on bytes or an open `File`, so the change is at the edges, not in the pipeline.

`MANAGE_EXTERNAL_STORAGE` would make paths work, but Play only grants it to file-manager-type
apps. It is fine for sideloaded developer builds as a stopgap, not for a release.

### B4: suspend throws away the user's work

`crates/duckfoot/src/main.rs:353`, in `suspended()`:

```rust
self.app = None;
self.surface = None;
```

The comment on `struct Surface` says "the `PhotoApp` outlives it"; the code does not keep it.
On Android, `suspended` fires on every app switch and screen-off, **and when the system file
picker opens**. So the picker round-trip from B2 would hand the chosen file to a freshly reset
app, with the open photo, adjustment stack, undo history, snapshots, shelf and selection all gone.

**Fix:** split what dies with the window from what does not.

- **Keep** across suspend: `Instance`, `Adapter`, `Device`, `Queue`, the egui `Renderer`, the
  `egui::Context`, and `PhotoApp`. A wgpu device survives surface loss on Android Vulkan.
- **Drop and recreate:** `Arc<Window>`, `wgpu::Surface`, and `egui_winit::State`. On resume,
  re-create the surface against the existing adapter, and rebuild the renderer only if the
  swapchain format changed.

Also required for a release candidate:

- **Process death in the background:** persist a small session file (the open asset, stack,
  shelf and snapshots) to app storage on suspend, and autosave the sidecar.
- **The back gesture must not end the activity with unsaved edits.** Map it to "close sheet /
  cancel crop / leave select mode" first, via `OnBackPressedCallback` in the Kotlin shim for
  Android 14+ predictive back.
- **Rotation:** set `android:configChanges` so rotating is a `Resized` event, not an activity
  restart.

### B5: no entry point, manifest, packaging or CI

This is known, and the README already sketches it. Concretely:

- A new `crates/duckfoot-android` crate (`crate-type = ["cdylib"]`) exporting
  `#[no_mangle] fn android_main(app: AndroidApp)`, which builds its event loop with
  `EventLoopBuilderExtAndroid::with_android_app`. The `Surface` / `Duckfoot` handler should move
  into a shared module rather than being copied from `main.rs`. A stub of exactly this compiles
  for Android.
- `android_logger` instead of `env_logger`, which writes to stderr, and stderr goes nowhere on
  Android.
- The workspace `winit` / `egui-winit` features are currently `x11` + `wayland` only; the
  Android crate needs `android-game-activity`.
- A Gradle project (or `xbuild`) with a manifest that requires Vulkan
  (`android.hardware.vulkan.version` ≥ 0x401000, see R4), declares `configChanges` and the
  `ACTION_VIEW` intent filters for the RAW MIME types, and covers ABIs `arm64-v8a` plus
  `x86_64` for the emulator. Also release signing.
- A CI job: `cargo ndk -t arm64-v8a build --release -p duckfoot-android`, and ideally an
  emulator smoke launch.

---

## Release-candidate risks (not compile blockers)

| # | Risk | Where | Suggested handling |
|---|---|---|---|
| R1 | **GPU memory.** One open photo holds the Rgba16Float sensor texture with mips (~10.7 B/px) plus the spare R16 mosaic in `UploadPool` (2 B/px): about **0.3 GB at 24 MP and 0.8 GB at 61 MP**. Export adds about 8 B/px briefly, and a load at a new size briefly holds two sensor textures. Android kills heavy background apps quickly | `engine.rs` `UploadPool`, `retired_sensor` | on `onTrimMemory` / suspend, drop the pool spare, the retired sensor and off-screen thumbnails. Worth a Mali 6 GB test device |
| R2 | **Export fails above ~67 MP on every platform.** The readback is one buffer of stride × height, and the device is created with `downlevel_defaults()`, which caps `max_buffer_size` at 256 MiB. wgpu's default uncaptured-error handler panics | `engine.rs` ~l.990, `main.rs` `required_limits` | read back in row bands, or request the adapter's buffer limit |
| R3 | Texture size limit. `check_size` refuses cleanly, but GPUs that report 8192 px refuse 61 MP files (9504 px wide) | `engine.rs:291` | tile or downscale on load as a fallback |
| R4 | The histogram is a compute shader with storage-buffer atomics, so a GLES-3.0-only device fails `request_device`, and the app exits with only a log line | `main.rs` `Surface::new` | require Vulkan in the manifest, and show an error screen rather than `event_loop.exit()`. `caps.formats[0]` also panics on an empty format list |
| R5 | The loader spawns `available_parallelism()` workers plus rayon, which runs hot on big.LITTLE phones | `loader.rs:203` | cap at about 4 on Android |
| R6 | Three text fields need the soft keyboard: the rename pattern (lib.rs:660), snapshot name (:2204) and filter search (:2506). IME support in egui-winit on Android is thin | | GameActivity (B2), and test on a device |
| R7 | The LGPL relink question is still open | README § Licence | settle it before any APK leaves the building, including F-Droid or internal tracks |

---

## Feature parity: the bottom-navigation plan works, with these touch gaps closed

Your plan: a bottom navigation bar with icons, and one module's slider group shown at a time.
Here is where every current feature lands.

Build it as a **width breakpoint** inside `photo-app` (say below 600 dp), not as an Android
code path. Tablets and foldables then keep the desktop layout, and the phone layout can be
exercised in the existing headless test (`tests/headless.rs`) on Linux CI, with no device.

| Tab | Holds (from today's UI) |
|---|---|
| **Library** | folder bookmarks and browse (`shelf_folders`), shelf list, filters and search, ratings and flags, remove item, add photos, "Paste to all N" |
| **Edit** | the stage, plus a horizontal icon scroller of the visible modules in Customize order; the tapped module's controls open in a bottom sheet with its on/off toggle and summary in the sheet header. Crop & rotate is the first icon, and selecting it enters crop mode |
| **Scopes** | histogram, 3D inspector map, and the filmic 3D view at full width (its sliders stay in Edit) |
| **History** | undo/redo list and jump (`history_group`), snapshots (`snapshots_group`), copy / paste stack, reset all, save sidecar, revert |
| **Export** | single export, the batch dialog (recipes, renamer, output location, progress, cancel) |
| Top app bar | undo · redo · a/b split · zoom readout (tap cycles fit / 1:1) · open · sidecar status dot · overflow (Customize modules, telemetry) |

### Things that are keyboard-, modifier- or hover-only today, which would silently disappear on touch

1. **Multi-select** needs Ctrl/Shift-click on shelf rows and filmstrip tiles (lib.rs:2075, :2645).
   Batch export and selection-wide rating/flag depend on it (`execute_batch_export`), so without
   it batch export always exports one photo. Add a select mode, entered by long-press or a
   toolbar button, with tap to toggle and a range option.
2. **Select all** is Ctrl+A only (lib.rs:1403). No button exists anywhere, on desktop either.
3. **Fine slider drag** is Shift-only (widgets.rs:212). Add a precision gesture (long-press
   then drag), or tap the value to type it.
4. **Rating and flag for the current photo** are keys (0–5, P/X/U) or a long-press context menu.
   Long-press does work, because egui opens `context_menu` on a long touch, but it is
   undiscoverable. Put visible stars and flags on the Library row and in the Edit top bar.
5. **Previous/next photo** is ←/→. Add a horizontal swipe on the stage when zoomed to fit
   (at fit there is nothing to pan, so the gesture is free).
6. **The shelf row remove badge** is painted only while hovered (lib.rs:4880), so it is invisible
   on touch. Show it in select mode or behind a swipe.
7. **Tone-map 3D handles** use a 12 px hit radius (tonemap_3d.rs:288), too small for a finger;
   use about 24 px (roughly 48 dp) when the input is touch.
8. **Shift-click to open a second module alongside** (lib.rs:3582) has no equivalent in a
   one-at-a-time sheet. Either accept that as a deliberate phone-layout loss, or let the sheet
   pin a second module.
9. **Ctrl+scroll zoom on the 3D views** is already covered by pinch (`zoom_delta`), and
   **double-click resets** fire on a double tap. No action needed.
10. **Tooltips** carry keyboard hints almost exclusively, so losing them is fine. The one piece
    of real information is "Barrel 2/3 is design only" on the disabled chips; put it in the
    overflow menu.
11. Scrolling the sheet by dragging will grab sliders by accident. In the sheet, give sliders
    a horizontal-drag threshold, or scroll only from the gutter. This is the main reason
    one-group-at-a-time is the right call.

## Suggested order

1. B1: two manifest lines.
2. B5 skeleton plus a CI job that links an arm64 `.so`, so regressions show up from day one.
3. B4: keep `PhotoApp` and the device across suspend. Worth doing before B2, because the
   picker round-trip depends on it.
4. B3 and B2 together: `AssetId`, the `Storage` and picker traits, the desktop implementation
   first (no behaviour change), then the Android one.
5. The phone layout behind the width breakpoint, with the touch-parity list above.
6. R1–R7, then a device matrix pass (at least one Adreno and one Mali device).

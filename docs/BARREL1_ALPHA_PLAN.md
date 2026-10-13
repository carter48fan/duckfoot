# Barrel 1 (Photo) — plan to alpha and beyond

Companion to [`AUDIT_BARREL1_ALPHA.md`](./AUDIT_BARREL1_ALPHA.md). This plan covers
**every** finding in that audit, the missing features it lists, and one new requirement:

> **U-01 (new, replaces the audit's fix for B3):** when you navigate from one image to
> another, the sliders reset. They show the incoming photo's own state immediately, and never
> the outgoing photo's values. The full spec is in [§4](#4-u-01--sliders-reset-on-navigation-full-spec).

Audit IDs `B1–B10` and `H1–H12` are used as written. The audit's unnumbered Medium and Low
findings, and a few the audit agents found that did not make the report's summary, get
`M-xx` / `L-xx` IDs in the [traceability matrix](#11-traceability-matrix), so nothing is
tracked only by prose. Features are `F-xx` (alpha floor) and `W-xx` (worth considering).
Refactors are `D-xx`.

Each work package (WP) is sized as roughly one PR. The plan tracks tasks, not time: there
are no estimates, only what is done and what is next.

**Owner's direction (2026-09):** the repository is private and not yet open source. The
goal is to finish the product first; release candidates and licensing are decided after.
Licensing work (DEC-1, Phase 6's licence items) is **deferred**, not dropped. DEC-2 is
**confirmed** as recommended. Keys, zooms, controls, local adjustments and spot heal are
left to the implementer's judgement, so the recommendations for DEC-3 onwards are adopted.

---

## 1. Shape of the plan

| Phase | Goal | Status | Gate to leave the phase |
|---|---|---|---|
| **0** | Stop the bleeding | done | No public binaries; no known crash from a bad file; X-Trans refused loudly |
| **1** | Never lose work (includes **U-01**) | done | Kill -9 / Android LMK / quit / photo switch never lose an edit; headless tests prove it |
| **2** | Foundation: test harness + refactor | done | Real-GPU pixel tests in CI; module registry; `Command` enum; `lib.rs` split |
| **3** | Correct pixels | done | Filmic, export geometry, banded export, NaN, preview≈export at every zoom — all under GPU tests |
| **4** | Interaction, batch, design | done (recipe editor with F-02) | Batch and renamer correct; drags cancel; accessibility floor; layout parity |
| **5** | Robustness, performance, platforms | next | Fuzzed decoders; memory-bounded on a 4 GB phone; Windows/macOS built *and tested* |
| **6** | Release engineering and licensing | licensing deferred; CI hardening partly done | Licence notices; hardened CI signing; versioned and changelogged |
| **7** | Alpha feature floor (F-01 … F-10) | done | Feature checklist in [§9](#9-alpha-release-gate) |
| — | **Alpha release** | | [§9](#9-alpha-release-gate) all green |
| **8** | Worth considering (W-01 … W-14) | after alpha | Post-alpha roadmap |
| **9** | Gesture-first traversal and aligned bars (G-01 … G-05) | after every feature | Owner's direction, 27 Sep: Snapseed-like, swipe anywhere; every bar on one line; a tutorial that teaches it |

**Critical path:** 0 → 1 → 2 → 3 → 7.

**Parallel tracks:**
- Phase 6 can start any time after Phase 0.
- Phases 4 and 5 can interleave with 3 once Phase 2's refactor has landed.

**Ordering rule:** the refactor (Phase 2) comes *before* the feature floor on purpose. Every
new module today touches about nine parallel tables (audit §7). Local adjustments,
denoise and lens correction would multiply that, and the History bug (M-04) shows what
happens when one table is missed.

---

## 2. Decisions needed from you

Each has a recommendation. The plan assumes the recommendation unless you say otherwise.

| # | Decision | Recommendation |
|---|---|---|
| **DEC-1** | **LGPL route** for rawler (B5) | **Deferred** by the owner until release candidates are chosen; the analysis stands. **Ship with the source.** Duckfoot is itself open source, so LGPL-2.1 §6's relink requirement can be met by shipping the complete corresponding source. Attach `duckfoot-<ver>-source.tar.gz` (with `cargo vendor` output) to every release, include the LGPL text, and add a "Source & licences" entry in About. This takes a day, versus weeks for a C-ABI decoder `cdylib`. *Not legal advice — confirm with whoever advises you.* |
| **DEC-2** | Where do sliders land on navigation (U-01)? | **Confirmed by the owner, and shipped.** On the **incoming photo's own saved edits** if it has a sidecar, otherwise **defaults** plus the camera's orientation. Resetting a previously edited photo to defaults would look like data loss. If you literally want defaults every time, say so; it is a one-line change in WP1.2. |
| **DEC-3** | Keys `0`–`5` (H5) | **Plain `0`–`5` = rating** (the Lightroom and darktable convention). **Cmd/Ctrl+`0` = fit, Cmd/Ctrl+`1` = 100%.** Fix the README and tooltips to match. |
| **DEC-4** | Departures from the look-and-feel rules (M-32) | **Conform the code** for the look tiles' gradients, the glow ring, the permanent HUD pills, the decorative lime, and red for non-destructive warnings. **Amend DESIGN.md** for three things: the WB rail gradient (it is information, not decoration), clipping-overlay colours (F-06), and the tour, which becomes an opt-in "Show me around" chip instead of starting automatically. |
| **DEC-5** | Do local adjustments (W-01) and spot heal (W-02) fit the thesis? | **Yes, as a fixed set:** at most 8 masks, three mask types, one fixed parameter block; heal as a list of spots. No layers, no blend modes, no mask arithmetic. This keeps the fixed-order, fused-pass design intact. |
| **DEC-6** | Lossy WebP (H2) | **Add the `webp` crate** (it builds libwebp, BSD-3, with `cc`, and cross-compiles to Android). The alternative is labelling WebP "lossless" and removing its quality setting. |
| **DEC-7** | Filmic (B9) and tone-curve domain (M-18) fixes change how **existing** edits look | Filmic: **accept the change** (pre-alpha), and bump the sidecar version so old files are identifiable. Curve: **version-gate** — old sidecars keep linear-domain semantics through a `curve_domain` field; new edits use the display domain. |
| **DEC-8** | Lens data (F-04) | **Bundle the lensfun XML database as data only** (CC-BY-SA 3.0, needs attribution), parsed in Rust. Do **not** link the LGPL lensfun C library. |
| **DEC-9** | Platforms at alpha | **Linux + Android are required.** Windows and macOS ship labelled "experimental" once their CI jobs run the tests (WP5.7). |

---

## 3. DESIGN.md amendments this plan requires

The plan deliberately bends four invariants. Each bend is written down so that it is a
decision, not drift (invariant 8):

1. **Invariant 2 (one render path).** Change it to *"same shader, same uniforms; the target
   format may differ."* A 16-bit export (F-02) needs an `Rgba16Unorm` colour target. wgpu
   ties a pipeline to its target format, so the adjust shader gets **two pipelines from one
   module**. The source stays identical.
2. **Invariant 4 (no readback in the interactive path).** Add *"…except one-shot readbacks
   triggered by an explicit click: the WB eyedropper (5×5 texels) and Auto tone/WB (a
   ≤64×64 mip)."* These are never per-frame.
3. **"The whole chain is one shader pass."** Add *"…plus at most one cached pre-pass (luma
   denoise, F-03), re-run only when its own parameters change."* The sensor texture stays
   immutable; the pre-pass writes a derived texture.
4. **Invariant 6 (a drag does not allocate).** Keep the invariant. Fix the code to match it:
   the bind group and LUT allocations are removed (L-13).

**Also update:**
- **§Colour:** describe what the code does; after W-09 lands, describe the wide-gamut
  working space.
- **§Decoding:** rawler only (WP2.6).
- **"32 bytes" per slider drag:** give the real uniform size.
- **Semantic colours:** add the clipping overlay (DEC-4).
- **Tour:** make it opt-in (DEC-4).

---

## 4. U-01 — Sliders reset on navigation (full spec)

### Today
- `open_path` (`crates/photo-app/src/lib.rs:1277-1284`) only starts the decode. Its doc
  comment says *"the current photo stays on screen and editable until the new one is
  ready."* In fact the stage switches to the incoming photo's embedded preview
  (`lib.rs:3202-3206`), while `self.stack`, `self.loaded_path` and the inspector still
  belong to the photo being left. That mismatch is B3.
- The swap to the new stack happens only in `finish_open` (`lib.rs:1395-1405`), when the
  decode lands.

### New behaviour
The **edit target moves the instant you navigate**. The engine image follows when the decode
lands.

1. **Navigation** means any of these: arrow keys, a swipe, a filmstrip or shelf click,
   Open…, drag-and-drop, or Android "Open with". All of them go through one
   `Session::navigate_to(path)`.
2. **Leaving photo N**, in the same frame, in this order:
   1. End any active slider, curve or handle drag. Commit it as the undo step it already is.
   2. If a crop session is open, **cancel** it: an unapplied crop is never saved (M-05).
   3. `flush()`: write N's sidecar if `stack != saved_stack` (WP1.1). A failure is shown in
      the status bar, with the filename.
   4. Park N's `History` and snapshots in the per-photo map (WP2.4). Until WP2.4 lands they
      are dropped, as today.
3. **Arriving at photo N+1**, in the same frame:
   1. `edit_path = N+1`. This is new state, split from `engine_path`, which is the photo
      whose pixels are on the GPU.
   2. `self.stack` resets to N+1's starting point:
      - its sidecar stack, if its `ShelfItem` has one (the sidecar is parsed when the photo
        is shelved; see WP5.3 for moving that off the UI thread); **otherwise**
      - `PhotoStack::default()`, with `geometry` taken from the orientation the preview
        scan reports (the loader's `Done::Preview` already parses it). If that is not known
        yet, identity, fixed up in step 5.
   3. `saved_stack` and `metadata` come from the same sidecar read. `baseline` is the
      default-plus-orientation.
   4. The history is N+1's (empty, or restored by WP2.4). The view resets to fit. Split view
      keeps its on/off setting.
   5. **Every slider, toggle, curve and crop control now shows N+1's values**, from the
      first frame. The inspector never again shows N's numbers while N+1 is on the stage.
4. **While N+1 decodes (`engine_path != edit_path`):**
   - Tone and colour controls **stay editable**. Edits apply to N+1's stack, and are
     checkpointed and autosaved like any other edit. The stage keeps showing the stand-in
     preview, with a small "Loading…" tag. The edits appear the moment the decode lands.
     Decode normally takes 80–300 ms, so this is rarely noticed.
   - **Geometry controls are disabled** (rotate, flip, straighten, crop). The true
     orientation and dimensions are only known once the file is decoded.
   - Hotkeys follow the same rule. Export, copy/paste and batch actions are disabled until
     the decode lands.
5. **When the decode lands** (`finish_open`): the engine loads it and `engine_path =
   edit_path`. If N+1 had no sidecar and its orientation was not known in step 3.2, set
   `stack.geometry = orientation` without a checkpoint. It is the baseline, not an edit.
   Then render with the current stack, including any edits made during the decode.
6. **Rapid navigation** (N → N+1 → N+2 before any decode lands): each step runs 2–3 against
   the current `edit_path`. The existing generation counter drops stale decodes, so only
   N+2's decode is used. N+1's sidecar is written only if it was actually edited while it
   was the target.
7. **Decode failure:** `edit_path` stays N+1 with its baseline, and the inspector disables.
   The error is shown with the filename, and **nothing is written** to N+1's sidecar.
8. **Autosave, rating and metadata writes** always target `edit_path`, never `engine_path`.

### Tests (headless, `crates/photo-app/tests/headless.rs`)
- **Reset:** open A, set exposure +1, step → B. On the same frame, **before B's decode is
  delivered**, check `stack == B.baseline`, the exposure slider reads 0, and A's sidecar
  contains ev = +1.
- **Sidecar:** B has a sidecar with ev = −0.5. Step → B, and the slider reads −0.5
  immediately.
- **Edit during load:** step → B, set exposure +0.7 before the decode lands, then deliver
  it. B renders with +0.7, and A is untouched.
- **Geometry locked:** during the load, rotate is a no-op and the button is disabled.
- **Rapid:** A → B → C with nothing delivered. B's sidecar is not written, and only C's
  decode is applied.
- **Failure:** step → a broken file. The inspector is disabled and no sidecar is created.
- **Crop:** open a crop on A, step → B. A's sidecar has no crop change.

### Pairs with
**W-03 "Same as previous"** (Cmd/Ctrl+Alt+V, and a chip on the phone) applies the previous
photo's stack, minus geometry. Now that sliders reset on every navigation, this becomes the
one-click way to carry a look along a roll.

---

## 5. Phases and work packages

Each WP lists its **items**, its **implementation** notes (files verified at `0ab2f42`), and
its **done when** criteria. "Test" means an automated test lands in the same PR.

### Phase 0 — Stop the bleeding

**WP0.1 Release visibility** — B4, L-28 (stray tag)
- **Implementation:**
  - `build.yml:272`: `[ "$PUBLIC_NIGHTLY" = "true" ] || visibility=--draft`.
  - `build.yml:301`: add `--draft`.
  - Validate `RELEASE_TAG` against `^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$`.
  - Unpublish the current `nightly`, and delete the draft tagged `"v0.1.0 "`.
  - Make `android/README.md` match the new logic.
- **Done when:** a push to main produces a *draft* nightly, and a bad tag fails fast.

**WP0.2 Crash containment** — B6 (first part)
- **Implementation:**
  - `loader.rs`: wrap each open and thumbnail job in
    `std::panic::catch_unwind(AssertUnwindSafe(..))` and map a panic to `Err("<file>: decoder
    crashed (<msg>)")`.
  - Add a `GateGuard` drop-guard so `set_opening(false)` always runs.
  - `batch/queue.rs`: the same per job; a panic counts as a failed job.
  - Shell (`crates/duckfoot/src/lib.rs`, device creation): add
    `device.on_uncaptured_error(..)`, which logs the error and forwards it to the app as a
    status error instead of panicking.
  - Engine: `push_error_scope(OutOfMemory)` / `(Validation)` around sensor, mip, output and
    readback allocations; `pop` turns a failure into `Err`.
- **Test:** a `RawDecoder` test double that panics. The next open still succeeds, and
  thumbnails still arrive.
- **Done when:** a panicking decode cannot wedge the app.

**WP0.3 Refuse X-Trans** — B10 (first part)
- **Implementation:** `decode.rs:277-294` and `487-503`: `bail!("{make} {model}: X-Trans
  sensor — not supported yet (@stub)")` unless the CFA is 2×2. Update the README stub table.
- **Test:** a synthetic 6×6 CFA is rejected with that message.

**WP0.4 Export and status hotfixes** — H1, H3, M-14
- **Implementation:**
  - `export.rs`: JPEG via `JpegEncoder::new_with_quality(.., 92)`.
  - `.tif` / `.tiff` write real TIFF; an unknown extension is an error, not a silent PNG.
  - Update the test at `export.rs:173`.
  - `lib.rs:629-650`: consume `Finished` once (`queue.take_finished()`), after which the
    queue returns to `Idle`.
  - `Export::Done` / `Failed` return to `Idle` after the notice is shown (phone status line).
- **Tests:** a JPEG q check (file-size bound on a noise image); `Finished` is reported
  exactly once.

**WP0.5 Hotkeys agree with the docs** — H5 (per DEC-3)
- **Implementation:** keep plain `0`–`5` as ratings. Change the tooltips at
  `lib.rs:~2628/2634` to "Cmd+1" / "Cmd+0". Fix README:162 and add the missing keys (P, X,
  U, 0–5, Z, Ctrl+A, Ctrl+Shift+E, Enter). WP4.8 later generates this table from code.

### Phase 1 — Never lose work

**WP1.1 `flush()` and autosave** — B1, M-01
- **Implementation:**
  - New `session.rs` (it becomes part of the `lib.rs` split in WP2.5). `PhotoApp::flush()`
    writes the sidecar if `stack != saved_stack`, **not** `!= baseline`. That fixes M-01:
    a reset photo now saves as reset.
  - Shell (`crates/duckfoot/src/lib.rs`): call `flush()` on
    - `CloseRequested` (`:476`), before `exit`;
    - `suspended` (`:454`);
    - `WindowEvent::Destroyed`;
    - `ApplicationHandler::memory_warning`;
    - `Focused(false)` on Android.
  - **Debounced autosave:** on every undo checkpoint, arm a 1 s timer (via
    `request_repaint_after`). If nothing changes for 1 s, `flush()`.
  - Writes go through `atomic_write` (WP1.6).
  - **Unclean-exit marker:** write `session.lock` on start and remove it on a clean exit.
    If it is present at launch, show "Recovered edits from the last session" (F-01).
- **Tests:**
  - headless: edit, drop `PhotoApp` through `flush()`, check the sidecar;
  - the debounce fires once after a burst of edits;
  - reset-to-default then switch writes the reset stack.

**WP1.2 U-01 navigation reset** — U-01, B3, M-05 (part)
- Implement [§4](#4-u-01--sliders-reset-on-navigation-full-spec) exactly.
- **Implementation:**
  - Split `loaded_path` into `edit_path` and `engine_path`.
  - Add `Session::navigate_to`, used by `step_photo` (`lib.rs:1256`), `open_path`
    (`:1277`), shelf and filmstrip clicks, `DroppedFile`, and the Android open.
  - Move the stack, baseline and history setup out of `finish_open` (`:1395-1405`) into
    `navigate_to`.
  - `finish_open` keeps only the engine load and the orientation fix-up.
  - Gate the geometry controls and hotkeys on `engine_path == edit_path`.
  - Fix the `open_path` doc comment.
- **Tests:** the seven listed in §4.

**WP1.3 Sidecar integrity** — B2, M-15
- **Schema v2** (`photo-engine/src/sidecar.rs`):
  `{ version: 2, stack: Option<PhotoStack>, metadata, snapshots: [] (WP2.4),
  #[serde(flatten)] extra: Map }`.
  - `stack: None` means metadata-only: ratings and flags on an unopened photo **never**
    write a stack, which fixes the orientation clobber.
  - `extra` round-trips unknown fields from newer builds.
  - Reading v1 and legacy formats still works.
- **Never overwrite what you can't parse:** a parse failure is surfaced ("DSC0001.ARW:
  sidecar from a newer Duckfoot or damaged — edits not loaded, file left untouched") and
  marks the path `sidecar_locked`, which skips all writes for it. Remove the
  `unwrap_or(None)` at `lib.rs:1090, 1105`.
- **Sanitise on load:**
  - clamp `quarter_turns % 4`;
  - finite-check every `f32`, and clamp each to its slider range from the module registry
    (WP2.2; until then, a hand-written table);
  - cap curve nodes at 32;
  - clamp `rating` to 0–5.
- **Writes:**
  - Use a unique tmp name (`.<name>.duckfoot.<pid>.<n>.tmp`) and `File::sync_all` before
    the rename.
  - Write the **primary** path only. The `<stem>.duckfoot` fallback is read only when no
    sibling file shares the stem (RAW+JPEG pairs).
  - A write failure becomes an error with the filename; the `let _ =` at
    `lib.rs:1094/1109` goes away.
- **Batch export of a never-opened photo** (`lib.rs:903-909`) uses the orientation from the
  file, not `PhotoStack::default()`.
- **Tests:**
  - rating an unopened portrait photo leaves `quarter_turns` intact;
  - a corrupt sidecar is not overwritten;
  - a v2 file with unknown fields survives an older reader's save;
  - NaN and out-of-range values are clamped.

**WP1.4 Safe paste** — H4
- **Implementation:**
  - Copy excludes geometry by default. The full per-module picker comes with W-03; its
    default comes here.
  - Paste onto the open photo goes through a checkpoint and updates `stack`, not only
    `saved_stack` (`lib.rs:1223-1225`).
  - **Paste to all N:**
    - Always goes through the same geometry-excluding filter.
    - Before writing, keep each target's prior sidecar bytes in memory.
    - The status notice gets an **Undo** action that restores them all, and Ctrl+Z works
      while the notice is up.
    - Add a `.duckfoot.bak` of each overwritten sidecar (one generation), so the paste is
      recoverable after a restart too.
- **Tests:** paste-to-all is reversible; it does not touch orientation; the open photo shows
  the paste.

**WP1.5 Crop-mode correctness** — M-05
- **Implementation:**
  - In crop mode, Ctrl+Z cancels the crop session and restores `crop_before`, rather than
    undoing the previous committed step.
  - Navigation cancels the crop (§4 step 2.2).
  - Split rendering is suppressed while cropping; `split_x` is not passed while cropping
    (`lib.rs:1454`).

**WP1.6 Atomic writes everywhere** — M-08, L-03, D-05 (part)
- **Implementation:** `photo_engine::fsio::atomic_write(path, |w| ..)`, which writes a tmp
  file in the same directory, fsyncs it, then renames it. Use it for sidecars, settings
  (`settings.rs:189`), single export, batch export and presets.
- **Test:** a writer that errors halfway leaves the original file intact.

**WP1.7 Phone Save / Revert** — M-35 (part)
- **Implementation:**
  - A Save / Revert row in the phone Edit sheet's footer. Save is less needed once autosave
    exists, but Revert (back to the saved sidecar) is not.
  - A **Reset all** that is a checkpointed command.

### Phase 2 — Foundation

**WP2.1 Real-GPU test harness** — test gap
- **CI:** install `mesa-vulkan-drivers` (lavapipe) on `ubuntu-latest` and set
  `WGPU_ADAPTER_NAME=llvmpipe`.
- **New `crates/photo-engine/tests/gpu.rs`:** needs a real adapter; *fails* in CI if none
  is found and skips locally with a notice. Inputs are synthesised, reusing the logic from
  `examples/synth_raw.rs` (move it into a `testkit` module): a small Bayer DNG, a neutral
  stripe target and a flat field. Assertions:
  - export equals preview byte-for-byte at lod 0, and within 1 LSB mean / 3 LSB p99 at lod 2
    (H9's regression test);
  - clipped highlights stay neutral, and neutral stripes gain no chroma;
  - a flat field gives exactly one histogram bin;
  - rotated export dimensions are right (B8);
  - banded export, forced with a tiny `max_buffer_size`, equals single-shot export (B7);
  - no NaN anywhere in a saturated-blue render (H8);
  - filmic mid-grey lands at 118 ± 2 (B9).
- **Tooling:** keep `bench` for real-file timing, but switch `bench` and `probe` to
  `DefaultRawDecoder` (they use `RawloaderBackend` today).
- **Done when:** the GPU tests run on every push and were each shown to fail with their fix
  reverted.

**WP2.5 Split `lib.rs`** — D-02 (structure). Mechanical only; land it first in Phase 2.
- **Layout:**
  - `app.rs` (struct, `new`, `update`), `session.rs` (from WP1.1–1.3), `history.rs`,
    `keys.rs`, `modules/`, `curve_editor.rs`, `export.rs`, `icons.rs`;
  - `desktop/{top_bar,shelf,filmstrip,stage,inspector,batch_dialog,status}.rs`;
  - `compact.rs` becomes `phone/`.
- **Done when:** the diff is moves only, tests are green, and `clippy` is clean.

**WP2.2 Module registry** — D-01, M-04
- **`modules/registry.rs`:** one `ModuleSpec { id, title, short_name, enabled(&PhotoStack),
  set_enabled, reset, summary, diff_names, sliders: &[SliderSpec], body }`.
- **`SliderSpec`:** `{ key, label, get/set, range, default, decimals, unit }`. It drives the
  UI, sanitising (WP1.3), preset application, `describe_change` and phone reset.
- **Replaces:**
  - `title` (`lib.rs:79`), `module_enabled` (`:4895`), `module_on` (`:4914`),
    `summarise` (`:4935`) and `describe_change` (`:5119`);
  - `compact::reset_module` and `module_short_name`;
  - the field list in `presets::apply`.
- **Engine side:** `PhotoStack::bitmask` and the 8 hand-written `eq_*` blocks
  (`engine.rs:1324-1371`) become loops over `ColorEq::bands()`.
- **Test:** every `PhotoStack` field is covered by a registry entry. The test serialises a
  stack, mutates each leaf through the registry, and checks that the diff names it. This
  catches the next M-04 at compile/test time.

**WP2.3 `Command` enum** — D-02, L-14, L-18
- **`commands.rs`:** `enum Command { SetParam{..}, ToggleModule, ResetModule, ResetAll,
  Rotate(±1), FlipH, FlipV, Straighten, SetCrop, Paste{filter}, ApplyPreset, SetRating,
  SetFlag, Undo, Redo, … }`. `App::apply(cmd)` is the **only** mutation path. It
  checkpoints, marks the stack dirty, arms autosave, and is a no-op without an image.
  Moving every mutation onto it enforces invariant 3 by construction.
- **Replaces:**
  - the rotate/flip implementations at `lib.rs:1679-1706`, `:4127-4174` and
    `compact.rs:1109`;
  - the three undo/redo buttons;
  - the rating/flag menus, duplicated at `:2377` and `:3010`;
  - the `set_rating_*` / `set_flag_*` pairs.
- **Also fixes:**
  - L-18: the slider number box checkpoints once on focus gain, not per keystroke;
    `curve_editor`'s silent re-sort (`lib.rs:5803-5808`) becomes a command.
  - L-14: commands are ignored while a modal (the batch dialog) is open, and a Tab-focused
    *button* no longer swallows hotkeys; only text-edit focus does.

**WP2.4 Per-photo history and snapshots** — M-02, M-03
- **Implementation:**
  - `HashMap<PathBuf, History>`, LRU-capped at 20 photos. Navigating back restores undo.
  - Snapshots move into sidecar v2's `snapshots: Vec<{name, stack}>` and are per photo.
    The global list is removed.
- **Tests:**
  - A → B → A, then undo, restores A's last edit;
  - a snapshot taken on A is not offered on B.

**WP2.6 Rawler only; decoder dedup** — D-04, L-07, L-08, H-surface
- **Implementation:**
  - Remove `RawloaderBackend` and the `rawloader` dependency. `DefaultRawDecoder` is rawler
    (`decode.rs:169`).
  - Fold `decode_reader` / `decode_reader_rawler` into one path.
  - Keep one `orientation_geometry` (they are identical at `:337/:382`) and one DNG-WB
    fallback.
  - A failed decode runs once, not three times (L-07). L-08 disappears with rawloader.
- **Risk:** check the Pixel 8 Pro DNG WB issue and the per-ISO saturation fix against rawler
  on the corpus (§8) *before* deleting rawloader.
- **Done when:** the corpus decodes with equal or better results; the binary is smaller; the
  README is updated.

**WP2.7 Shared scaffolding** — D-05, D-06, D-07
- **Implementation:**
  - One `unused_name(dir, stem, ext)` in Rust, called from the renamer and the Android
    export. The Java `unusedName` is deleted, and JNI asks Rust.
  - A WGSL prelude (`common.wgsl`, holding `vs()`, `Demosaic`, `sample_at`, `luma`),
    prepended with `concat!(include_str!(..))`.
  - `view3d.rs`: floor, axes and projection shared by `inspector_map_3d.rs` and
    `tonemap_3d.rs`.

**WP2.8 One encoder** — D-03
- **Implementation:** `photo_engine::encode::{ExportSettings, encode(&ExportedImage,
  &ExportSettings, path)}` replaces both `ExportedImage::write` and
  `batch::encode_and_save`. Single export becomes a one-job use of the same settings.
  `ExportSettings` grows into F-02's option set.
- **Test:** the same settings produce identical bytes from the single and batch paths.

### Phase 3 — Correct pixels. Each WP lands with its GPU test from WP2.1.

**WP3.1 Filmic transfer** — B9 (DEC-7)
- **Implementation:**
  - `adjust.wgsl:101-111`: output linear. The sigmoid's output is display-referred, so apply
    `srgb_to_linear` to it before the chain continues, so that the single
    `linear_to_srgb` at `:516` stays the only encode.
  - Re-derive the constants so mid-grey (0.18) maps to 0.18 linear at the default
    contrast, `black_ev` maps to 0, and `white_ev` to 1.
  - Move filmic from per-channel to **ratio-preserving**: apply the curve to max(RGB) or a
    norm and scale RGB by the ratio. This removes the hue shifts; keep per-channel as a
    "Look: Classic" option.
  - Bump the sidecar version; the README troubleshooting note about `latitude` goes.
- **Test:** mid-grey is 118 ± 2; black is ≤ 2; a saturated red keeps its hue within 3°.

**WP3.2 Export uses the stack's geometry** — B8
- **Implementation:**
  - `engine.rs:978-998`: `let (w, h) = stack.geometry.output_size(sensor_w, sensor_h)`,
    used for the target, the uniforms and `crop.to_pixels`.
  - Drop the redundant `crop` parameter from `Engine::export` (it duplicates
    `stack.geometry.crop`) and update the callers (`lib.rs:549`, `queue.rs:249`).
- **Test:** batch-export a RAW with `quarter_turns = 1`; the dimensions are swapped and
  there are no black bars.

**WP3.3 Banded, async, lean export** — B7, M-28, perf
- **Banding:** render the full-resolution frame once, then copy and read back in row bands
  of `≤ min(max_buffer_size, 64 MiB)` through one reused `MAP_READ` buffer. Stream each
  band into the encoder, or into one `Vec` for formats that need the whole image. That
  removes the 256 MiB crash.
- **Device limits:** request `adapter.limits()` clamped to sane maxima instead of
  `downlevel_defaults()` (`crates/duckfoot/src/lib.rs:79`). Keep `downlevel_defaults` as
  the fallback.
- **Async:** `Engine::export` becomes `begin_export → ExportJob` with `poll(&device) ->
  Progress | Done(ExportedImage)`. It maps asynchronously, and the UI polls in `update`, so
  `poll(Wait)` goes and the UI thread no longer blocks (M-28).
- **Separate uniforms:** export gets its own uniform buffer, so the preview is not
  overwritten and `needs_render` isn't forced.
- **Memory:** remove `self.rgba.clone()` (`export.rs:49`) and use
  `into_rgb8`/`into_raw`. Drop alpha while unpadding when the target has none.
- **Tests:** a banded export equals a single-shot export; a 100 MP synthetic export
  succeeds under a 256 MiB limit (lavapipe); peak RSS is below 6 B/px (measured in `bench`).

**WP3.4 Shader numerics** — H8
- **Implementation:**
  - `adjust.wgsl:221-227`: `max(luma(..), 0.0) + floor_y` in all three terms.
  - Audit every `log2`, `pow`, `sqrt` and division in `adjust.wgsl` for a negative or zero
    domain.
  - Debug builds and tests get a `FLAG_NAN_PAINT` uniform that paints NaN pixels magenta,
    so GPU tests can assert on it.
- **Test:** a saturated blue and violet chart with clarity, texture and dehaze on has no NaN.

**WP3.5 Resolution-independent kernels** — H9
- **Implementation:**
  - Express the sharpen radius and step (`adjust.wgsl:416`) in **full-resolution pixels**,
    converted to preview texels via `lod`.
  - Texture and clarity use **absolute** levels (`fine_level = 2` in full-res terms, not
    `max(2, lod+1)` at `:218`), matching what dehaze already does.
  - Where a band is finer than the preview can show, attenuate it by the same factor the
    export's downscale would, so fit-view matches a resized export.
- **Test:** preview at lod 2 matches a 4× downscaled export within tolerance (WP2.1).

**WP3.6 Smaller pixel fixes** — M-17, L-05, L-06
- **Vignette:** pass the crop rect in uniforms and centre on it (`adjust.wgsl:509`).
- **Split and histogram:** the split's "before" half skips sharpen, and the histogram
  measures the "after" frame only (render the scope from the unsplit uniforms).
- **Colour EQ:** normalise the band weights to a partition of unity
  (`adjust.wgsl:356-371`).
- **Tests:** each gets a pixel test.

**WP3.7 Curve domain** — M-18 (DEC-7)
- **Implementation:**
  - Add a `curve_domain: Linear | Display` field to the curve module; old sidecars get
    `Linear` and new ones default to `Display`.
  - In `Display`, the LUT applies to sRGB-encoded values: encode, look up, decode. That
    aligns it with the histogram drawn under the editor.

**WP3.8 Demosaic, mips, thumbnails** — L-09, L-10, L-11
- **Demosaic:** reflect by 2 at the edges to keep the CFA phase, and add optional G1/G2
  equilibration.
- **Mips:** use floor sizing to match the wgpu mips (`engine.rs:151`) and fix the
  `downsample.wgsl:24` comment.
- **Thumbnails:** get CR3/RAF thumbnail orientation from rawler metadata instead of the
  TIFF walker.

**WP3.9 Histogram freshness** — H7
- **Implementation:**
  - Tag each measurement with a render counter.
  - When the collected result is older than the last render and no readback is pending,
    set `hist_dirty`, and dispatch a histogram-only pass on the next frame.
  - While a readback is pending, call `request_repaint_after(16 ms)` so it is collected
    without input.
- **Test:** the last frame of a drag is always measured.

### Phase 4 — Interaction, batch, design

**WP4.1 Batch export correctness** — H2, H6, M-07, M-38, L-04 (DEC-6)
- **Renamer:**
  - `{date}` and `{camera}` come from `photo_engine::metadata` (the minimal EXIF reader
    added here, extended in F-10). The hard-coded `"20260924"` at `renamer.rs:273` goes.
  - `{seq}` follows **shelf/view order**, not `HashSet` order (`lib.rs:872`).
  - The job plan tracks names already claimed, in memory as well as on disk.
  - Outputs use `create_new` unless the policy is Overwrite, and go through `atomic_write`.
  - Sanitise every token (`/ \ : * ? " < > |`, `..`, control characters, and Windows
    reserved names), refuse absolute or `..` patterns, and cap `{seq:N}` at 9 digits.
- **Honest recipes:**
  - Remove `ColorProfile::AdobeRgb` / `P3` from the UI until W-09 implements them.
  - Lossy WebP via the `webp` crate (DEC-6).
  - The TIFF recipe becomes 16-bit once F-02 lands.
- **Progress and cancel:**
  - The worker sends a progress message and the UI calls `request_repaint` while a batch
    is running.
  - Cancel sets an `AtomicBool` that is checked between bands, so a job stops mid-image.
    The UI shows "Cancelling…" until the worker acknowledges. A partial file is removed.
- **Recipe editor:** edit, duplicate and delete recipes (format, quality, size, subfolder,
  rename). Recipes are persisted (WP4.9).
- **Tests:**
  - two sources resolving to one name produce `_1` / `_2`, not an overwrite;
  - `{date}` comes from the file;
  - cancel removes the partial file.

**WP4.2 Navigation and selection** — M-06, M-12, M-13, L-19, L-25
- **Stepping:** step through the **filtered and sorted view**. Opening a photo no longer
  calls `select_single` when there is a multi-selection (`lib.rs:1393`).
- **Multiple drops:** multi-file `DroppedFile` is collected for a frame and sent to
  `add_to_shelf`, opening the first one (`crates/duckfoot/src/lib.rs:480`).
- **Pinch at fit:** swipe detection ignores any gesture during which `multi_touch()` was
  seen.
- **Touch state:** `touch_seen` resets on mouse movement.
- **Back chain:** Back closes, in order, the filters panel, then leaves zoom, then leaves
  split, then the rest of the existing chain.
- **Selection:** Shift-click selects a range; `toggle_select` keeps a deterministic anchor.

**WP4.3 Drags and touch** — M-09, M-10, M-11
- **Esc cancel:** every drag (slider, curve node, 3D handle, crop handle, split divider)
  stores the value at drag start, and **Esc** restores it and drops the checkpoint.
- **Touch sliders:** on touch, drags are **relative**, and a tap without movement does
  nothing. Double-tap resets (`widgets.rs:330-359`).
- **Shelf row:** the size badge stops being a Remove button. Remove moves into the context
  menu and a swipe-to-reveal on phone, and it is undoable: the removed item is restored on
  Ctrl+Z or from the notice's Undo. Removal also clears the selection and frees the
  thumbnail.

**WP4.4 One notice system** — L-15 (and the H3 / M-14 fixes)
- **Implementation:** `notices.rs`, a queue of `{text, kind, expires_at, action: Option<Undo>}`.
  The renderer calls `request_repaint_after(expiry)`. It is used by desktop and phone alike,
  and replaces the scattered `status` / `error` / `Export::*` string logic.

**WP4.5 Design conformance and honest scopes** — M-32, M-33, L-27 (DEC-4)
- **Look and feel:**
  - Remove the look-tile gradients (`compact.rs:1554`), the tonemap glow ring and the lime
    accent.
  - The HUD pills auto-hide after 1.5 s of no pointer.
  - Clipping warnings use the F-06 colours, not destructive red.
- **Scopes:**
  - Replace the "RGB gamut cube" (`inspector_map_3d.rs:716-747`) with a real 3D RGB scatter
    from a 32³ binned readback of the preview mip. That readback is part of the existing
    histogram compute pass (+128 KB, async, same invariant-4 budget class). Or remove the
    cube.
  - Drop the made-up tonemap "Spectrum" axis, or label it "illustrative".
- **Tokens:** the 30 raw `Color32::from_gray` values become theme tokens. Pick one spelling
  ("colour") in UI copy.
- **DESIGN.md:** amend it per DEC-4.

**WP4.6 Accessibility floor** — M-34
- **Contrast:** `TEXT_FAINT` goes from #555 to at least #7a7a7a (≥4.5:1 on #000).
  Instructions never use the faint colour.
- **Touch targets:** when `touch_seen` or the layout is phone/tablet, the minimum touch
  target is 44 pt. Landscape tablets get touch metrics in the desktop layout.
- **Font scale:** a setting from 85–150% (egui `zoom_factor` on UI text, while the stage
  stays at 1:1). The minimum label size is 11 pt at 100%.
- **Screen readers:** enable egui's AccessKit (`egui-winit` `accesskit` feature). Every
  painter-drawn widget reports `WidgetInfo` with a label and value. Add a visible focus
  ring for keyboard navigation.

**WP4.7 Layout parity** — M-35 (the rest)
- **Phone:** copy, paste and "Same as previous" (W-03) chips; a Snapshots and History
  sheet.
- **Desktop:** a **Presets** panel in the inspector, the same `presets::LOOKS` with blend
  strength.

**WP4.8 Shortcuts, one source of truth** — M-37
- **Implementation:**
  - `keys.rs` holds a table of `(key, modifiers, Command, description)`. It drives input
    handling, a `?` overlay, and the README Controls table: a test fails if the README
    drifts from the table.
- **Fixes the H5 class permanently.**

**WP4.9 Settings and session persistence** — M-36, F-09
- **Persist:**
  - the shelf (paths, filter, sort), the last open photo, desktop module order and
    visibility, single-open, panel widths, shelf open/closed;
  - export recipes, the rename pattern, the last folders (import and export), and the font
    scale.
- **Mechanism:** a versioned `settings.json` written with `atomic_write`, saved on change
  (debounced) and on `flush()`.
- **On launch:** reopen the shelf. Missing files are shown greyed with "missing", not
  dropped. Reopen the last photo.

### Phase 5 — Robustness, performance, platforms

**WP5.1 Hostile input** — B6 (second part), L-12 — ✅ done: a 250 MP cap checked from the
header before pixels are allocated (rasters by `image`'s dimensions, TIFF-based RAWs by the
largest IFD), the TIFF walker on a 64-IFD / 20,000-entry budget with a `HashSet`, previews
read (an 8 MB head, the rest only when needed, up to 256 MB) instead of mapped, and
seeded mutation tests of the TIFF walker, JPEG scanner, raster decode, DNG colour and
sidecar parsing that run with every `cargo test`. `fuzz/` holds cargo-fuzz targets for the
TIFF walker, the embedded-JPEG scanner, raster decode, RAW decode, sidecars, presets and ICC
profiles, run ten minutes each every week and on demand (`.github/workflows/fuzz.yml`) from seeds the
code writes (`write_fuzz_seeds`). Its first runs found, each now fixed and a regression
test: an offset overflow in the TIFF walker (SubIFD values taken as offsets unchecked); a
TIFF entry counting four billion values, which rawler allocates before reading (13 GB),
now refused when any entry claims more than the file holds, including in an MRW's TTW
block; an IFD chain that points back at itself, which rawler follows until memory runs
out; an MRW block length that wraps rawler's step to zero (a hang); a file starting like
a JPEG whose APP1 rawler cannot read, which loops rawler's JFIF parser for ever (four bytes
did it); and a QuickTake file, reached by content whatever its name, whose decoder sizes
its pixels from the header. rawler now only sees the families the RAW extensions name.
Panics inside rawler stay contained, as in the app, and are for reporting upstream. Still to
come: rawler's own mmap on removable media, and reporting the rawler findings upstream.
- **Size check first:** read the dimensions from rawler metadata before decoding pixels.
  Reject anything over the pixel cap (default 250 MP) or `max_texture_dimension_2d`, *before*
  allocating. The same applies to rasters (`image::ImageReader::into_dimensions` first).
  `check_size` (`engine.rs:298`) stays as a backstop.
- **TIFF walker** (`decode.rs:950-981`): a global budget of IFDs and entries, and the `seen`
  list replaced with a `HashSet`.
- **mmap:** replace it (`preview.rs:201`) with a bounded `read` for files up to 256 MB
  (every RAW). That removes the SIGBUS risk on removable media.
- **Fuzzing:** `cargo fuzz` targets for the TIFF walker, the embedded-JPEG scanner,
  `decode_bytes`, sidecar parsing and preset import. Run them nightly for 10 minutes in CI.

**WP5.2 Memory discipline** — M-23, M-29, M-30 — ✅ done in code: thumbnails are asked for
as their tiles come within 24 of the view (and the open photo's always, for its Info),
kept under an LRU cap of 1,500 on the desktop and 400 on the phone, and evicted on remove;
one mosaic-fallback thumbnail decodes at a time; Android's onTrimMemory releases the
engine's spare textures and all but a quarter of the thumbnails; the batch engine unloads
and drops its spares after every job. The 2,000-photo PSS measurement waits on the device
test.
- **Low memory:** `memory_warning` and Android `onTrimMemory` release the engine's spare
  textures (scratch green, pooled mosaic, `retired_sensor`) and evict thumbnails to 25%.
- **Thumbnails:**
  - request only for tiles in view plus a prefetch margin;
  - an LRU cap (desktop 1,500, phone 400 textures);
  - evict on remove;
  - allow one mosaic-fallback decode at a time (a semaphore in `loader.rs:~293`).
- **Batch engine:** it releases every texture between jobs. On Android, a batch job waits
  until the interactive engine has released its spares.
- **Done when:** a 2,000-photo roll on a 4 GB phone stays below a measured ceiling
  (target: 1.2 GB PSS). This is part of the device test in §8.

**WP5.3 I/O off the UI thread** — M-31 — ✅ done: shelving is a name only, its sidecar and size read on the loader's file thread (a mark set before they arrive reads them first, so it never writes over what the sidecar held); folder opens, drops and restored watches read on that thread; the folder browser's subfolders come from a listing refreshed off-thread every few seconds. Tested by a folder that opens with the shelf still empty and fills in with its marks.
- **Implementation:** `ShelfItem::new`'s sidecar parse and stat, `scan_supported_images`,
  and `FolderTree::list_subfolders` (read every frame at `lib.rs:2243`) all move to the
  loader, with cached results and a refresh on a timer or on navigation.
- **Test:** adding 2,000 files returns within one frame, and items fill in as they are
  parsed.

**WP5.4 Per-frame waste** — L-13 — ✅ done: the History list's labels are cached against a revision every history mutation bumps (only the two beside the live stack are described each frame; each was a whole-stack comparison, brush strokes included); the stack is cloned per frame only while it changes; the histogram's bind group is made once per preview texture, not per frame; the curve editor keeps its plotted samples until a node moves instead of building a 1,024-entry LUT every frame (the engine already uploaded its LUT only on a change); names are lowercased once when shelved and the search query once per frame, not per photo; the 3D histogram's ribbons reuse their rows and hold each quad in an array, not two `Vec`s. Invariant 6 is held by `tests/drag.rs`: every bind group, texture and buffer the engine makes is counted, and sixty frames of a drag (exposure, vibrance, a curve node) must make none and leave wgpu's live counts where they were; it fails against the old per-frame histogram bind group. It counts GPU objects, not heap bytes: wgpu's own command encoding allocates on the heap every frame, which no test of ours could hold at zero.
- **Implementation:**
  - Cache the histogram bind group per output view.
  - Upload the curve LUT only on a node change, without cloning the `Vec`.
  - Cache the History rows until the history changes.
  - Pre-lowercase filenames on add.
  - Build the curve-editor preview LUT only on a change.
  - Keep the 3D ribbon `Vec`s in a reused arena.
  - Replace per-frame `PhotoStack` clones with borrows or `Rc`.
- **Done when:** invariant 6 holds exactly, checked by a counting allocator in a bench test
  over 60 slider frames.

**WP5.5 Android hardening** — H11, H12, M-20, M-21, M-22, M-24, L-17, L-23 — 🟡 in part: a copy into the library needs its size plus 500 MB free; once the app stops (edits flushed) the process ends, so the next launch is cold (H12); a failed JNI call's Java exception is described and cleared; export names are claimed with `create_new` (L-23); backups leave the library of copies out and keep the settings (M-22); `versionCode` is `(major·10⁴ + minor·10² + patch)·10⁴ + CI run` everywhere (L-17); the Library sheet's **storage** screen lists every copy, largest first, with its size (sidecars and virtual copies included) and whether it is on the shelf, with multi-select delete and **clear unused**, each asking a second tap, and select mode gains **remove + delete copy**; deletes run on the file thread, and the open photo is never deleted under itself (H11). Every JNI call runs in a local frame of its own (M-20): sharing and every export had each leaked a string on the event-loop thread, which is attached for good, so a long session of exports would have filled the 512-entry local table and aborted. The tab bar and the Edit sheet's module strip, both swiped sideways, are excluded from the system Back gesture through `setSystemGestureExclusionRects`, sent only when they move and never on the desktop layout (M-24; the photo keeps the system's edges, since Android allows 200 dp per edge). Still to come, on a device: CheckJNI in a device lane, and checking the exclusions by hand.
- **Library (H11):**
  - Check `StatFs` before copying and refuse when free space is below
    `size + 500 MB`.
  - "Remove from shelf" offers **Remove and delete the copy**, and a Library screen lists
    the copies with their sizes, a multi-select delete, and a "clear unused" action.
  - Later: open through persistable SAF URIs without copying (ANDROID_AUDIT B3).
- **Activity recreation (H12):** on `MainEvent::Destroy`, call `flush()` and then
  `finishAndRemoveTask()` + `Process.killProcess(myPid())`, so the next launch is a clean
  cold start. Store `Bridge.activity` in a replaceable slot. Verify on a device with "Don't
  keep activities" enabled.
- **JNI (M-20):** every call goes through `with_local_frame`, and on `Err`:
  `exception_describe` then `exception_clear`. The leaks at `duckfoot-android/src/lib.rs:113`
  and `309-321` are fixed. Add a CheckJNI debug build to CI's device-test lane.
- **Export:** single export on the phone defaults to JPEG q92 (M-21) through
  `ExportSettings`. `unused_name` uses a `create_new` loop (L-23).
- **Backups (M-22):** `dataExtractionRules` / `fullBackupContent` exclude `files/library`
  and include the settings and sidecars.
- **Gesture edges (M-24):** set `setSystemGestureExclusionRects` for the swipe strip, or move
  the swipe target inward; verify on a device.
- **Versioning (L-17):** `versionCode` comes from a monotonic scheme
  (`major*1e6 + minor*1e4 + patch*100 + build`) that is the same locally and in CI.

**WP5.6 Desktop shell hardening** — M-25, L-01, L-02, L-20, L-21, L-22, L-26 — ✅ done: no GPU shows a native message and writes `duckfoot.log` beside the settings; `args_os`, every argument shelved; the desktop entry lists DNG, ORF, PEF, SRW, WebP and more; dialogs on a thread (macOS keeps its main-thread dialogs) with the app polling while one is open; bookmarks from USERPROFILE on Windows; the layout measured before the text-size zoom; a surface-format change replaces egui's renderer and context but keeps the app, which lets go of what it had registered (the photo's texture, thumbnails, the stage preview) and registers it again, so the photo, its edits and the roll stay (L-21); a Suboptimal frame is presented and the surface configured again after it.
- **No adapter:** show a native message box via `rfd::MessageDialog` ("No Vulkan/Metal/DX12
  GPU found — see README Troubleshooting"), and write a log file to the state directory
  (`dirs::state_dir()` / `%LOCALAPPDATA%`).
- **Arguments:** `args_os` instead of `args`, and every argument goes to the shelf (`%F`).
- **Desktop entry:** `MimeType` gains DNG, CR2, NEF, RAF, ORF, RW2 and PEF, and the icons
  go into the correct hicolor sizes.
- **Surface:** reconfigure on `Suboptimal`. A surface-format change rebuilds the renderer
  without dropping `PhotoApp`.
- **File dialogs:** `rfd` dialogs run on a thread (`AsyncFileDialog`) with an event-loop
  proxy wake-up, so the window keeps repainting.
- **Windows:** bookmarks come from `dirs::home_dir()` etc., not `HOME`, which also fixes the
  Windows test.
- **UI zoom:** egui's own Ctrl+Plus zoom can't flip the window into the phone layout. The
  layout decision uses physical window size, not zoomed points.

**WP5.7 Windows and macOS for real** — M-26 (part), DEC-9
- **CI:** `windows-latest` and `macos-latest` jobs run `cargo test --workspace --locked`
  (the no-op GPU tests; lavapipe is Linux-only) and build a release. macOS produces an
  unsigned `.app` in a `.dmg`; Windows a zip.
- **Manual smoke test:** open, edit, export, then close with the edits kept.
- **README:** "experimental" labels until the smoke tests pass.

**WP5.8 Recursive import** — L-24 — ✅ done: folders opened or dropped are read four levels deep on the file thread, hidden and system folders skipped, at most 256 folders; each folder that held photos is watched; while a folder opened or dropped is read, the status line counts the photos and folders found so far with a **cancel** button, which stops the read at the next folder and shelves nothing (restored watches are not cancelled).
- **Implementation:** a folder import on Android and desktop recurses with a depth limit
  (default 4), skipping hidden and system folders. It is cancellable and shows progress,
  and runs on the loader thread (WP5.3).

### Phase 6 — Release engineering and licensing (parallel)

**WP6.1 Licence compliance** — B5 (DEC-1)
- **Notices:** `cargo about generate` produces `THIRD_PARTY_LICENSES.html` in every
  artifact (APK assets, tarball, AppImage, zip, `.app`), plus an **About → Licences**
  screen.
- **LGPL:** include the LGPL-2.1 text and state where to get the source. Every release
  attaches `duckfoot-<ver>-source.tar.gz` built with `cargo vendor`.
- **Fonts and data:** the OFL fonts are already covered. The lensfun data attribution is
  added when F-04 lands.
- **README:** replace the "Unresolved, blocks distribution" note with the resolution.
- **Done when:** a `cargo deny check licenses` allowlist passes, and every artifact contains
  the notices.

**WP6.2 CI hardening** — H10, M-26
- **Permissions:** top-level `permissions: contents: read`, with `contents: write` only on
  the release job.
- **Pinning:** third-party actions pinned by commit SHA; Dependabot for `github-actions`
  and `cargo`.
- **Signing:**
  - The cargo and Gradle build runs **without secrets** and produces an unsigned APK.
  - A separate `sign` job in a protected `environment: release` runs only on `main` and on
    tags. It decodes the keystore into `$RUNNER_TEMP`, runs
    `apksigner sign --ks … --ks-pass env:…`, and deletes the keystore.
  - Feature-branch builds are unsigned or debug-signed.
- **Pinned tools:** appimagetool at a fixed release, with its `sha256` checked.
- **Checks:**
  - `--locked` everywhere;
  - a `cargo deny` job (advisories, licences, bans, sources);
  - `gradle/actions/wrapper-validation`.
- **Deduplicate:** `ci.yml` runs checks and `build.yml` runs builds, each on its own
  triggers, so nothing is built twice.

**WP6.3 Versioning and changelog** — L-28
- **Version:** the workspace goes to `0.2.0-alpha.1`. One source of truth: Gradle reads the
  version from `Cargo.toml`.
- **Changelog:** `CHANGELOG.md` in Keep-a-Changelog format, whose section becomes the
  release notes.
- **Reproducibility:** set `SOURCE_DATE_EPOCH` from the commit time.
- **About dialog:** shows the version, the commit and the build date.

**WP6.4 Delete the TypeScript tree** — L-30
- Remove `apps/`, `packages/`, `pnpm-lock.yaml`, `package.json`, `pnpm-workspace.yaml`,
  `tsconfig.base.json`, the Prettier/Biome settings in `.vscode/settings.json`, and the
  `src-tauri` `.gitignore` entries.

**WP6.5 Docs match reality** — M-27, L-29
- **README "Current state":** fix the contradictions (the fit-preview row, the platform
  rows, the test count, rawler), and update it at the end of each phase.
- **DESIGN.md:** the §3 amendments.
- **Code comments:** the uniform size (`engine.rs:66`) and the curve comment.

### Phase 7 — Alpha feature floor

**F-01 Autosave and crash recovery.** Delivered by WP1.1: debounced autosave, flush on every
lifecycle exit, and the unclean-exit notice. **Done when:** `kill -9` during an edit loses at
most 1 s of work; Android LMK kills lose nothing that settled.

**F-02 Export options, metadata, 16-bit**
- **`ExportSettings`:**
  - **Format:** JPEG, PNG-8, PNG-16, TIFF-8, TIFF-16, WebP.
  - **Quality.**
  - **Resize:** long edge, short edge, megapixels, percent; Lanczos3, done after banded
    readback, or on the GPU via a downsample pass for speed.
  - **Output sharpening:** Off, Screen, Matte or Glossy, at Low, Std or High.
  - **Colour space:** sRGB (P3 and Adobe RGB arrive with W-09).
  - **Metadata:** All, Copyright only, or None, plus **Strip location**.
  - **Filename template:** the WP4.1 renamer.
- **16-bit:** a second adjust pipeline targeting `Rgba16Unorm` (DESIGN §3.1). Export at
  16-bit reads back 8 B/px through the banded path. The 8-bit preview and 8-bit export get
  an ordered or blue-noise **dither** in the shader, the same in both, so banding in skies
  disappears.
- **Metadata out:** new `photo_engine::exif_out`, a minimal TIFF/EXIF IFD writer:
  - `DateTimeOriginal`, `Make`, `Model`, `LensModel`, `ExposureTime`, `FNumber`, ISO,
    `FocalLength`;
  - `Orientation = 1`, because pixels are already oriented;
  - `Software`, `Artist`, `Copyright`, and GPS unless stripped.
  - Values come from rawler's `Exif` (verified: it exposes exposure, f-number, ISO, dates,
    focal length, lens spec, artist, copyright).
  - Embedded via the `image` 0.25 encoders' `set_exif_metadata` (verified present for JPEG,
    PNG and WebP). TIFF gets its tags through the `tiff` crate directly.
- **ICC:** embed a v4 sRGB profile via `set_icc_profile` (verified for JPEG, PNG, TIFF and
  WebP), bundled from Elle Stone's CC0 compact ICC profiles.
- **UI:** the desktop export dialog replaces the plain save dialog: settings plus a
  destination. The phone Export sheet uses the same settings. Recipes become saved
  `ExportSettings`.
- **Tests:** round-trip the EXIF fields through the `image` decoder; the ICC is present;
  16-bit TIFF preserves more than 256 levels; dither produces no mean shift.

**F-03 Noise reduction**
- **Status:** done, under GPU tests. Luminance and colour both run in one cached pre-pass (colour moved there from the fused pass, since it reads coarser levels anyway). There is a "match ISO" starting point. On an 8-bit step target the PSNR gain is 7.9 dB, the edge keeps its width, and false colour drops 5×.
- **Module "Denoise":** Luminance, Luminance detail, Chroma, Chroma smoothness. The default
  strength scales with ISO from the metadata (F-10).
- **Chroma:** runs **inline in the fused pass**, as an edge-aware (bilateral) average of
  opponent chroma over the existing `blurred()` mip taps. It needs no pre-pass.
- **Luma:** a **cached pre-pass** (DESIGN §3.3): a guided filter, or a fast non-local
  means on 5×5 patches in a compute shader. It runs over the sensor at the preview level
  when its parameters change, and at full resolution for export. It writes a derived
  texture that the adjust pass samples instead of the sensor when Denoise is on, so the
  sensor texture stays immutable.
- **Tests:** PSNR on a synthetic noisy flat field plus edge target improves by at least
  6 dB; the edge MTF50 drop is at most 10% at defaults; preview matches export (WP2.1).

**F-04 Lens correction** (DEC-8)
- **Status:** done, all under tests.
  - **Constrained crop.**
  - **Manual corrections:** distortion (a pincushion correction zooms to fill), red and blue fringes, vignetting.
  - **DNG opcodes:** `GainMap` (OpcodeList2) and `FixVignetteRadial` are applied to the mosaic at decode. `WarpRectilinear` is folded into the shader's inverse mapping, one polynomial per plane, and the eyedropper's CPU mirror follows it. Its tangential terms are not applied.
  - **Profiles:** the lensfun database (CC BY-SA 3.0, `data/LENSFUN-NOTICE.md`) is distilled by `tools/lensfun_extract.py` into a 280 KB table: 1,000+ cameras and the rectilinear lenses.
    - The lens is matched by the file's LensModel. The numbers must be exact (focal range, aperture) and most of the words must match; the mount and the calibration's sensor size break ties.
    - Distortion (ptlens/poly3/poly5), TCA (linear/poly3) and vignetting ("pa", interpolated in stops) are taken at the shot's focal length and aperture, carried over between sensor sizes in millimetres, and applied by the shader as the DNG warp is.
    - They are on while Lens is on, with a "profile" switch. A file that corrects itself skips them.
- **Module "Lens":**
  - **Profile:** automatic from EXIF `LensModel` against the bundled lensfun XML (parsed
    with `quick-xml`). It gives distortion (ptlens/poly3), TCA and vignetting. A manual
    lens pick is available when there is no match.
  - **DNG opcodes:** apply `WarpRectilinear` and `FixVignetteRadial` from `OpcodeList3`
    when present. Phone DNGs (Pixel, iPhone ProRAW) rely on these.
  - **Manual:** chromatic aberration (red/cyan and blue/yellow radial scale), distortion
    (±), vignetting (±).
- **Implementation:** everything folds into the existing inverse-geometry UV mapping
  (`adjust.wgsl:~132-160`). Distortion remaps the UV; lateral CA samples R and B at
  per-channel radial scales; vignetting is a radial gain applied before WB. It stays one
  pass.
- **Constrain crop:** after straighten or distortion, auto-crop to the largest inscribed
  rectangle of the chosen aspect. The toggle is on by default. This fixes the empty
  corners after straightening.
- **Tests:** a synthetic barrel-distorted grid is straightened to within 0.5 px RMS; a CA
  target is fixed to within 0.3 px; the lensfun parser has tests.

**F-05 WB eyedropper, Auto WB, Auto tone** (DESIGN §3.2)
- **Eyedropper:** click the stage and read back a 5×5 patch of the **sensor** (pre-WB
  camera RGB) at the inverse-mapped coordinate. Multipliers that neutralise it become a
  `Command::SetWb`. Hotkey `W`; a chip on phone.
- **Auto WB:** robust grey-world plus white-patch on a 64×64 mip readback, excluding
  clipped and near-black texels.
- **Auto tone:** from the existing histogram, measured on a neutral render:
  - exposure puts the median at mid-grey;
  - whites and blacks clip at most 0.1% / 0.05%;
  - highlights go negative when the top bin is populated;
  - optionally, filmic white is set from the scene maximum.
- **All** are one checkpointed command (Undo removes the whole auto adjustment), shown as
  **Auto** buttons in the Exposure and WB module headers.
- **Tests:** a grey card at 3200 K is neutralised within ΔE 2; auto tone on synthetic
  under- and over-exposures converges.

**F-06 On-image clipping overlay**
- **Implementation:** a preview-only uniform flag, like the split, so export never sees it.
  Pixels with any channel ≥ 1.0 after the chain show one colour and pixels ≤ 0 another
  (DEC-4 amends the semantic colours). Hotkey `J`; press-and-hold on the clip % badge; a
  toggle chip on phone.
- **Test:** export with the flag set is byte-identical to export without it.

**F-07 Crop handles and aspect lock**
- **Handles:** 8 handles (edges and corners), move by dragging inside, and **rotate by
  dragging outside** (drives straighten).
- **Aspect:** Original, 1:1, 4:5, 5:4, 3:2, 2:3, 16:9, 9:16, Free, Custom W:H. `X` swaps the
  orientation. Shift temporarily locks the aspect.
- **Overlays:** thirds, golden ratio and diagonals, with `O` to cycle.
- **Touch:** 44 pt handles, and pinch inside the crop to resize.
- **Commit:** Esc and Enter semantics as today. One undo step per session.
- **Tests:** the aspect is preserved while dragging corners; the crop is clamped inside the
  constrained region (F-04).

**F-08 X-Trans and raster fidelity**
- **Status:** done, under GPU tests.
  - **X-Trans** demosaics through the whole 6×6 pattern. Green is a directional blend in four directions and red/blue come from colour differences over the 5×5, with the Bayer path's highlight reconstruction. On the grey test scene its false colour is 1.1 levels against Bayer's 0.6.
  - **Rasters:** 16-bit PNG/TIFF keeps 16 bits and float TIFF is read as linear. An embedded matrix/TRC profile (Display P3, Adobe RGB, ProPhoto, grey) is read by a small parser here instead of qcms, and the pixels stay in the file's primaries, converted in the shader as for RAW. A P3 JPEG's patches land within 2.5 levels of the published P3→sRGB matrix.
  - **Left out:** a LUT-based (CMYK/printer) profile is read as sRGB.
- **X-Trans:** a Markesteijn one-pass demosaic in WGSL, with the 6×6 CFA passed as a
  uniform array. It replaces WP0.3's refusal. Highlight reconstruction is reused.
- **Raster (M-16):**
  - decode 16-bit PNG/TIFF to 16-bit, through a new `Rgba16Unorm` raster upload;
  - honour an embedded ICC with **qcms** (pure Rust, MPL-2.0), transforming to linear
    working primaries at load;
  - Display-P3 phone JPEGs and Adobe RGB TIFFs then render correctly.
- **Tests:** an X-Trans synthetic neutral pattern has false colour at most the Bayer
  baseline plus 1; a P3 test JPEG's patches are within ΔE 1 of reference.

**F-09 Session persistence.** Delivered by WP4.9.

**F-10 Metadata panel**
- **Status:** done. The desktop Info band, the phone Library sheet's one-line summary opens the same card, and filmstrip tooltips.
- **`photo_engine::metadata::PhotoInfo`:** camera, lens, shutter, aperture, ISO, focal
  length (and 35 mm equivalent), date and time with offset, dimensions, file size, GPS (as
  text — no map, local-first), copyright and artist. It comes from rawler `Exif` for RAWs
  and from the TIFF walker over the APP1 EXIF for rasters. It is parsed on the loader
  thread and cached on the `ShelfItem`.
- **UI:** an **Info** section in the desktop inspector, a detail card in the phone Library
  sheet, and tooltips on filmstrip tiles.
- **Used by:** the renamer's `{date}` and `{camera}` (WP4.1), the ISO-scaled Denoise
  defaults (F-03), lens matching (F-04) and EXIF out (F-02).

### Phase 8 — Worth considering, post-alpha, in priority order

| ID | Feature | Shape | Est. |
|---|---|---|---|
| **W-03** ✅ | Selective paste / sync; **Same as previous** (done: a remembered module set — crop & rotate off by default — that Paste, Previous and Paste to all follow; a dialog on the desktop via Modules… or Ctrl+Shift+V, inline on the phone's Export sheet) | A per-module checkbox picker driven by the module registry, geometry off by default. Cmd/Ctrl+Alt+V pastes from the previous photo (pairs with U-01). | 2 d |
| **W-04** ✅ | User presets: save, import, export (done: one versioned `.duckfoot-preset` file each in the settings folder, carrying the modules the paste set names; tiles after the built-in looks on both layouts, right-click or hold to export or delete; import on the desktop, and on Android through the system picker, which copies the file into the app cache for the same import) | `.duckfoot-preset` JSON (versioned, sanitised through the registry on import, with an included-modules mask); a Presets panel on both layouts. | 2 d |
| **W-01** ✅ | Local adjustments (DEC-5) — linear and radial done (sensor-space masks that follow turns and flips; exposure, temperature, tint, contrast, saturation, clarity, sharpness; invert, feather, a red wash to see a mask; handles on the stage on both layouts); brush masks done too (dabs in sensor space, rasterised into an R8 layer each that preview and export share, painted and erased on the stage, one undo step a stroke); local noise reduction done too: the denoise pre-pass works in sensor space, as masks do, so it evaluates the masks that ask for it (brushes included) and raises its own strength there to at least the mask's amount; one cached copy still carries global and local, re-made when those masks or their strokes change, and made for local alone when the module is off | ≤8 masks, each **linear gradient**, **radial** or **brush**, with a fixed parameter block: exposure, temperature, tint, contrast, saturation, clarity, sharpness, denoise. Gradient and radial masks are evaluated analytically in the fused shader; brush masks are rasterised into an R8 texture array at preview and full resolution. Mask overlay UI, feather and invert. | 10 d |
| **W-02** ✅ | Spot heal / clone (DEC-5) — done: ≤32 spots in sensor space, heal matched to a ring around the destination via mips, or clone; applied as the sensor is sampled (sharpening sees the healed pixels); a one-shot search picks each source from matching ground; tap to heal, drag a spot or its source; a preview-only dust view | ≤32 circular spots `{dst, src, radius, feather, mode}`, evaluated in the sampling stage: clone samples the source; heal matches the source's mean to the destination's ring via mips. Automatic source search, plus a "visualise dust" high-pass view. | 5 d |
| **W-09** ✅ | Wide-gamut working space (done: every new edit works in linear Rec.2020, the camera matrix composed onto it; a sidecar without the field keeps linear sRGB and its look, and a chip in white balance moves between them (History names it; paste carries it). Luminance weights follow the space; the colour EQ picks bands by the hue sRGB would name and turns and scales chroma about the grey axis with nothing clamped. The output transform converts to the export's space and brings out-of-gamut colour in along the line to its own luminance's grey, so hue holds where a channel clip shifted it; exports can be sRGB, Display P3 or Adobe RGB (1998)-compatible, each with its ICC profile, from the Export dialog and recipes. Tests: matrices against the published ones, a P3 green surviving to a P3 export and not through the sRGB space, the P3 JPEG test on the new mapping, every format carrying its profile. DNGs now use their own profile in the wide space: ForwardMatrix (to XYZ D50, Bradford to D65) where present, else the two colour matrices interpolated for the as-shot white, which an `AsShotNeutral` file's is found by iterating temperature and matrix until they agree; an edit in the older sRGB space keeps its old matrix) | Camera → XYZ → linear Rec.2020 working space (DESIGN §Colour as written), with DNG ForwardMatrix and dual-illuminant interpolation always. A gamut-mapped output transform to sRGB, Display P3 or Adobe RGB, with the matching ICC embedded. It fixes the saturated-colour clamps and hue shifts, and re-enables the Adobe RGB recipe honestly. | 6 d |
| **W-08** ✅ | RCD demosaic; highlight *detail* recovery (RCD done: four passes in `demosaic_rcd.wgsl`, soft vertical/horizontal and diagonal directions, low-pass-ratio green and gradient-weighted colour differences; its two scratch planes live only for the demosaic. On a grey zone plate with a hard edge, false colour 3.49 → 3.13 and PSNR against the undemosaiced scene 35.96 → 36.63 dB, held by a test. Hamilton-Adams is gone; X-Trans keeps its own passes. Highlight detail: a pyramid of trusted colour ratios, one sample a Bayer quad straight from the mosaic and box-averaged down to one texel, gives each pixel whose channels partly clipped the red/green and blue/green of what is unclipped nearby; its clipped channels are rebuilt from the least clipped one, so a blown sunset keeps its colour and texture, and only where every channel clipped, or nothing unclipped is near, does it fall back to the neutral. Two tests: a sky where red clips keeps its ratio and stripes, and a blown light under a strong white balance stays white to its rim. X-Trans keeps the neutral-only reconstruction) | RCD replaces Hamilton-Adams (a drop-in point). Clipped channels are reconstructed from the unclipped ones by ratio propagation over mips. | 5 d |
| **W-07** ✅ | Perspective / keystone (done: vertical and horizontal sliders as an inverse homography before straighten, zoomed to keep every corner on the photo; the CPU mirrors follow it. Auto upright done too: keystone.rs finds the photo's straight edges on a 512 px overview (a Hough transform steered by each edge's gradient and split by its polarity, each peak refitted by least squares to its own edge pixels), and solves straighten and keystones so they stand upright and level, through the same `output_point` mirror, with a small pull to no keystone; level / vertical / full, as Lightroom's Upright, in the crop & rotate card) | Vertical and horizontal sliders (an inverse homography in the geometry map). Later: auto from line detection. | 3 d |
| **W-05** ✅ | Virtual copies (done, as darktable does duplicates rather than one sidecar holding versions: a copy is `IMG_0001 (v2).dng`, a name with no file behind it and a sidecar of its own beside the photo, so each copy has its own edits, marks, undo history and snapshots and exports under its own name; reads go to the photo's file. Shelved right after its photo and its other copies, name sort keeps them together, and reading the folder again finds them from their sidecars. Ctrl+', the tile's right-click menu, or the phone's + copy chip. Removing one from the shelf keeps its sidecar; deleting a copy outright is not offered yet) | Sidecar `versions: Vec<{name, stack}>`, with stacked tiles in the shelf. | 3 d |
| **W-06** ✅ | Persistent undo (done: the sidecar keeps the newest 50 undo steps, capped at 256 KB so brush-heavy stacks can't bloat it; a photo opened with no history in memory gets its history back, so undo reaches past a relaunch. Redo stays per-session) | The last 50 history entries are serialised into the sidecar (after WP2.4). | 1.5 d |
| **W-10** 🟡 | Soft proofing; print sharpening (done for RGB spaces: S or the stage toolbar's proof chip shows the preview as the export's colour space will have it, brought in as the export brings it and then shown on the screen; its gamut chip greys what that space cannot hold. Printer profiles too, CMYK or RGB LUT-based: the toolbar's printer… chip picks one, and `proof.rs` bakes export file → printer → screen through moxcms into a 33³ table the shader samples, its alpha marking colours the printer loses. Still to come: a rendering-intent choice and paper-white simulation) | Proof against an output ICC with qcms (a preview-only uniform, like the split). Output sharpening is already in F-02. | 3 d |
| **W-11** ✅ | Colour labels, keywords, collections-lite, file watching (labels and keywords done: red / yellow / green / blue / purple, `6`–`9` toggling the first four, swatches in the tile menu, the Info band and the phone's Library row, a band along the tile's top, and a label filter; keywords kept in the sidecar, trimmed and once each, added from the Info band or the phone's info card (to the whole selection when several are chosen) and found by the shelf's search. Saved filters: the filter showing, search and sort included, kept under a name as a chip that brings it back (right-click or long-press deletes it), remembered across launches. A folder opened whole is watched, by polling every 2 s off the UI thread rather than `notify`, so network shares and card readers behave the same: a photo that arrives joins the shelf, one taken off the shelf stays off, and a shelved photo whose file goes is marked missing until it is back) | UI for the existing `color_label` field (an L-16 dead field comes alive); keywords in the sidecar and searchable; saved filters as "collections"; `notify`-based rescan. | 4 d |
| **W-12** ✅ | Android share sheet (done: the export's notice and the Export sheet offer share, which hands the file to `ACTION_SEND` through a chooser; exports are in the shared Pictures folder, so the media scanner's own content:// URI is granted instead of a FileProvider, which would have needed androidx. Platforms say whether they can share; the desktop shows nothing) | After export, `ACTION_SEND` via a `FileProvider` URI. | 1 d |
| **W-13** ✅ | Localisation scaffolding (done: `fluent-bundle` with `crates/photo-app/locales/en-US/duckfoot.ftl` embedded; `i18n::tr(id)` returns a `&'static str` so it stands where names already were, `tr_args` fills `{ $values }`; a language is picked from `DUCKFOOT_LANG` or `LANG` and falls back to English message by message. Moved so far: the phone's tabs and tour, every module's card and strip name, the Info rows, and two notices. A test reads the sources and fails on any id the English file lacks. The rest of the strings move as their code is touched) | `fluent` bundles with the strings extracted; English only at first. Doing it early is cheap; late is expensive. | 2 d |
| **W-14** 🟡 | Display colour management (done as a setting: Customize → Screen takes sRGB, Display P3, Adobe RGB or the monitor's own matrix/TRC ICC file, a table curve fitted to its nearest power; the preview's last step encodes for it and exports are untouched. Reading the profile from the OS (colord, Windows, ColorSync) is still to come, and the phone draws in sRGB) | Desktop monitor ICC (colord / Windows / macOS APIs) applied as a final preview-only transform. It closes the README's "display profiles are ignored". | 3 d |

### Phase 9 — Gesture-first traversal and aligned bars (after every feature)

**Owner's direction (27 Sep 2026):**
- Align the top and bottom menu content; every button and control on the top two bars is misaligned.
- Swipes are not exact enough. Swiping left or right anywhere should traverse, even on the image.
- Everything is too small to move through quickly without tapping, especially the tight sliders in big panels.
- Traversal should be simple and gesture-based, like Snapseed, and there must still be a good tutorial.

This phase comes after all features, so it works on the finished set of controls and does not have to be done twice.

**What is wrong today** (screenshots at 1280×800 and 412×892):
- **Desktop top bar:** the controls have four heights (the Photo/Audio/Video switch, the camera box, the layout group, the buttons). The right-hand groups sit on different centre lines. `?` is a bare glyph beside boxed buttons.
- **Desktop stage toolbar:** CROP, A/B and CLIP are bare text sitting a few pixels above the boxed zoom group (− 19% +), FIT and 1:1. The panel headers (SHELF BIN, SCOPE and its HIST/RIBBON/SURFACE chips) each keep their own baseline.
- **Phone top bar:** text items (13%, CLIP, FINE) and icons (reset, compare, undo, redo) have different sizes and uneven gaps, and only the compare icon is boxed.
- **Phone bottom bars:** the module strip marks the chosen module with a line under its label, while the navigation bar marks the chosen tab with a line above its icon. The strip's undo/redo glyphs sit off its text baseline.

| ID | Item | Shape |
|---|---|---|
| **G-01** 🟡 (midlines done and tested on every bar and size; one height per group and even gaps still to come) | One line per bar | Every bar item goes through one `bar_item` widget: one height per bar (32 px desktop, 44 px touch), centred on the bar's midline, with a fixed gap and group padding. Glyphs get a fixed square, so an icon and a word are the same size. Within a group, items are either all boxed or all bare; `?` becomes a boxed icon like its neighbours. The selected state is marked the same way on every bar: one underline at the same distance below. The zoom group's numbers are centred between its ± buttons. **Test:** an *alignment detector* beside the overlap test reads egui's widget rects per bar. It fails if an item's vertical centre is more than 1 px off the bar's midline, if heights in a group differ, or if gaps are uneven. It runs on every desktop and phone size the overlap test covers. |
| **G-02** ✅ (in code; device checklist in G-05) | Swipe anywhere, exactly | One gesture recogniser for the whole app. It locks the direction after 10 px, so a swipe is never misread as a scroll or a slider drag. A flick is decided by velocity as well as distance, so a short fast swipe counts. There is a dead zone at the screen edges for the system back gesture. A **horizontal swipe anywhere** traverses: on the photo, on the sheet, even across a slider row (a drag that starts on a slider's thumb still moves that slider). In the Library and with no sheet open, a swipe moves between photos. In Edit it moves between tools (G-03). While zoomed in, one finger pans and a two-finger swipe traverses. |
| **G-03** ✅ (in code; row-relative sheet sliders and ticks still to come) | Snapseed-style editing on the photo | In Edit, the photo itself is the control. **Swipe up or down** to choose a parameter of the current tool: a large, readable list floats over the photo and follows the finger. **Swipe left or right** to change its value, with a large readout ("Exposure +0.35") over the photo, like a toast. Slow drags are finer and flicks coarser; a light tick marks each step, zero and each end. A **double-tap** puts the parameter back. **Hold** shows the original (the before view without the split). **Pinch** zooms, and two fingers pan. The module strip remains for jumping straight to a tool, and a sideways flick on it moves one tool at a time. The sheet's sliders stay, as big rows: a drag anywhere on a row adjusts it relative to where it was, instead of needing to hit a thin rail. On desktop the same engine runs from a right-button or Alt drag on the photo, and ↑/↓ choose and ←/→ adjust for the keyboard and screen readers. |
| **G-04** ✅ | A tutorial that teaches the gestures | The tour is rewritten as interactive coach marks on the real photo. Each step waits for the user to do the gesture before moving on: swipe up or down to choose, left or right to adjust, double-tap to reset, hold to compare, swipe the strip to change tool, and swipe in the Library to change photo. It can be skipped at any step, replayed from `?` and the phone's menu, and shown again on the next launch after a skip if the user asks. Each step's text stays short enough to sit clear of the controls it points at, and the overlap test covers every step. Headless tests drive each step with synthetic touches and check that it advances. |
| **G-05** | Proof on a phone | Everything above, checked on real Android hardware in both orientations. Swipe accuracy is measured with a checklist of 20 swipes: none misread, none missed. The APK must still build on every commit. |

---

## 6. Cross-cutting rules for every PR

1. **Invariant check in review:** does this PR add a mutation outside `Command`, a readback,
   a per-frame allocation, or a second render path? If so, it needs a DESIGN §3 amendment
   or it is rejected.
2. **Tests land with the fix.** Every bug ID in §11 gets a test that fails before the fix
   and passes after it, the same "fails when the fix is removed" discipline the bench
   already follows.
3. **The docs move in the same PR:** the README status table, Controls (generated) and
   DESIGN.
4. **Sidecar schema changes** bump `version` and add a migration test.
5. **Both layouts:** a feature is done when it exists on desktop **and** phone, or has a
   written reason not to.

## 7. DRY targets (from the audit's §7), and where each lands

| ID | Duplication | WP |
|---|---|---|
| D-01 | Seven parallel per-module tables, plus the engine's bitmask and `eq_*` blocks | WP2.2 |
| D-02 | Mutations not funnelled (rotate/flip ×3, undo/redo ×3, rating/flag ×2); `lib.rs` 6.4k lines | WP2.3, WP2.5 |
| D-03 | Two export encoders | WP2.8 |
| D-04 | Two `decode_reader*`, identical orientation mappers, the DNG-WB fallback twice | WP2.6 |
| D-05 | Three "unused filename" routines; ad-hoc atomic writes | WP2.7, WP1.6 |
| D-06 | Four copies of the fullscreen `vs()`; `Demosaic` / `sample_at` twice | WP2.7 |
| D-07 | 3D scaffolding copied between the two 3D views | WP2.7 |

## 8. Test strategy

| Layer | What runs | Where |
|---|---|---|
| Unit | maths, parsers, renamer, sidecar migrations, registry coverage | every push, all 3 OSes |
| Headless (no-op GPU) | app state machine: U-01 navigation, autosave, paste, batch planning, notices, keys | every push, all 3 OSes |
| **GPU (lavapipe)** | pixel truth: preview≈export at lod 0/2, neutrality, NaN, filmic, banding, rotation, dither, clip overlay excluded from export | every push, Linux (WP2.1) |
| Fuzz | TIFF walker, JPEG scanner, `decode_bytes`, sidecar, preset import | nightly, 10 min each (WP5.1) |
| Real-file corpus | decode plus colour sanity across a CC0 corpus from raw.pixls.us: Sony ARW, Canon CR2/CR3, Nikon NEF, Fuji RAF (X-Trans and Bayer), Olympus ORF, Panasonic RW2, Pentax PEF, Leica/Pixel/iPhone ProRAW DNG, a monochrome DNG, a 100 MP GFX. Cached in CI. | nightly |
| Device matrix (manual, per release) | Android: a Pixel 7/8 (Mali), a Samsung A-series 4 GB (low memory), an Adreno device. Checks: suspend/resume, LMK kill with the edits intact, "Don't keep activities", a 60 MP export, a 2,000-photo roll memory ceiling, gesture edges. | before each alpha tag |
| Perf | `bench` on real files: open time, drag ms/frame, export time and peak RSS, allocations per drag frame (must be 0) | per phase, results in the README |

## 9. Alpha release gate

- [ ] All of B1–B10, H1–H12 and M-xx are closed with tests, or explicitly deferred in
      writing.
- [ ] F-01 … F-10 are shipped on desktop and phone.
- [ ] Every artifact carries licence notices and there is a source tarball on the release
      (WP6.1).
- [ ] CI is green on Linux, Windows and macOS; the GPU tests are green; `cargo deny` is
      green.
- [ ] Device matrix passed. The Android LMK kill test loses nothing.
- [ ] The README "Current state" table is accurate, and DESIGN.md carries the §3
      amendments.
- [ ] `CHANGELOG.md` has a `0.2.0-alpha.1` section; the tag is drafted and then published
      by hand.

## 10. Risks

| Risk | Mitigation |
|---|---|
| rawler regresses a camera that rawloader handled (WP2.6) | Diff the corpus first; if one camera regresses, keep rawloader behind a feature flag for that camera only, with a tracking issue |
| Filmic and curve changes alter users' existing looks | Sidecar version bump plus `curve_domain` gating (DEC-7); a changelog note |
| Luma denoise too slow on mobile at the preview level | Run the pre-pass at one level coarser than the preview while dragging, and refine on release; a quality or speed setting |
| lavapipe differs from real GPUs in float precision | Tolerances in pixel tests; bit-exact checks only within the same device (preview vs export) |
| Scope creep from W-01 / W-02 | Fixed caps (8 masks, 32 spots) written into DESIGN; no layer or blend UI (DEC-5) |
| LGPL interpretation (DEC-1) | Get the source-bundle approach confirmed before the first public tag; the `cdylib` route stays as a fallback |

---

## 11. Traceability matrix

Every finding and feature, and the WP that closes it.

### Blockers and high

| ID | Finding | WP |
|---|---|---|
| B1 | Edits lost on quit, background or crash; no phone Save | WP1.1, WP1.7 |
| B2 | Rating/flag/batch sidecars drop rotation; unparseable sidecar overwritten; write failures swallowed | WP1.3 |
| B3 | Inspector edits the previous photo during a decode | **WP1.2 (U-01)** |
| B4 | Public releases despite the hold | WP0.1 |
| B5 | No licence notices; LGPL | WP6.1 |
| B6 | A decoder panic wedges the loader; no GPU error handler; unbounded allocation | WP0.2, WP5.1 |
| B7 | Export over ~67 MP crashes; CPU peak | WP3.3 |
| B8 | Batch export ignores the stack's geometry | WP3.2 |
| B9 | Filmic double transfer | WP3.1 |
| B10 | X-Trans demosaiced as Bayer | WP0.3, F-08 |
| H1 | JPEG q75; `.tif` gets PNG bytes | WP0.4, WP2.8 |
| H2 | Recipes claim Adobe RGB and lossy WebP | WP4.1, W-09 |
| H3 | Batch "finished" re-fires every frame | WP0.4, WP4.4 |
| H4 | Paste-to-all destructive, carries geometry, doesn't show on the open photo | WP1.4, W-03 |
| H5 | `0`/`1` rate instead of zooming; docs wrong | WP0.5, WP4.8 |
| H6 | Renamer date, camera, seq order, collisions | WP4.1 |
| H7 | Histogram stale after a drag | WP3.9 |
| H8 | Clarity/texture NaN | WP3.4 |
| H9 | Preview ≠ export for sharpen and texture | WP3.5 |
| H10 | CI signing and permissions hygiene | WP6.2 |
| H11 | Android library unbounded | WP5.5 |
| H12 | Second `android_main` gives a black screen | WP5.5 |
| U-01 | **Sliders reset on navigation** | **WP1.2** |

### Medium

| ID | Finding | WP |
|---|---|---|
| M-01 | Reset-to-default not saved on switch | WP1.1 |
| M-02 | Snapshots global and in-memory | WP2.4 |
| M-03 | Undo discarded on switch | WP2.4 |
| M-04 | History "no change" for Clarity, Dehaze and Colour EQ | WP2.2 |
| M-05 | Crop mode: Ctrl+Z, switching mid-crop, split while cropping | WP1.5, WP1.2 |
| M-06 | Stepping ignores filter and sort; opening wipes the multi-select | WP4.2 |
| M-07 | Batch progress freezes; cancel semantics | WP4.1 |
| M-08 | Exports write in place | WP1.6 |
| M-09 | No Esc-cancel on drags | WP4.3 |
| M-10 | Touch tap jumps a slider | WP4.3 |
| M-11 | Size badge is a Remove button; removal leaves selection and thumbnail | WP4.3 |
| M-12 | Multi-file drop keeps one | WP4.2 |
| M-13 | Pinch at fit can change photo | WP4.2 |
| M-14 | Phone status stuck on "exported" | WP0.4, WP4.4 |
| M-15 | Sidecar: no version, no validation, fixed tmp name, no fsync, stem fallback shared | WP1.3 |
| M-16 | Raster input 8-bit only; ICC ignored | F-08 |
| M-17 | Vignette centred on the uncropped frame | WP3.6 |
| M-18 | Tone curve in the linear domain | WP3.7 |
| M-19 | Working space is linear sRGB; DNG profiles underused | W-09 |
| M-20 | JNI ref leaks; exceptions never cleared | WP5.5 |
| M-21 | Phone export is always a full-resolution PNG | WP5.5, F-02 |
| M-22 | `allowBackup` includes the RAW library | WP5.5 |
| M-23 | No memory-pressure handling; spare textures retained | WP5.2 |
| M-24 | Gesture-edge conflicts (unverified) | WP5.5 |
| M-25 | No-adapter silent exit; no log on Windows | WP5.6 |
| M-26 | CI coverage gaps (OSes, `--locked`, deny, wrapper validation, double builds) | WP5.7, WP6.2 |
| M-27 | Docs contradict CI and platform status | WP6.5 |
| M-28 | Export blocks the UI thread; shared uniform buffer | WP3.3 |
| M-29 | Batch builds a second full `Engine` | WP5.2 |
| M-30 | Thumbnails eager, never evicted; mosaic fallback concurrency | WP5.2 |
| M-31 | Synchronous disk I/O on the UI thread | WP5.3 |
| M-32 | Departures from DESIGN's look and feel | WP4.5 (DEC-4) |
| M-33 | Scopes present invented data | WP4.5 |
| M-34 | Accessibility (contrast, targets, tablet metrics, font size, AccessKit) | WP4.6 |
| M-35 | Layout parity (phone Save, copy/paste, snapshots, history; desktop presets) | WP1.7, WP4.7 |
| M-36 | Almost nothing persisted between runs | WP4.9 |
| M-37 | Shortcuts undocumented; no overlay | WP4.8 |
| M-38 | Single export has no options or cancel; recipes not editable | WP4.1, F-02 |

### Low

| ID | Finding | WP |
|---|---|---|
| L-01 | `env::args` panics on non-UTF-8; only the first argument is opened | WP5.6 |
| L-02 | `.desktop` MimeType and icon directory | WP5.6 |
| L-03 | Settings written non-atomically | WP1.6 |
| L-04 | Renamer sanitising, `{seq:N}` allocation, `../` escape | WP4.1 |
| L-05 | Split "before" is sharpened; histogram measures the mixed frame | WP3.6 |
| L-06 | Colour EQ weights don't sum to 1 | WP3.6 |
| L-07 | A failed decode runs three times | WP2.6 |
| L-08 | rawloader DNG black levels by position | WP2.6 |
| L-09 | Demosaic edge clamping; green imbalance | WP3.8 |
| L-10 | Downsample `div_ceil` vs floor; wrong comment | WP3.8 |
| L-11 | CR3/RAF thumbnails not rotated | WP3.8 |
| L-12 | TIFF walker budget; mmap SIGBUS | WP5.1 |
| L-13 | Per-frame allocations | WP5.4 |
| L-14 | Hotkeys with no image, behind dialogs, Tab-focused buttons | WP2.3 |
| L-15 | Desktop notice doesn't repaint to expire | WP4.4 |
| L-16 | Dead code (`extended_range`, `histogram_plot`, unused widgets, `color_label`) | WP2.5 (delete), W-11 (`color_label`) |
| L-17 | Gradle `versionCode` local = 1 | WP5.5 |
| L-18 | Number-box undo per keystroke; curve re-sort without a checkpoint | WP2.3 |
| L-19 | `touch_seen` never resets; Back chain gaps | WP4.2 |
| L-20 | egui zoom can flip to the phone layout (unverified) | WP5.6 |
| L-21 | Surface `Suboptimal` / format change drops `PhotoApp` | WP5.6 |
| L-22 | `rfd` dialogs block the event loop | WP5.6 |
| L-23 | Android `unused_name` race; reinstall visibility (unverified) | WP5.5 |
| L-24 | Folder import only one level deep | WP5.8 |
| L-25 | Selection anchor arbitrary; Shift-click not a range | WP4.2 |
| L-26 | Windows `HOME` bookmarks | WP5.6 |
| L-27 | Raw greys outside theme tokens; mixed spelling | WP4.5 |
| L-28 | Version mismatch, stray tag, no changelog, not reproducible | WP0.1, WP6.3 |
| L-29 | Doc drift (uniform size, curve comment, DESIGN decoder) | WP6.5 |
| L-30 | Dead TypeScript tree | WP6.4 |

### Features

| ID | Feature | WP |
|---|---|---|
| F-01 | Autosave and crash recovery | WP1.1 |
| F-02 | Export options, EXIF/ICC, 16-bit, dither | F-02 |
| F-03 | Noise reduction | F-03 |
| F-04 | Lens correction and constrained crop | F-04 |
| F-05 | WB eyedropper, Auto WB, Auto tone | F-05 |
| F-06 | Clipping overlay | F-06 |
| F-07 | Crop handles and aspect lock | F-07 |
| F-08 | X-Trans and raster fidelity | F-08 |
| F-09 | Session persistence | WP4.9 |
| F-10 | Metadata panel | F-10 |
| W-01 … W-14 | Worth considering | Phase 8 table |

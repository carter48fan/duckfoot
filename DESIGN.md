# Duckfoot — Design Brief

## Mission

Duckfoot fires three purpose-built barrels from one stock. Each barrel does one job
completely and refuses the other two.

The measure of success is not what Duckfoot can do — it's what it declines to do. Every
feature request answered with *"that belongs in another barrel, or nowhere"* is the
design working. Build tools a friend can open, do one thing, and close: no project
wizard, no onboarding, no mode you can get lost in.

## Where this document is aimed right now

Duckfoot is being rebuilt as a **native Rust application on wgpu**. The current work is
a **proof of concept for Barrel 1 only** — the photo studio and RAW processor — running
on Linux. Audio and Video are design, not code, and nothing in this repo should claim
otherwise.

The PoC has one question to answer: *can a fixed-order RAW pipeline on the GPU stay
genuinely interactive on a full-resolution sensor image?* Everything here is subordinate
to answering that.

The three barrels remain the product thesis, and Barrel 1 is built so the other two can
land beside it later without redesign. They are described at the end of this document so
the shape of the whole is not lost, but they are **not** on the near roadmap.

## Why native, and why not a webview

An earlier build put the pipeline behind a Tauri webview and moved pixels across the IPC
boundary. That failed for a structural reason worth writing down so it is not repeated:

**Any architecture that moves a frame's worth of pixels between the renderer and the UI
per interaction has already lost.** A 1600×1067 preview is ~7 MB of RGBA. Serialised as
JSON it is an order of magnitude worse. There is no amount of optimisation downstream of
that decision that recovers it — the fix is to not have the boundary.

In the native design the sensor data is uploaded to the GPU **once**, and a slider drag
writes **32 bytes to a uniform buffer**. The image never travels again.

The same reasoning rules out a webview on Android, where the copy is most expensive and
the hardware least able to absorb it.

## Target platforms

| Platform | Status | Shell |
|---|---|---|
| Linux (Wayland/X11) | PoC target | `winit` + `egui-winit` |
| Windows | planned | same, new bootstrap |
| macOS | planned | same, new bootstrap |
| Android (APK) | planned | `winit` + `android-activity`, same UI code |

All four run the same engine crate and the same egui UI. Only the entry point differs.
This is the entire reason for the crate split described below — an Android build must be
a new `main.rs`, never a second implementation.

---

# The engine

## Crate layout

```
crates/
  photo-engine/     wgpu device, textures, shaders, decode. Knows nothing about windows.
  photo-app/        egui UI. Knows nothing about winit.
  duckfoot/         bootstrap: winit. The only platform-specific crate.
```

`photo-engine` takes a `wgpu::Device` and `Queue` it did not create. `photo-app` takes an
`&mut Engine` and an `&egui::Ui`. Neither can reach a window handle, which is what
makes the Android bootstrap a small file instead of a fork.

## The pipeline

```
  FILE ──► decode (CPU, once per image)
              │  rawloader → u16 CFA mosaic + metadata
              ▼
        upload (once)  ──►  MOSAIC TEXTURE   R16Uint, immutable
                                  │
              ┌───────────────────┘
              ▼
        PASS 1  demosaic          ──►  SENSOR TEXTURE  RGBA16Float, immutable
              │                         linear camera-native RGB
              │
              ▼
        PASS 2  the adjustment chain — ONE fragment shader, ONE pass
              │   levels · white balance · camera→working matrix · exposure
              │   tone curve (1D LUT texture) · filmic · colour balance
              │   all driven by ONE uniform buffer
              ▼
        PASS 3  output transform → sRGB ──► egui texture, presented
              │
              └─► PASS 2b (compute, atomics) ──► histogram buffer, 4 KB readback
```

**Passes 1 is run once per image. Pass 2 and 3 run per frame. Nothing is read back per
frame except 4 KB of histogram bins.**

## Why the whole chain is one shader pass

This is the central performance decision, and it falls directly out of the product thesis.

DESIGN's Barrel 1 rule has always been that **the module stack is fixed-order and not
rearrangeable** — that is the line that stops the photo studio becoming a node graph.

A fixed order means the chain is known at compile time. A known chain can be **fused into
a single fragment shader**: one texture read and one texture write per pixel for the
entire fourteen-module stack, instead of a round trip to VRAM per module.

The old JS implementation allocated a buffer per module. At preview scale that is ~20 MB
of traffic per enabled module per frame. Fusing the chain makes the pipeline
memory-bandwidth-optimal, and it is *the refusal to allow reordering that permits it*.

The product constraint and the fast implementation are the same decision. Do not relax one
without understanding that it costs the other.

## Disabled modules

A disabled module is a branch in the shader on a uniform flag, not a recompile. Uniform
branching is coherent across the whole draw — every invocation takes the same side — so it
costs essentially nothing. Do not build a shader permutation cache for this; it is the
classic premature optimisation here and it makes the "one render path" invariant
unverifiable.

## Preview and export

They are the same shader, the same uniforms, and the same code path. The *only* difference
is which texture is bound as the source: a fit-to-window mip for preview, the full-
resolution sensor texture for export.

This is not a convention to be maintained by discipline — it is the only way the code is
written. There is no second renderer to drift.

## Colour

The decoder hands over camera-native linear RGB. It is not sRGB and must not be treated as
sRGB. The chain is:

```
camera RGB ──[xyz_to_cam⁻¹]──► XYZ ──[Bradford]──► working space ──[OETF]──► display
```

The previous implementation skipped this entirely and encoded camera primaries as if they
were sRGB. Every image came out wrong in a way that looks like a bad white balance and
isn't. The matrix comes from the file — `rawloader` exposes `xyz_to_cam` — and applying it
is not optional.

White balance is applied as camera-neutral channel multipliers from `wb_coeffs`, **before**
the matrix. A kelvin/tint UI control modulates those multipliers; it does not replace them.

## Decoding

`rawloader` 0.37, behind a `RawDecoder` trait.

It is LGPL-2.1, as is `rawler` — the licence is a wash between them, so the choice is on
merit and convenience. `rawloader` wins on convenience today: it is a two-call API that
hands over exactly the fields needed (`cfa`, `crops`, `wb_coeffs`, `xyz_to_cam`, `cpp`,
`blacklevels`, `whitelevels`), and it cross-compiles cleanly.

`rawler` (~2,500 camera models vs a few hundred, actively maintained) is the known upgrade
and the reason the trait exists. Swapping is one module.

**The LGPL-2.1 obligation is real and unresolved.** Static linking into an MIT binary
carries a relink obligation. This must be settled before any binary is distributed. It does
not block the PoC.

### What the decoder must not skip

The previous decoder ignored most of the file and produced green, uncropped, wrong-primaried
images from a hardcoded RGGB assumption. All of the following are mandatory:

- `crops` — or the masked sensor border is in the frame
- `cfa` — the real pattern, not an RGGB guess; BGGR/GRBG/GBRG swap red and blue
- `wb_coeffs` — or every image is ~2× green
- `xyz_to_cam` — or the primaries are wrong
- `cpp` — a 3-component file is not a mosaic and must not be indexed as one

---

## Invariants — do not route around these

1. **A tool never owns its document.** State lives above the UI. *Because the previous
   build destroyed the user's work on every tool switch.*

2. **One render path serves preview and export.** Same shader, same uniforms; only the
   source texture's resolution differs. *Because a second export path always drifts from
   what the user was looking at, and the last build had two implementations of the same
   six modules in two languages that already disagreed.*

3. **Every mutation is a command on an undo stack.** No exceptions, no "just this one
   little toggle." *Because undo is what makes people trust an editor, and it cannot be
   retrofitted.*

4. **No GPU→CPU readback in the interactive path.** Export reads back. Scopes read back
   4 KB of histogram bins. Nothing else, ever. *Because a per-frame readback is the
   webview mistake wearing a different hat.*

5. **The decoded sensor texture is immutable.** Every adjustment is a shader reading it.
   Nothing writes to it after upload. *Because in-place mutation is how "revert to
   original" quietly stops working — and on the GPU this one is free, so there is no
   excuse.*

6. **A slider drag writes a uniform buffer.** It does not re-decode, does not re-upload a
   texture, and does not allocate. If an interaction does any of those, it is a bug.

7. **No timeline outside Barrel 3.** This is the product thesis. It is not negotiable for
   convenience.

8. **The docs document only what runs.** A stub throws when called, is tagged `@stub`, and
   never gets a checkmark before it works end to end. The stub audit covers Rust as well
   as everything else — the last one only grepped `packages/*/src`, which is precisely
   where the unfinished code wasn't.

---

## Look and feel

**The UI must never compete with the media.** The image is the only saturated pixel
region on screen. Everything else recedes.

- **Dark by default**, and only dark. Neutral greys — gunmetal, not blue-grey. Surfaces at
  neutral-950/900, borders at neutral-800, text at neutral-100 with neutral-400 secondary.
- **One accent, used sparingly.** Warm brass/amber, after the duckfoot pistol — brass and
  walnut. Not indigo; indigo-on-zinc is the default generated-app look.
- **Semantic colour is reserved and never decorative:** red = destructive, accent wash =
  selection, high-contrast = active value.
- **No gradients, shadows or glows on or adjacent to the media surface.** They lie about
  what you're editing.
- Monospace for all numbers — dimensions, EV, kelvin, percentages. They are read as values,
  not prose.
- Every control that changes the image shows its current numeric value. No mystery sliders.

These constraints suit an immediate-mode UI unusually well. egui is poor at gradients,
shadows and glows, and this design bans all three.

## Shared shell

One frame, three fills — the furniture is identical across barrels so learning one teaches
the others:

```
+------------------------------------------------------+
|  brand   [ Photo | Audio | Video ]        status      |
+--------+-----------------------------+---------------+
|        |                             |               |
| Shelf  |      barrel workspace       |   Inspector   |
|        |                             |               |
+--------+-----------------------------+---------------+
|              transport / tool actions                |
+------------------------------------------------------+
```

In the PoC the Photo tab is the only one that mounts. The Shelf and the tab bar are built,
because their absence would let Barrel 1 grow assumptions that the other two can't satisfy.

## Interaction laws

- **Keyboard first.** `Esc` cancels the current gesture. `Ctrl+Z` undoes, everywhere, always.
- **Hotkeys never fire while a text field has focus.** Check it for every hotkey.
- **Every destructive action is undoable rather than confirmed.** No "are you sure" dialogs;
  that is what the undo stack is for.
- **Drag has a live preview and a cancel.** Show the result during the drag, revert on `Esc`.
- **Long operations show real progress and can be cancelled.** Export is the only modal-ish
  state, and even then the Shelf stays visible.
- **Failures surface in the UI, never only in the console.** A file that won't decode says
  so, in place, with its filename.

## Done means (Barrel 1 PoC)

Open a camera RAW file. See it correctly demosaiced, white-balanced and colour-converted —
not green, not cropped wrong, not in the wrong primaries. Drag exposure, tone curve, filmic
and colour balance with **no perceptible latency on a full-resolution 24 MP file**. Watch a
live histogram. Undo every change. Export a PNG that matches the screen exactly.

That is the whole PoC. Anything else is out of scope until it passes.

---

# The other two barrels — design only, not scheduled

Recorded so Barrel 1 does not grow assumptions that rule them out. **No code.**

**Barrel 2 — Audio Cutter.** Load one or more time-aligned sources. Balance them, align
their phase, find the part you want, envelope it, clean it, export it.
*Is not:* a DAW, a mixer with an effects rack, a recorder.

It is a **stem stack, not a timeline**. Stems are time-aligned by definition. N lanes locked
at `t=0`, per-lane gain/mute/solo and phase alignment (polarity, bounded ±50 ms fractional
delay, phase rotation), one shared playhead and in/out selection across the whole stack, one
4-node gain envelope over the selection. Exports a mixdown WAV or trimmed stems.

Refuses: no horizontal positioning handle, no clips, no effects rack, no general automation
curves, no recording, no EQ. They are **lanes**, never tracks — different word, different
mental model, and it stops the distinction eroding the first time someone reasonably asks
"can I just nudge this one a bit."

**Barrel 3 — Video Editor.** Three fixed tracks — overlay, main, audio. Sequence, split,
trim, export. *Is not:* nested compositions, unlimited tracks, keyframe curves, a node graph.

It keeps a *single* audio track. You combine music and voice in Barrel 2, export the mixdown
to the Shelf, and drop that one file onto Barrel 3's audio track.

**The Shelf is the stock.** One asset list, shared by all three, and the only route between
them. An export from any barrel lands back in the Shelf, not in `~/Downloads`. That
round-trip is the entire reason these three things live in one app.

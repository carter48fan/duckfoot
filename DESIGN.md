# Duckfoot — Design Brief

## Mission

Duckfoot fires three purpose-built barrels from one stock. Each barrel does one job
completely and refuses the other two.

The measure of success is not what Duckfoot can do — it's what it declines to do. Every
feature request answered with *"that belongs in another barrel, or nowhere"* is the
design working. Build tools a friend can open, do one thing, and close: no project
wizard, no onboarding, no mode you can get lost in. (The phone's short tour is offered —
its chip is lit until it has been seen — and never started on its own.)

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
writes **one uniform buffer — a few hundred bytes**. The image never travels again.

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
  duckfoot/         the winit shell, and the desktop main.
  duckfoot-android/ android_main, logcat, and the bridge to android/'s Java activity.
```

`photo-engine` takes a `wgpu::Device` and `Queue` it did not create. `photo-app` takes an
`&mut Engine` and an `&egui::Ui`. Neither can reach a window handle, which is what
makes the Android bootstrap a small file instead of a fork.

## The pipeline

```
  FILE ──► decode (CPU, once per image)
              │  rawler (rawloader fallback) → u16 CFA mosaic + metadata
              ▼
        upload (once)  ──►  MOSAIC TEXTURE   R16Uint, immutable
                                  │
              ┌───────────────────┘
              ▼
        PASS 1  demosaic          ──►  SENSOR TEXTURE  RGBA16Float, immutable
              │                         RCD, four passes, for Bayer;
              │                         two passes of its own for X-Trans
              │                         linear camera-native RGB
              │                         highlight reconstruction (neutral in A)
              │
              ▼
        PASS 2  the adjustment chain — ONE fragment shader, ONE pass
              │   levels · white balance · camera→working matrix · exposure
              │   tone curve (1D LUT texture) · filmic · colour eq · saturation
              │   · colour balance (after saturation, so black and white can be toned)
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

**As built today** the working space is linear Rec.2020 for new edits; sidecars saved
before it existed keep linear sRGB (`PhotoStack::working_space`). One `cam_to_srgb` matrix
(the steps above, folded together, dcraw-normalised) takes camera RGB to sRGB primaries and
is composed with sRGB → Rec.2020, so colours outside sRGB carry through as negative sRGB
channels, which every stage must tolerate (see the NaN guard in local contrast). At the end,
`finish()` converts to the output's primaries (sRGB, P3 or Adobe RGB) and brings anything
outside them in towards the grey of its own luminance rather than clipping per channel.

Fixes that change how a photo looks are versioned like the working space, so a saved edit
is drawn as it was: `PhotoStack::rendering` is `V1` for a sidecar without it and `V2` for
every new edit. V2 rolls highlights off by their brightest channel (keeping their hue),
fades colours past white to white as their luminance passes one stop over instead of sending
them to grey, and honours a DNG's AnalogBalance and CameraCalibration on the ForwardMatrix
path. The export's resize, which no edit's look depends on, is in linear light for all.

The previous implementation skipped this entirely and encoded camera primaries as if they
were sRGB. Every image came out wrong in a way that looks like a bad white balance and
isn't. The matrix comes from the file — the decoder exposes `xyz_to_cam` — and applying it
is not optional.

White balance is applied as camera-neutral channel multipliers from `wb_coeffs`, **before**
the matrix. A kelvin/tint UI control modulates those multipliers; it does not replace them.

## Demosaic: green first, then colour differences

Green is reconstructed first and alone. It carries most of the luminance, it is sampled at
twice the density of red or blue, and every other channel is derived from it — so an error
here propagates into all three.

Green is interpolated **directionally**: estimate the gradient horizontally and vertically
and interpolate along whichever is smoother, never across an edge. The centre photosite's
own curvature corrects the result, because at an edge all three channels turn together.
Averaging across edges instead is what produces zipper artefacts.

Red and blue are then interpolated as **colour differences** (`R - G`, `B - G`) rather than
directly. This is the step that removes false colour. Red and blue are sampled at a quarter
of the sensor's density, so above that frequency they alias, and the aliases land in
different places per channel — which is exactly what magenta and green speckle is. Chroma,
though, varies slowly in almost every real scene even where luminance does not, so the
difference signal survives the sparse sampling. Detail comes from green; colour comes from a
signal smooth enough to interpolate.

Measured against a ground-truth neutral pattern, direct interpolation scores a mean false
colour of 49.5/255. Colour-difference interpolation scores 0.

## Highlight reconstruction is not optional either

A photosite saturates at a fixed level, so a blown specular reads as the white level in all
three channels no matter what colour it actually was. White balance then multiplies those
equal readings by unequal gains — `[2.03, 1.00, 1.80]` on a Sony A7 III — and the colour
matrix subtracts the inflated red and blue from green. Every blown highlight comes out
magenta.

For a neutral subject **green saturates first**, because its balance gain is 1.0 while red
and blue are above it. So the damaging case is *partial* clipping, not total: a fully
clipped pixel clamps to white anyway, which is why a test built on one proves nothing.

A saturated photosite carries no colour information, so the only defensible colour for it is
neutral, at green's level: on a neutral subject green is the first to saturate, so at the
clip it *is* the clip level, and at the rim of a blown area it is the one channel not taken
through a colour difference. (The minimum channel, which this used to be, turns rim pixels
black wherever red or blue undershot to zero.) An unclipped neutral pixel already has three
equal channels, so it is a no-op everywhere the sensor had range left.

Two things decide *where* it applies, and both were magenta in practice:

- **Judge the photosites, not the pixel.** A pixel next to a clipped site is built partly
  from it and is wrong by unequal amounts per channel, while its own demosaiced value can sit
  well under the clip. So a pixel is reconstructed if it, or any photosite in the 3×3 block
  around it, is near saturation. Otherwise every blown specular keeps a one-pixel magenta rim.
- **Do it before the mips, and store it linearly.** Reconstruction is not linear, and the
  preview samples a box-filtered mip: a texel half blown and half not averages to a peak
  nowhere near the clip and comes through magenta. So it runs once, in the demosaic, and the
  sensor texture holds each pixel as `rgb` (the camera colour still trusted) plus `a` (the
  neutral replacing the rest). Both average correctly into every mip, and pass 2 recombines
  them as `rgb × wb + a` — white balance scales the measured colour and never tints the
  neutral.

This recovers **colour, not detail**. Texture inside a blown highlight is gone at capture.

## Decoding

`rawler` (~2,500 camera models, actively maintained) behind a `RawDecoder` trait, with
`rawloader` 0.37 as a fallback for anything rawler refuses. Each is tried once, under
`catch_unwind`, and a failure names both reasons. rawloader goes once a real-camera corpus
shows no camera depends on it.

**The LGPL-2.1 obligation is real.** Static linking into an MIT binary carries a relink
obligation. Releases meet it by shipping the complete source with every binary
(`duckfoot-<version>-source.tar.gz`, built by `build.yml`), so anyone can rebuild against a
modified rawler or rawloader.

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
  selection, high-contrast = active value, green = kept (a pick, a saved sidecar). Clipping
  follows the convention every RAW editor shares: red for blown highlights, blue for
  crushed shadows.
- **Colour that is information may be shown as colour.** The white-balance rails are
  painted from blue to amber and green to magenta because that is what they do; that is a
  scale, not decoration, and it is the one gradient allowed.
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

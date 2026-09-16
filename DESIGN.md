# Duckfoot — Design Brief: The Three Barrels

## Mission

Duckfoot fires three purpose-built barrels from one stock. Each barrel does one job
completely and refuses the other two.

The measure of success is not what Duckfoot can do — it's what it declines to do. Every
feature request answered with *"that belongs in another barrel, or nowhere"* is the
design working. Build tools a friend can open, do one thing, and close: no project
wizard, no onboarding, no mode you can get lost in.

## The three barrels

**Barrel 1 — Photo Studio.** Load one image. Adjust, crop, brand, export.
*Is not:* a layer compositor, a RAW developer, a batch processor.

**Barrel 2 — Audio Cutter.** Load one or more time-aligned sources. Balance them, find
the part you want, cut it, clean it, export it.
*Is not:* a DAW, a mixer with an effects rack, a recorder.

**Barrel 3 — Video Editor.** Three fixed tracks — overlay, main, audio. Sequence, split,
trim, export.
*Is not:* nested compositions, unlimited tracks, keyframe curves, a node graph.

**The Shelf is the stock.** One asset list, shared by all three, and the only route
between them. An export from any barrel lands back in the Shelf, not in `~/Downloads`.
That round-trip is the entire reason these three things live in one app.

## Barrel 2 is a stem stack, not a timeline

Stems are time-aligned by definition: three tracks out of a DAW all start at zero and
run the same length. That needs a **stack**, not a timeline.

```
BARREL 2 — stem stack

  drums   [~~~~~~~~~~~~~~~~~~~~~~~~~~]  gain --O--  M  S
  bass    [~~~~~~~~~~~~~~~~~~~~~~~~~~]  gain --O--  M  S
  vox     [~~~~~~~~~~~~~~~~~~~~~~~~~~]  gain --O--  M  S
                ^         :  in/out spans ALL lanes
             playhead     :
```

**Has:** N lanes locked at `t=0`. Per-lane gain, mute, solo. One shared playhead and one
shared in/out selection across the whole stack. Trim, delete-region, normalize and fades
apply to the selection across all active lanes. Exports either a mixdown WAV or the
trimmed stems individually.

**Refuses** — this is the line that keeps it from becoming Audacity:

- **No per-lane horizontal offset.** Lanes are locked at zero. The moment one lane can
  slide against another you must visualise the offset, and that visualisation *is* a
  timeline. If you need offset, that's Barrel 3.
- **No clips.** A lane is one continuous source, not a sequence. No splitting a lane.
- **No effects rack.** Gain, normalize, fades. That is the whole verb list.
- No recording, no automation curves, no EQ.

They are **lanes**, never tracks — in the code and in the UI. Different word, different
mental model, and it stops the distinction eroding the first time someone reasonably
asks "can I just nudge this one a bit."

**Consequence for Barrel 3:** it keeps a *single* audio track. You combine music and
voice in Barrel 2, export the mixdown to the Shelf, and drop that one file onto Barrel
3's audio track. That is the shared Shelf earning its place.

## Invariants — do not route around these

1. **A tool never owns its document.** State lives above the components; all three stay
   mounted. *Because the previous build conditionally rendered them, so every barrel
   switch silently destroyed the user's work.*
2. **One render function serves preview and export.** If you can't export it, don't
   render it. *Because a second export path always drifts from what the user was
   looking at.*
3. **Every mutation is a command on an undo stack.** No exceptions, no "just this one
   little toggle." *Because undo is what makes people trust an editor, and it cannot be
   retrofitted.*
4. **Preview is approximate, export is exact.** Preview works on downscaled or decimated
   data and must stay interactive. Full-resolution work happens at export only, off the
   main thread. *Because full-res pixel loops per slider tick will freeze the tab.*
5. **Never mutate a source buffer or bitmap in place.** Operations return new data.
   *Because in-place mutation is how "revert to original" quietly stops working.*
6. **No timeline outside Barrel 3.** This is the product thesis. It is not negotiable
   for convenience.
7. **The README documents only what runs.** Nothing gets a checkmark before it works
   end-to-end.

## Look and feel

**The UI must never compete with the media.** The image, the waveform and the video
frame are the only saturated pixels on screen. Everything else recedes.

- **Dark by default**, and only dark. Neutral greys — gunmetal, not blue-grey. Surfaces
  at neutral-950/900, borders at neutral-800, text at neutral-100 with neutral-400 for
  secondary.
- **One accent, used sparingly.** Warm brass/amber, after the duckfoot pistol — brass
  and walnut. Not indigo; indigo-on-zinc is the default generated-app look.
- **Semantic colour is reserved and never decorative:** green = in-point, red = out-point
  and destructive actions, white/high-contrast = playhead, accent wash = selection.
- **No gradients, shadows or glows near the media surface.** They lie about what you're
  editing.
- Monospace for all numbers — timecode, dimensions, sample rate, percentages. They are
  read as values, not prose.
- Every control that changes the media shows its current numeric value. No mystery
  sliders.

## Shared shell

One frame, three fills:

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

Shelf and Inspector are collapsible and remember their state. Only the middle and the
transport row change between barrels. A user who learns one barrel has already learned
the furniture of the other two.

## Interaction laws

- **Keyboard first.** `Space` is transport in every barrel that has one. `Esc` cancels
  the current gesture. `Ctrl/Cmd+Z` undoes, everywhere, always.
- **Hotkeys never fire while a text field has focus.** Check the event target — for
  every hotkey, not just `Space`.
- **Every destructive action is undoable rather than confirmed.** No "are you sure"
  dialogs; that is what the undo stack is for.
- **Drag has a live preview and a cancel.** Crop boxes, trim handles, clip moves: show
  the result during the drag, revert on `Esc`.
- **Long operations show real progress and can be cancelled.** Export is the only
  modal-ish state in the app, and even then the Shelf stays visible.
- **Failures surface in the UI, never only in the console.** A file that won't decode
  says so, in place, with its filename.

## Done means

**Photo:** open an image, drag a crop box with an optional locked ratio, move every
slider without stutter on a 24 MP file, add a watermark, export PNG/JPEG/WebP where the
file matches the preview exactly, undo every step back to the original.

**Audio:** load three stems, balance and solo them, scrub, set in/out and hear *only*
the selection across all lanes, trim and delete-region, normalize with a visible result,
fade in/out, revert to the true original, export a mixdown WAV that opens correctly in
another editor.

**Video:** drag an asset from the Shelf onto the right track, move and trim clips with
snapping, split at the playhead, add a title, hear the audio track during preview, and
export an MP4 with correct frames *and* audio that plays in an external player.

## Order of work

Build the shared shell and the document/undo layer first, with one barrel wired through
it end to end — Photo, because it is the smallest complete loop. Prove that export
matches preview and that undo survives a barrel switch. Only then start the second
barrel.

Do not build three barrels to 80%. One finished barrel teaches you what the shared layer
actually needs; three unfinished ones teach you nothing and hide the same bug in
triplicate.

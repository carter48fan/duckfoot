# Changelog

What changes for people using Duckfoot, newest first. Versions are the release tags.

## 1.0.0

### New
- **Find a mask:** *+ subject* finds the photo's main subject, *+ sky* guesses where the sky
  is; each becomes a mask like any other.
- **Black & white mixer:** how light each colour turns out in grey, and still toned by
  colour balance after.
- **Film grain**, strongest in the mid-tones.
- **Coarse grain** in noise reduction, for the blotches high ISO leaves.
- **Survey (N):** the selected photos side by side, to choose between; marks go to the one
  framed.
- **Folders** in the left panel: the shelf's folders as a tree, one click to show only one.
- **Title, caption, creator and copyright** for each photo, kept in its XMP and in exports.
- **Text watermark** on exports and batch recipes.
- **More RAW formats** accepted: SRW, MRW, CRW, 3FR, FFF, ERF, DCR, DCS, KDC, MEF, MOS, IIQ,
  RWL, ORI (not yet tried on real files from each camera).
- **Photos too large for the GPU** open at a fraction of their size, with a notice, instead
  of not opening.
- **About & licences** in the app menu, with the version and every notice.
- **Copy and paste** of text with other apps (the desktop had no clipboard).
- The window reopens where it was, and its title names the open photo.

### Faster, and no more freezes
- Exporting, the cutout's subject finding, Remove spots and rating many photos at once no
  longer stop the app while they work; an export can be cancelled.
- A big shelf (tens of thousands of photos) no longer slows every action.
- Folder watching only runs while the window has focus.

### Fixed
- Batch exports now correct the lens as the editor does.
- Edits to a photo in a read-only folder are kept, and quitting asks before losing edits
  that could not be saved.
- Keys no longer act on the photo behind Preferences; Ctrl+A selects only what the filter
  shows; Ctrl+Y redoes; a Mac shows ⌘ ⇧ ⌥ for shortcuts.
- A file can no longer make the app hang (a flood of keywords), crash a decoder (an
  oversized DNG gain map), black out the render (infinite slider values), or open a look on
  a network path.
- A JPEG's ratings no longer go into its RAW's `.xmp`.
- CJK file and folder names show instead of boxes (using the system's font).

### Android
- The app is now signed with its own key, and test builds are a separate app
  ("Duckfoot testing") that can never replace it. **Earlier versions were signed with a
  public testing key: uninstall them once before installing this one** (export anything you
  want to keep first).
- A phone with no usable GPU now says so instead of showing a black screen.

### Releases
- Releases come only from tagged versions, after the tests pass on Linux, Windows and
  macOS, and carry checksums and build-provenance attestations.

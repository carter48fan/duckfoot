# 🦆 Duckfoot

**A local-first FOSS creative suite: focused Photo, Audio and Video editing.**

> **Status: pre-alpha. Nothing runs yet.**
> The application shell was demolished deliberately and is being rebuilt. What remains
> in this repository is the media/DSP core and the three tool components, kept as
> reference for the rewrite. There is no runnable app, no desktop build and no release.

---

## The idea

Traditional editors force every creative task through one monolithic timeline. Slicing a
podcast or cropping a product photo shouldn't require a 500 MB NLE with twenty video
tracks and a cloud account.

Duckfoot mounts three dedicated studios onto one shared asset shelf — like a duckfoot
pistol firing distinct barrels from a single stock.

```
                    ┌────────────────────────┐
                    │      Asset Shelf       │
                    │   (local-first stock)  │
                    └───────────┬────────────┘
             ┌──────────────────┼──────────────────┐
             ▼                  ▼                  ▼
      [ 📷 Photo ]        [ 🎵 Audio ]       [ 🎬 Video ]
       one image           stem stack         3-track timeline
       no timeline         no timeline        the only timeline
```

The design brief — including what each barrel deliberately refuses to do — is in
[DESIGN.md](./DESIGN.md). Read that before contributing.

### Principles

1. **Focused simplicity.** Dedicated views for dedicated jobs. No timeline outside the
   video editor.
2. **Local-first, zero-cloud.** No logins, no database, no analytics. Your media never
   leaves your device.
3. **One codebase, native and web.** A Tauri desktop app, plus a static browser build.

---

## Repository layout

```
duckfoot/
├── DESIGN.md              # the design brief — start here
└── packages/
    ├── core/              # domain types, storage drivers, math + colour helpers
    ├── media/             # RMS waveforms, WAV encoder, image filters, video exporter
    ├── tool-photo/        # Photo Studio component  (reference; pending rewrite)
    ├── tool-audio/        # Audio Cutter component  (reference; pending rewrite)
    └── tool-video/        # Video Editor component  (reference; pending rewrite)
```

`apps/` is intentionally absent. The web shell is being rebuilt as a plain Vite SPA and
the desktop shell will be regenerated with the Tauri CLI.

---

## Current state, honestly

**Works:** the pure functions in `packages/media` — RMS waveform computation, audio
slice/normalize/fade, 16-bit PCM WAV encoding, canvas image adjustments, and a
`mediabunny` WebCodecs export pipeline with a MediaRecorder fallback.

**Reference only:** the three `tool-*` components render and lay out correctly, but hold
all state internally, have no undo, and carry known defects (in-place buffer mutation,
unawaited video seeks during export, full-resolution work on the UI thread). They are
kept so their canvas and layout code can be lifted into the rewrite. Do not treat them
as working software.

**Not built:** the asset shelf, persistence, the document/undo model, photo crop, the
stem stack, timeline editing, and both application shells.

---

## Roadmap

- [ ] Shared shell: asset shelf, document model, undo/redo
- [ ] Local-first persistence (OPFS on web, Tauri fs on desktop)
- [ ] Barrel 1 — Photo Studio end-to-end
- [ ] Barrel 2 — Audio stem stack
- [ ] Barrel 3 — 3-track video timeline with MP4 export
- [ ] Tauri desktop shell
- [ ] Offline-capable PWA build

---

## Development

Requires Node 20+ and pnpm 9+.

```bash
pnpm install
pnpm typecheck
```

That is currently the entire build surface. `pnpm dev` returns once there is an app to
run.

---

## Licence

MIT — see [LICENSE](./LICENSE). Duckfoot contains code derived from
[OpenCut](https://github.com/OpenCut-app/OpenCut), also MIT; their copyright notice is
retained.

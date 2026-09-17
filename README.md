# 🦆 Duckfoot

**A local-first FOSS creative suite: focused Photo, Audio and Video editing.**

> **Status: Alpha.**
> The web shell is rebuilt as a Vite SPA with a unified document model, snapshot undo/redo,
> and two barrels running: Photo Studio (RAW pipeline) and Audio Cutter (stem stack with phase alignment).
> Video Editor is preserved as reference and registered as a stub.

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
       RAW processor       stem stack         3-track timeline
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
├── apps/
│   └── web/               # Vite SPA shared application shell
└── packages/
    ├── core/              # document model, snapshot undo, storage drivers, tokens
    ├── media/             # RMS waveforms, WAV encoder, RAW scene, audio DSP
    ├── ui/                # shared controls: sliders, chips, gestures, focus rings
    ├── tool-photo/        # Barrel 1: Photo Studio (RAW pipeline, scopes, A/B split)
    ├── tool-audio/        # Barrel 2: Audio Cutter (stem stack, phase alignment, envelope)
    └── tool-video/        # Barrel 3: Video Editor (reference; pending turn 3 rebuild)
```

---

## Current state, honestly

| Area | Export / Symbol | Status | Notes |
|---|---|---|---|
| **Core** | `createDocument`, `execute`, `undo`, `redo` | Working | Snapshot-based undo reducer with command coalescing |
| **Core** | `OpfsStorageDriver`, `MemoryStorageDriver` | Working | Local-first browser storage and memory fallback |
| **Core** | `tokensToCssText`, design tokens | Working | Single TS source of truth injecting `:root{--df-*}` |
| **Media** | `computeHistogram`, `pickSample` | Working | Real preview histogram and sample picker |
| **Media** | `decodeRasterAsScene` | Working | Lifts 8-bit raster to scene-referred linear float |
| **Media** | `invertPolarity`, `applyIntegerDelay` | Working | Exact per-lane phase and integer delay |
| **Media** | `evaluateEnvelopeDb`, `buildDuckEnvelope`| Working | Evaluates envelope curve at sample time |
| **Media** | `decodeRaw` | `@stub` | Camera RAW demosaicing (CR3, NEF, ARW) |
| **Media** | `crossCorrelate`, `autoAlign` | `@stub` | Inter-lane cross-correlation and phase alignment |
| **Media** | `rotatePhase` | `@stub` | Broadband Hilbert phase rotation |
| **Media** | `computeWaveform`, `computeVectorscope`| `@stub` | RGB parade and CbCr vectorscopes |
| **Tool Photo** | `PhotoWorkspace`, `PhotoInspector` | Working | 14-module RAW pipeline, before/after split, snapshots |
| **Tool Audio** | `AudioWorkspace`, `AudioInspector` | Working | Stem stack, phase controls, 4-node gain envelope |
| **Tool Video** | `VideoEditor` | Reference | 544 lines of reference implementation; stub mounted |

Audit stubs anytime with `pnpm stubs`.

---

## Roadmap

- [x] Shared shell: asset shelf, document model, snapshot undo/redo
- [x] Local-first persistence (OPFS driver, Memory fallback)
- [x] Barrel 1 — Photo Studio (RAW processor, 14-module pipeline, scopes, before/after split)
- [x] Barrel 2 — Audio stem stack (phase alignment, 4-node gain envelope, mixdown)
- [ ] Barrel 3 — 3-track video timeline with MP4 export
- [ ] Tauri desktop shell
- [ ] Offline-capable PWA build

---

## Development

Requires Node 20+ and pnpm 9+.

```bash
pnpm install
pnpm typecheck
pnpm build
pnpm dev
```

Verification helpers:

```bash
pnpm run lint:vocab    # enforces lanes-never-tracks invariant in audio & ui
pnpm run stubs         # audits all @stub tags across packages
```

---

## Licence

MIT — see [LICENSE](./LICENSE). Duckfoot contains code derived from
[OpenCut](https://github.com/OpenCut-app/OpenCut), also MIT; their copyright notice is
retained.

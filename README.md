<div align="center">

# 🦆 Duckfoot

### The Multi-Barreled Media Editing Platform
**A lightweight, local-first FOSS creative suite for focused Photo, Audio, and Video editing.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS%20%7C%20Windows%20%7C%20Web-indigo.svg)](#cross-platform-support)
[![Engine](https://img.shields.io/badge/Stack-Tauri%20v2%20%2B%20Vite%20%2B%20React%2019-emerald.svg)](#technology-stack)
[![Package Manager](https://img.shields.io/badge/Package%20Manager-pnpm-orange.svg)](#development--running)

---

</div>

## 💡 Design Philosophy

Traditional media editors force every creative task into a monolithic, bloated non-linear video timeline. Slicing a podcast episode or cropping a product photo shouldn't require opening a 500MB NLE with 20 video tracks, nested compositing graphs, and cloud database requirements.

**Duckfoot** is designed on a different principle: **The Multi-Barreled Architecture**.

Just like a historical duckfoot pistol fires distinct barrels from a single stock, Duckfoot mounts three dedicated, purpose-built creative studios onto one shared asset foundation:

```
                            ┌────────────────────────┐
                            │   Shared Asset Shelf   │
                            │ (Local-First / Memory) │
                            └───────────┬────────────┘
                                        │
             ┌──────────────────────────┼──────────────────────────┐
             ▼                          ▼                          ▼
   [ 📷 Barrel 1: Photo ]    [ 🎵 Barrel 2: Audio ]     [ 🎬 Barrel 3: Video ]
    • Canvas 2D / WebGL       • Web Audio API            • 3-Tier Timeline
    • Crop / Orientation      • Zoomable Waveform        • Overlay / Main / Audio
    • Color Adjustments       • Slice / Trim / Normalize • Cut / Snap / Ripple
    • Watermark / Export      • 16-bit WAV Export        • Mediabunny MP4 Export
    • NO Timeline!            • NO Multitrack!           • Multi-Track Dedicated
```

### Core Principles

1. **Focused Simplicity:** Dedicated views for dedicated jobs. No multitrack timeline exists anywhere outside the Video Editor.
2. **Local-First & Zero-Cloud:** No logins, no databases (no Postgres/Redis), no analytics, no subscription paywalls. Your media never leaves your device.
3. **Single Codebase, Native Everywhere:** Powered by **Tauri v2**, Duckfoot compiles to a featherweight (<15MB), lightning-fast native desktop application on Pop! OS/Linux, macOS, and Windows while deploying as an offline-capable Progressive Web App (PWA).
4. **Strictly `pnpm` Native:** Built clean from the ground up as a modern monorepo adhering strictly to `pnpm` workspace standards.

---

## 🎯 The Three Barrels

### 📷 Barrel 1: Photo Studio (Zero Timeline Overhead)
Designed for instantaneous photo adjustments, cropping, and branding without opening heavy image editors.
* **Canvas Viewport:** Smooth mouse-wheel zooming, panning, and pixel dimensions inspection.
* **Orientation Transforms:** 90° clockwise rotation, horizontal flip, vertical flip.
* **Non-Destructive Crop:** Freeform cropping or locked aspect ratios (`1:1 Square`, `4:5 Portrait`, `16:9 Landscape`, `9:16 Story`).
* **Real-Time Color Pipeline:** Exposure, Brightness, Contrast, Saturation, Color Temperature (warm/cool), and Vignette.
* **Branding:** Instant text watermark overlay with drop-shadow protection.
* **Export Formats:** Direct export to **PNG** (lossless), **JPEG** (quality-adjusted), and **WebP**.

### 🎵 Barrel 2: Audio Cutter (Single-Track Precision)
A surgical audio tool built on the Web Audio API for trimming voiceovers, samples, and podcast clips.
* **RMS Waveform Visualizer:** High-density RMS peak waveform canvas (harvested and optimized from OpenCut Classic).
* **Clock-Synchronized Playhead:** Real-time Web Audio playback with Spacebar toggle and click-to-seek.
* **In/Out Range Markers:** Draggable range handles with `I` (set In) and `O` (set Out) hotkeys.
* **Non-Destructive Operations:**
  * **Trim Selection:** Crops audio down to the selected range.
  * **Delete Region:** Removes the selected section and seamlessly joins surrounding audio.
  * **Normalize Peak:** Automatically normalizes gain to -0.1 dBFS headroom.
  * **Fades:** One-click 1.5-second linear fade-in and fade-out envelopes.
* **Export:** Instant client-side render to uncompressed **16-bit PCM WAV**.

### 🎬 Barrel 3: Video Editor (Focused 3-Track Timeline)
The only barrel equipped with a timeline—streamlined down to three essential tiers:
* **Track 1 (Overlay):** Title cards, captions, subtitles, and watermarks.
* **Track 2 (Main Video):** Sequential video clips with magnetic snapping, razor split at playhead (`S`), edge trimming, and ripple delete.
* **Track 3 (Audio):** Dedicated background music or voiceover track with independent gain.
* **Canvas Preview Compositor:** Real-time synchronized video and text rendering.
* **Hardware-Accelerated Export:** WebCodecs pipeline powered by `mediabunny` exporting directly to **1080p MP4** (H.264/AAC) with automated fallback to WebM.

### 📦 The Stock: Shared Asset Shelf
* Collapsible drawer accessible across all three tools.
* Drag-and-drop file ingestion for images, audio tracks, and video clips.
* Real-time metadata probing (duration, sample rate, pixel dimensions).
* Auto-category filtering (`All`, `Photo`, `Audio`, `Video`).
* Click any asset to immediately load it into its corresponding barrel.

---

## 🏗️ Repository Architecture

Managed with **`pnpm` workspaces**:

```
duckfoot/
├── apps/
│   ├── desktop/                 # Tauri v2 native desktop application
│   │   ├── src-tauri/           # Rust shell, native filesystem & dialog plugins
│   │   └── package.json
│   └── web/                     # Vite + React 19 + Tailwind v4 Web / PWA app
│       └── src/components/suite/CreativeSuite.tsx
│
└── packages/
    ├── core/                    # Unified storage (OPFS / Tauri fs), math, and color helpers
    ├── media/                   # Media primitives: RMS waveforms, WAV encoder, image filters, video exporter
    ├── tool-photo/              # Photo Studio component & state
    ├── tool-audio/              # Audio Cutter component & state
    └── tool-video/              # 3-Tier Timeline Video Editor & compositor
```

---

## 🚀 Installation & Setup

### Prerequisites

1. **Node.js:** v20.x or higher
2. **pnpm:** v9.x or higher (`corepack enable && corepack prepare pnpm@latest --activate`)
3. **Rust:** (Required only for building native desktop binaries) `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

---

## 💻 Cross-Platform Support & System Requirements

### 🐧 Linux (Pop! OS, Ubuntu, Debian)

Tauri v2 requires the system webview (WebKit2GTK 4.1) and native desktop libraries:

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

### 🍎 macOS

Ensure Xcode Command Line Tools are installed:

```bash
xcode-select --install
```

### 🪟 Windows

1. Install **Microsoft Visual Studio C++ Build Tools** (Select "Desktop development with C++").
2. Ensure **WebView2 Runtime** is installed (pre-installed on Windows 10/11).

---

## 🛠️ Development & Running

### 1. Clone & Install Dependencies

```bash
git clone https://github.com/carter48fan/duckfoot.git
cd duckfoot
pnpm install
```

### 2. Run the Web App (Browser / PWA)

Starts the Vite development server on `http://localhost:5173`:

```bash
pnpm dev
```

### 3. Run the Native Desktop App (Tauri v2)

Launches Duckfoot in a native window with full local filesystem access:

```bash
pnpm desktop:dev
```

### 4. Build Production Binaries

```bash
# Build web distribution (static assets for PWA deployment)
pnpm build

# Build native desktop installers (.deb/AppImage on Linux, .dmg on macOS, .msi on Windows)
pnpm desktop:build
```

---

## ⌨️ Global Keybindings

| Key | Context | Action |
| :--- | :--- | :--- |
| `Space` | Audio Cutter / Video Editor | Toggle Play / Pause |
| `I` | Audio Cutter | Set In-Point at playhead |
| `O` | Audio Cutter | Set Out-Point at playhead |
| `S` | Video Editor | Split selected clip at playhead |
| `Delete` / `Backspace` | Video Editor | Delete selected clip from timeline |

---

## 🗺️ Roadmap

- [x] Creative Triad Architecture (Photo, Audio, Video isolation)
- [x] Shared Asset Shelf with local-first persistence
- [x] Audio Cutter with RMS waveform, peak normalization, and WAV export
- [x] Photo Studio with non-destructive crop, color adjustments, and WebP/PNG export
- [x] 3-Tier Video Timeline with split, trim, and Mediabunny MP4 export
- [x] Tauri v2 desktop shell for Linux (Pop! OS), macOS, and Windows
- [ ] SoundTouch-powered pitch-preserving audio stretch controls
- [ ] LUT color grading filter presets for photos and video
- [ ] Offline WebAssembly Whisper transcription for automated subtitle generation
- [ ] Batch image processing (multi-photo resize & watermark)

---

## 📄 License

Duckfoot is released under the **MIT License**. Free and open-source software for creators everywhere.

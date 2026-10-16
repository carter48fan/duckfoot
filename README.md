# 🦆 Duckfoot

**A fast, free photo editor that keeps your pictures on your device.**

Open a photo, make it look the way you remember, and save a copy to share. Duckfoot works
with ordinary photos and with the RAW files from serious cameras, and it never needs an
account, a subscription or an internet connection.

**[⬇️ Download the latest version](https://github.com/carter48fan/duckfoot/releases/latest)** ·
[All releases](https://github.com/carter48fan/duckfoot/releases) ·
[Help Guide](./docs/GUIDE.md)

> **Duckfoot is an early release.** It is already a full photo editor, but it is young:
> the Windows, Mac and Android versions have had far less testing than Linux, so you may
> hit rough edges. If you do, please [tell us](https://github.com/carter48fan/duckfoot/issues).

---

## What you can do

- **Open almost any photo.** RAW files from Canon, Nikon, Sony, Fujifilm, Olympus,
  Panasonic, Pentax and many more, plus JPEG, PNG, TIFF, WebP and iPhone photos (HEIC).
- **Fix light and colour.** Brightness, shadows and highlights, warmth, vividness, and
  one-tap **looks** such as vivid, warm, film, mono and noir.
- **Sharpen and clean up.** Sharpening, noise reduction for grainy low-light shots, lens
  corrections, and **spot healing** to remove dust and distractions.
- **Edit just one part of a picture** with brushes and gradients.
- **Cut out a background.** Duckfoot finds the person or product and removes what's behind
  it, ready to save with a see-through or plain white background.
- **Crop and straighten**, then compare **before and after** side by side.
- **Work on many photos at once.** Edit one, paste the same look onto the rest, and
  export a whole set in one go.
- **Undo anything.** Every photo keeps its own history, and you can save named snapshots
  of versions you like.

**Your original photo is never changed.** Duckfoot saves your edits in a small file next
to the photo, so you can always get back to exactly how it came out of the camera.

**Private by design.** No logins, no cloud, no tracking. Your photos never leave your device.

---

## Download and install

Go to the **[latest release](https://github.com/carter48fan/duckfoot/releases/latest)**
and scroll down to **Assets**. Pick the file for your device:

| You have | Download the file ending in |
|---|---|
| **Windows** PC (64-bit) | `windows-x86_64.zip` |
| **Mac** with Apple Silicon (M1 or newer) | `macos.dmg` |
| **Android** phone or tablet (Android 11 or newer) | `android-arm64.apk` |
| **Linux** | `linux-x86_64.AppImage` (see [Linux](#linux-advanced)) |

The file names include the version number, for example `duckfoot-v0.3.17-windows-x86_64.zip`,
so look for the ending, not the whole name.

> **A note on warnings.** Duckfoot is a free project and its Windows and Mac downloads are
> not yet signed with a paid developer certificate. Your computer will therefore show a
> warning the first time you open it. That's expected; the steps below show how to continue.

### Windows

1. Download the file ending in `windows-x86_64.zip`.
2. Right-click it and choose **Extract All…**, then **Extract**. (Please extract it first.
   Running Duckfoot from inside the zip won't work.)
3. Open the new folder and double-click **`duckfoot.exe`**.
4. If a blue **"Windows protected your PC"** window appears, click **More info**, then
   **Run anyway**.

There's no installer: the folder *is* the app. Move it wherever you like, and delete the
folder to remove Duckfoot.

### Mac

Duckfoot for Mac is built for **Apple Silicon** (M1, M2, M3 and later). It does not run on
older Intel Macs.

1. Download the file ending in `macos.dmg` and double-click it.
2. Drag **Duckfoot** onto the **Applications** folder.
3. The first time, **right-click** (or Control-click) Duckfoot in Applications, choose
   **Open**, then click **Open** again in the box that appears.
4. If there is no **Open** button, go to **System Settings → Privacy & Security**, scroll
   down to the message about Duckfoot, and click **Open Anyway**.

**If macOS says "Duckfoot.app is damaged and can't be opened"** (some downloads trigger
this), the app isn't damaged: macOS is being cautious about an app that isn't signed by a
registered developer, and it offers no button to continue. Don't move it to the Trash.
Open **Terminal** (Applications → Utilities), paste this line, press Return, then open
Duckfoot again:

```
xattr -dr com.apple.quarantine /Applications/Duckfoot.app
```

It removes the "downloaded from the internet" mark from Duckfoot only.

### Android

1. On your phone or tablet, open the
   [latest release](https://github.com/carter48fan/duckfoot/releases/latest) and download
   the file ending in `android-arm64.apk`.
2. Open the downloaded file. Android will ask you to allow installing apps from this source
   (your browser or Files app). Allow it, then tap **Install**.
3. Open **Duckfoot** from your app list.

New versions install right over the old one and keep your work. On a phone, Duckfoot works
on one photo at a time, and finished photos are saved to **Pictures → Duckfoot**, where your
gallery will show them. You need Android 11 or newer.

### Linux (advanced)

Duckfoot for Linux is a single **AppImage** file. It needs a graphics driver with
**Vulkan** support (almost every recent machine has one), a Wayland or X11 desktop, and a
desktop file-dialog service (`xdg-desktop-portal` plus a backend for your desktop).

```bash
# 1. Make the downloaded file runnable, then start it
chmod +x duckfoot-*-linux-x86_64.AppImage
./duckfoot-*-linux-x86_64.AppImage

# Or open a photo straight away
./duckfoot-*-linux-x86_64.AppImage ~/Pictures/photo.ARW
```

**If something is missing**, on Debian, Ubuntu and Pop!_OS:

```bash
sudo apt install libvulkan1 mesa-vulkan-drivers xdg-desktop-portal-gtk
```

On Arch: `vulkan-icd-loader`, `xdg-desktop-portal-gtk`. Check that Vulkan works with
`vulkaninfo --summary`.

**If the AppImage complains about FUSE**, either install `libfuse2` or run it without FUSE:

```bash
./duckfoot-*-linux-x86_64.AppImage --appimage-extract-and-run
```

**Other ways to run it**

- **Tarball** (`linux-x86_64.tar.gz`): unpack it and run `./duckfoot`. Same requirements.
- **Build it yourself**: you'll need [Rust](https://rustup.rs). See
  [docs/BUILDING.md](./docs/BUILDING.md).
- **Source code** for every release is attached to it (`…-source.tar.gz`).

Every release's files are listed on its [release page](https://github.com/carter48fan/duckfoot/releases).

---

## Your first edit

1. Open Duckfoot and click the middle of the window to choose a photo, or **drag one in**
   from a folder.
2. Use the panel on the right. **Light** is at the top: drag **exposure** right to
   brighten, left to darken.
3. Colours too blue or too orange? Move **temp** under **white balance**.
4. Not sure? **Ctrl+Z** undoes (**⌘Z** on a Mac), and double-clicking any slider puts it
   back.
5. Happy? Click **Export** at the top to save a copy you can share.

You don't need to save. Duckfoot saves your edits by itself as you work.

The first time you open it, Duckfoot offers a one-minute tour. The full
**[Help Guide](./docs/GUIDE.md)** is also inside the app: click **Help** in the top bar.
Press **?** at any time to see every keyboard shortcut.

---

## Troubleshooting

**"Duckfoot needs a GPU … and could not start one."** Duckfoot draws with your graphics
card, so it needs a reasonably recent graphics driver. Update your graphics driver, or on
Linux install a Vulkan driver (see [Linux](#linux-advanced)), and try again.

**Windows or macOS warns me about the app.** That's expected for now; see the
[Windows](#windows) and [Mac](#mac) steps above.

**A photo won't open.** The bar at the bottom of the window names the file and says why.
Some very new or unusual cameras aren't supported yet.

**Everything looks wrong after an edit.** Press **Ctrl+Z** until it looks right, or click
**Reset all** at the bottom of the editing panel.

**A tool is missing.** It may be hidden. Click the gear (⚙) in the editing panel and tick
it, or click **Show all**.

**Linux: the Open button does nothing.** Install a file-dialog service such as
`xdg-desktop-portal-gtk` and log out and back in. Dragging a photo onto the window works
without it.

More fixes are in [docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md). Still stuck?
[Open an issue](https://github.com/carter48fan/duckfoot/issues) and say what you were doing,
which system you're on, and what the bottom bar said.

---

## What's next

Duckfoot is built as three focused studios that share one shelf of files. **Photo** is
here today. **Audio** and **Video** studios are designed and planned, but not built yet.
See [docs/STATUS.md](./docs/STATUS.md) for exactly what works and what's coming.

## For developers and contributors

Everything technical lives in [`docs/`](./docs/README.md):
[building from source](./docs/BUILDING.md), [testing](./docs/TESTING.md),
[architecture](./docs/ARCHITECTURE.md), [status and roadmap](./docs/STATUS.md) and
[keyboard shortcuts](./docs/SHORTCUTS.md). The design brief is [DESIGN.md](./DESIGN.md);
read it before contributing.

## Licence

Duckfoot is free and open source under the **MIT** licence: see [LICENSE](./LICENSE). It
includes open-source components under their own licences (see
[docs/LEGAL.md](./docs/LEGAL.md) and `THIRD_PARTY_LICENSES.md`).

Release builds can open iPhone and Mac HEIC photos, which uses HEVC video technology
covered by patents. No patent licence comes with Duckfoot, and if you redistribute it,
that's something to read up on first: see the
[legal notice](./docs/LEGAL.md#legal-notice-hevc-patents).

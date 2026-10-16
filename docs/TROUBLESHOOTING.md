# Troubleshooting (developer notes)

For the short, everyday version see the [README](../README.md#troubleshooting).

**`could not create a window or GPU device`** — no usable Vulkan adapter. Check
`vulkaninfo --summary`; install `mesa-vulkan-drivers` or your vendor driver.

**Everything is painfully slow** — you built in debug. Use `--release`.

**The Open RAW… button does nothing** — no XDG desktop portal backend. Install
`xdg-desktop-portal-gtk` (or `-cosmic` / `-kde`) and relog. Passing the file as a CLI
argument or dragging it onto the window bypasses the dialog entirely.

**The picker greys out files that are obviously RAW** — the filter list does not cover that
extension. XDG portal globs are case-sensitive, so the filter lists every extension in both
cases; if your camera writes something not in `RAW_EXTENSIONS`, switch the dialog to
**All files**, or pass the path on the command line. Add the extension to
`crates/photo-engine/src/lib.rs` if it decodes.

**A file fails to open** — the status bar gives the filename and the reason. `@stub`
reasons (non-mosaic, float-encoded, 4-colour CFA) are unimplemented, not broken. Run
`--example probe` on the file for detail.

**The image looks green** — that would mean white balance isn't being applied. Check
`--example probe` output: `wb mult` should not be `[1.0, 1.0, 1.0]` for a daylight shot.

**Magenta or green speckle on fine detail** — wire mesh, strings, fabric. That is demosaic
false colour, not a highlight or white-balance fault, and it needs a better demosaic kernel.
Magenta on *blown speculars* specifically is a different thing and is fixed.

**The image looks flat and grey with filmic on** — `latitude` is a percentage of the log
range. Above roughly 60 it compresses everything toward mid grey. The default is 20.

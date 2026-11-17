# Privacy

Duckfoot collects nothing.

- **No network.** The app makes no network connections: no accounts, no analytics, no
  crash reporting, no update checks. It contains no networking code. The Android app asks
  for no permissions at all, the internet included.
- **Your photos stay where they are.** Edits are kept in a small `.duckfoot` file beside
  each photo (or, for a photo whose folder cannot be written, in the app's own data folder),
  and in `.xmp` files only if you turn that on. Nothing is uploaded.
- **What the app keeps for itself**, in its settings folder on your device: its settings
  (the photos on the shelf, panel layout, export choices), presets you save, and a log of
  each run to help diagnose problems. The log names the files opened, so read it before
  sharing it in a bug report.
- **Exports** carry the metadata you choose in the export settings. Location (GPS) is left
  out by default.
- **Android:** photos you pick are copied into the app's private storage to be edited, and
  finished exports are saved to Pictures/Duckfoot. Android's backup does not include the
  photo library.

Questions: open an issue at https://github.com/carter48fan/duckfoot/issues.

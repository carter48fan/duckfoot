# lc-heif

Vendored, unmodified apart from its manifest, from LightCraft's `crates/heif`:

- Source: https://github.com/storytold/lightcraft, commit `b3d392b4994ee99dc389890f006600769d0c4ff5`
- Licence: MIT OR Apache-2.0 (used here under MIT; `LICENSE-MIT` is LightCraft's own)
- Copyright (c) 2026 ArtCraft Team and the LightCraft contributors

Changes: the package is renamed `lc-heif` and its manifest no longer inherits LightCraft's
workspace (versions, lints and the `wasm32` split are written out). In the source, only the
crate's own name changes where it names itself (`lc_heif::testdata`, `lc-heif`).

Duckfoot reaches it only through `photo-engine`'s optional `heif` feature. HEVC is covered by
patents in some jurisdictions; whether a build turns the feature on is a distribution decision.

To update: copy `crates/heif/src` and `README.md` from a newer LightCraft, bump the commit
above, and run `cargo test -p lc-heif`.

`clippy.toml` and `rustfmt.toml` are LightCraft's own, so its code is linted and formatted as it was
written (unwrap allowed in its tests, 150 columns) rather than rewritten to Duckfoot's style.

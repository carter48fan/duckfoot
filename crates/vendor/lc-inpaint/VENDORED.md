# lc-inpaint

Vendored, unmodified apart from its manifest, from LightCraft's `crates/inpaint`:

- Source: https://github.com/storytold/lightcraft, commit `b3d392b4994ee99dc389890f006600769d0c4ff5`
- Licence: MIT OR Apache-2.0 (used here under MIT; `LICENSE-MIT` is LightCraft's own). LightCraft
  ported it from PhotoCraft (`crates/algo/src/inpaint.rs`, `poisson.rs`), under the same licence.
- Copyright (c) 2026 ArtCraft Team and the LightCraft contributors

Changes: the package is renamed `lc-inpaint` and its manifest no longer inherits LightCraft's
workspace (versions written out; rayon unconditional, as Duckfoot has no wasm32 build). The
source is as it was. `clippy.toml` and `rustfmt.toml` are LightCraft's own, so the code is
linted and formatted as it was written.

Duckfoot uses it for the heal module's Remove spots (`photo-engine/src/engine.rs`, `fill_spot`).

To update: copy `crates/inpaint/src` from a newer LightCraft, bump the commit above, and run
`cargo test -p lc-inpaint`.

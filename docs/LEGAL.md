# Licence and legal notes

MIT — see [LICENSE](../LICENSE).

Duckfoot uses `rawler` and `rawloader` for RAW format decoding, which are licensed under the **GNU LGPL-2.1**. In compliance with LGPL-2.1 §6, complete corresponding source archives (`duckfoot-<version>-source.tar.gz`) are provided with every release to enable inspection, modification, and relinking.

The UI bundles three typefaces under the **SIL Open Font License 1.1** — JetBrains Mono, Space Grotesk and Hanken Grotesk (`crates/photo-app/assets/fonts/`). Lens correction data is adapted from the Lensfun database (**CC BY-SA 3.0**, `crates/photo-engine/data/LENSFUN-NOTICE.md`).

Duckfoot contains code adapted from [LightCraft](https://github.com/storytold/lightcraft) (MIT OR Apache-2.0, used under MIT): its XMP reader and merge, HEIF decoder and content-aware fill; see `THIRD_PARTY_LICENSES.md` and each `crates/vendor/*/VENDORED.md`.

### Legal notice: HEVC patents

**This is not legal advice.** If it matters to you, ask a lawyer who knows the countries
you work in.

Duckfoot's release builds (the AppImage, tarball, Windows, macOS and Android downloads)
include a software HEVC decoder (`heic-rs`, through `crates/vendor/lc-heif`), so that they
can open HEIF/HEIC photos. HEVC (H.265) is covered by patents held by many companies, in
many countries, and licensed among others through the Access Advance and Via LA patent
pools.

- **No patent licence comes with Duckfoot.** Its MIT licence, and its dependencies'
  open-source licences, grant rights in their code (copyright), not in anyone's patents.
  Duckfoot's authors hold no HEVC patents and cannot license them to you.
- **If you distribute Duckfoot with HEIF enabled, you may be obliged to obtain a patent
  licence, and obtaining it is your responsibility, not the authors'.** That applies most
  plainly if you ship it commercially, in volume, preinstalled on or bundled with a device,
  or as part of a product, in a country where the patents are in force. Whether the pools'
  terms reach you, and what they would cost, depends on what you ship, how many, and where.
- **Using it may be affected too** in some places.
- **To avoid HEVC entirely, build without it** (the default for a source build):
  `cargo build --release -p duckfoot`. Such a build refuses HEIF/HEIC files by name and
  contains no HEVC code. Redistributors who are unsure should ship that build.

The other formats Duckfoot reads and writes (JPEG, PNG, TIFF, WebP, AVIF, JPEG XL, the RAW
formats) are not known to need a patent licence for this use: AV1, which AVIF is built on,
is licensed royalty-free by the Alliance for Open Media.

Duckfoot previously contained code derived from [OpenCut](https://github.com/OpenCut-app/OpenCut) (MIT); that code is superseded, and their copyright notice is retained in git history.

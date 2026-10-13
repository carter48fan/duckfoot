# Third-Party Software Notices and Licenses

Duckfoot incorporates open-source software and font resources governed by the following licenses.

## Special Notice: GNU Lesser General Public License (LGPL-2.1)

Duckfoot links against the following libraries licensed under the GNU Lesser General Public License, Version 2.1:
- `rawler` (https://github.com/pedrocr/rawler)
- `rawloader` (https://github.com/pedrocr/rawloader)

In accordance with Section 6 of the GNU LGPL-2.1:
1. Complete Corresponding Source Code: The complete corresponding machine-readable source code
   for Duckfoot and all statically linked dependencies, including rawler and rawloader, is freely
   available with every release as `duckfoot-<version>-source.tar.gz` and hosted at:
   https://github.com/carter48fan/duckfoot
2. Relinking / Modification: Users have the right and ability to modify or substitute these
   libraries and relink or recompile the application using the provided source distribution.
3. No restriction is placed on reverse engineering for debugging modifications to the LGPL components.

## Bundled Typefaces (SIL Open Font License 1.1)

Duckfoot bundles the following open-source typefaces under the SIL Open Font License, Version 1.1:
- **JetBrains Mono**: Copyright (c) 2020 JetBrains s.r.o.
- **Space Grotesk**: Copyright (c) 2020 The Space Grotesk Project Authors
- **Hanken Grotesk**: Copyright (c) 2023 The Hanken Grotesk Project Authors

The complete license text for OFL-1.1 is included in the full license text section below and beside
the font files in `crates/photo-app/assets/fonts/`.

## Bundled Lens Calibration Data (CC BY-SA 3.0)

Duckfoot includes distilled lens and camera calibration tables derived from the Lensfun database:
- **Lensfun Database**: Copyright (c) The Lensfun Project Authors
- Licensed under the Creative Commons Attribution-ShareAlike 3.0 Unported License (CC BY-SA 3.0).
- Detailed notice: `crates/photo-engine/data/LENSFUN-NOTICE.md`.

## OpenCut Code Attribution (MIT)

Duckfoot contains code derived from OpenCut (https://github.com/OpenCut-app/OpenCut):
- Copyright (c) 2026 OpenCut
- Used under the terms of the MIT License.

## LightCraft Code Attribution (MIT)

Duckfoot contains code adapted from LightCraft (https://github.com/storytold/lightcraft): the XMP
reader and merge in `crates/photo-engine/src/xmp.rs`, and its HEIF decoder, vendored as
`crates/vendor/lc-heif` (see its VENDORED.md):
- Copyright (c) 2026 ArtCraft Team and the LightCraft contributors
- LightCraft is licensed MIT OR Apache-2.0; Duckfoot uses it under the terms of the MIT License.

## Rust Crate Dependencies Registry

| Package | Version | License | Repository / Homepage |
|---|---|---|---|
| `ab_glyph` | `0.2.32` | Apache-2.0 | [https://github.com/alexheretic/ab-glyph](https://github.com/alexheretic/ab-glyph) |
| `ab_glyph_rasterizer` | `0.1.10` | Apache-2.0 | [https://github.com/alexheretic/ab-glyph](https://github.com/alexheretic/ab-glyph) |
| `accesskit` | `0.24.1` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_atspi_common` | `0.18.1` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_consumer` | `0.35.0` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_consumer` | `0.36.0` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_consumer` | `0.38.0` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_macos` | `0.26.3` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_unix` | `0.21.1` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_windows` | `0.32.1` | MIT OR Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `accesskit_winit` | `0.32.2` | Apache-2.0 | [https://github.com/AccessKit/accesskit](https://github.com/AccessKit/accesskit) |
| `addr2line` | `0.25.1` | Apache-2.0 OR MIT | [https://github.com/gimli-rs/addr2line](https://github.com/gimli-rs/addr2line) |
| `adler2` | `2.0.1` | 0BSD OR MIT OR Apache-2.0 | [https://github.com/oyvindln/adler2](https://github.com/oyvindln/adler2) |
| `adler32` | `1.2.0` | Zlib | [https://github.com/remram44/adler32-rs](https://github.com/remram44/adler32-rs) |
| `ahash` | `0.8.12` | MIT OR Apache-2.0 | [https://github.com/tkaitchuck/ahash](https://github.com/tkaitchuck/ahash) |
| `aho-corasick` | `1.1.5` | Unlicense OR MIT | [https://github.com/BurntSushi/aho-corasick](https://github.com/BurntSushi/aho-corasick) |
| `aligned` | `0.4.3` | MIT OR Apache-2.0 | [https://github.com/rust-embedded-community/aligned](https://github.com/rust-embedded-community/aligned) |
| `aligned-vec` | `0.6.4` | MIT | [https://github.com/sarah-ek/aligned-vec/](https://github.com/sarah-ek/aligned-vec/) |
| `alloc-no-stdlib` | `2.0.4` | BSD-3-Clause | [https://github.com/dropbox/rust-alloc-no-stdlib](https://github.com/dropbox/rust-alloc-no-stdlib) |
| `alloc-stdlib` | `0.2.4` | BSD-3-Clause | [https://github.com/dropbox/rust-alloc-no-stdlib](https://github.com/dropbox/rust-alloc-no-stdlib) |
| `allocator-api2` | `0.2.21` | MIT OR Apache-2.0 | [https://github.com/zakarumych/allocator-api2](https://github.com/zakarumych/allocator-api2) |
| `android-activity` | `0.6.1` | MIT OR Apache-2.0 | [https://github.com/rust-mobile/android-activity](https://github.com/rust-mobile/android-activity) |
| `android-properties` | `0.2.2` | MIT | [https://github.com/miklelappo/android-properties](https://github.com/miklelappo/android-properties) |
| `android_log-sys` | `0.3.2` | MIT OR Apache-2.0 | [https://github.com/rust-mobile/android_log-sys-rs](https://github.com/rust-mobile/android_log-sys-rs) |
| `android_logger` | `0.15.1` | MIT OR Apache-2.0 | [https://github.com/rust-mobile/android_logger-rs](https://github.com/rust-mobile/android_logger-rs) |
| `android_system_properties` | `0.1.6` | MIT OR Apache-2.0 | [https://github.com/nical/android_system_properties](https://github.com/nical/android_system_properties) |
| `anstream` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/rust-cli/anstyle.git](https://github.com/rust-cli/anstyle.git) |
| `anstyle` | `1.0.14` | MIT OR Apache-2.0 | [https://github.com/rust-cli/anstyle.git](https://github.com/rust-cli/anstyle.git) |
| `anstyle-parse` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/rust-cli/anstyle.git](https://github.com/rust-cli/anstyle.git) |
| `anstyle-query` | `1.1.5` | MIT OR Apache-2.0 | [https://github.com/rust-cli/anstyle.git](https://github.com/rust-cli/anstyle.git) |
| `anstyle-wincon` | `3.0.11` | MIT OR Apache-2.0 | [https://github.com/rust-cli/anstyle.git](https://github.com/rust-cli/anstyle.git) |
| `anyhow` | `1.0.104` | MIT OR Apache-2.0 | [https://github.com/dtolnay/anyhow](https://github.com/dtolnay/anyhow) |
| `anymap3` | `1.1.0` | BlueOak-1.0.0 OR MIT OR Apache-2.0 | [https://github.com/reivilibre/anymap3](https://github.com/reivilibre/anymap3) |
| `arbitrary` | `1.5.0` | MIT OR Apache-2.0 | [https://github.com/rust-fuzz/arbitrary/](https://github.com/rust-fuzz/arbitrary/) |
| `arg_enum_proc_macro` | `0.3.4` | MIT | [https://github.com/lu-zero/arg_enum_proc_macro](https://github.com/lu-zero/arg_enum_proc_macro) |
| `arrayref` | `0.3.9` | BSD-2-Clause | [https://github.com/droundy/arrayref](https://github.com/droundy/arrayref) |
| `arrayvec` | `0.7.8` | MIT OR Apache-2.0 | [https://github.com/bluss/arrayvec](https://github.com/bluss/arrayvec) |
| `as-raw-xcb-connection` | `1.0.1` | MIT OR Apache-2.0 | [https://github.com/psychon/as-raw-xcb-connection](https://github.com/psychon/as-raw-xcb-connection) |
| `as-slice` | `0.2.1` | MIT OR Apache-2.0 | [https://github.com/japaric/as-slice](https://github.com/japaric/as-slice) |
| `ash` | `0.38.0+1.3.281` | MIT OR Apache-2.0 | [https://github.com/ash-rs/ash](https://github.com/ash-rs/ash) |
| `async-broadcast` | `0.7.2` | MIT OR Apache-2.0 | [https://github.com/smol-rs/async-broadcast](https://github.com/smol-rs/async-broadcast) |
| `async-channel` | `2.5.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-channel](https://github.com/smol-rs/async-channel) |
| `async-executor` | `1.14.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-executor](https://github.com/smol-rs/async-executor) |
| `async-io` | `2.6.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-io](https://github.com/smol-rs/async-io) |
| `async-lock` | `3.4.2` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-lock](https://github.com/smol-rs/async-lock) |
| `async-process` | `2.5.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-process](https://github.com/smol-rs/async-process) |
| `async-recursion` | `1.1.1` | MIT OR Apache-2.0 | [https://github.com/dcchut/async-recursion](https://github.com/dcchut/async-recursion) |
| `async-signal` | `0.2.14` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-signal](https://github.com/smol-rs/async-signal) |
| `async-task` | `4.7.1` | Apache-2.0 OR MIT | [https://github.com/smol-rs/async-task](https://github.com/smol-rs/async-task) |
| `async-trait` | `0.1.92` | MIT OR Apache-2.0 | [https://github.com/dtolnay/async-trait](https://github.com/dtolnay/async-trait) |
| `atomic-waker` | `1.1.2` | Apache-2.0 OR MIT | [https://github.com/smol-rs/atomic-waker](https://github.com/smol-rs/atomic-waker) |
| `atspi` | `0.29.0` | Apache-2.0 OR MIT | [https://github.com/odilia-app/atspi](https://github.com/odilia-app/atspi) |
| `atspi-common` | `0.13.0` | Apache-2.0 OR MIT | [https://github.com/odilia-app/atspi](https://github.com/odilia-app/atspi) |
| `atspi-proxies` | `0.13.0` | Apache-2.0 OR MIT | [https://github.com/odilia-app/atspi](https://github.com/odilia-app/atspi) |
| `autocfg` | `1.5.1` | Apache-2.0 OR MIT | [https://github.com/cuviper/autocfg](https://github.com/cuviper/autocfg) |
| `av-scenechange` | `0.14.1` | MIT | [https://github.com/rust-av/av-scenechange](https://github.com/rust-av/av-scenechange) |
| `av1-grain` | `0.2.5` | BSD-2-Clause | [https://github.com/rust-av/av1-grain](https://github.com/rust-av/av1-grain) |
| `avif-serialize` | `0.8.9` | BSD-3-Clause | [https://github.com/kornelski/avif-serialize](https://github.com/kornelski/avif-serialize) |
| `backtrace` | `0.3.76` | MIT OR Apache-2.0 | [https://github.com/rust-lang/backtrace-rs](https://github.com/rust-lang/backtrace-rs) |
| `bit-set` | `0.10.0` | Apache-2.0 OR MIT | [https://github.com/contain-rs/bit-set](https://github.com/contain-rs/bit-set) |
| `bit-set` | `0.11.1` | Apache-2.0 OR MIT | [https://github.com/contain-rs/bit-vec](https://github.com/contain-rs/bit-vec) |
| `bit-vec` | `0.9.1` | Apache-2.0 OR MIT | [https://github.com/contain-rs/bit-vec](https://github.com/contain-rs/bit-vec) |
| `bit-vec` | `0.10.1` | Apache-2.0 OR MIT | [https://github.com/contain-rs/bit-vec](https://github.com/contain-rs/bit-vec) |
| `bitflags` | `1.3.2` | MIT/Apache-2.0 | [https://github.com/bitflags/bitflags](https://github.com/bitflags/bitflags) |
| `bitflags` | `2.13.2` | MIT OR Apache-2.0 | [https://github.com/bitflags/bitflags](https://github.com/bitflags/bitflags) |
| `bitstream-io` | `4.10.0` | MIT/Apache-2.0 | [https://github.com/tuffy/bitstream-io](https://github.com/tuffy/bitstream-io) |
| `block2` | `0.5.1` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `block2` | `0.6.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `blocking` | `1.7.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/blocking](https://github.com/smol-rs/blocking) |
| `borsh` | `1.8.1` | MIT OR Apache-2.0 | [https://github.com/near/borsh-rs](https://github.com/near/borsh-rs) |
| `borsh-derive` | `1.8.1` | Apache-2.0 | [https://github.com/near/borsh-rs](https://github.com/near/borsh-rs) |
| `brotli-decompressor` | `5.0.3` | BSD-3-Clause/MIT | [https://github.com/dropbox/rust-brotli-decompressor](https://github.com/dropbox/rust-brotli-decompressor) |
| `built` | `0.8.1` | MIT | [https://github.com/lukaslueg/built](https://github.com/lukaslueg/built) |
| `bumpalo` | `3.20.3` | MIT OR Apache-2.0 | [https://github.com/fitzgen/bumpalo](https://github.com/fitzgen/bumpalo) |
| `bytemuck` | `1.25.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/Lokathor/bytemuck](https://github.com/Lokathor/bytemuck) |
| `bytemuck_derive` | `1.12.1` | Zlib OR Apache-2.0 OR MIT | [https://github.com/Lokathor/bytemuck](https://github.com/Lokathor/bytemuck) |
| `byteorder` | `1.5.0` | Unlicense OR MIT | [https://github.com/BurntSushi/byteorder](https://github.com/BurntSushi/byteorder) |
| `byteorder-lite` | `0.1.0` | Unlicense OR MIT | [https://github.com/image-rs/byteorder-lite](https://github.com/image-rs/byteorder-lite) |
| `bytes` | `1.12.1` | MIT | [https://github.com/tokio-rs/bytes](https://github.com/tokio-rs/bytes) |
| `calloop` | `0.13.0` | MIT | [https://github.com/Smithay/calloop](https://github.com/Smithay/calloop) |
| `calloop-wayland-source` | `0.3.0` | MIT | [https://github.com/smithay/calloop-wayland-source](https://github.com/smithay/calloop-wayland-source) |
| `cc` | `1.4.6` | MIT OR Apache-2.0 | [https://github.com/rust-lang/cc-rs](https://github.com/rust-lang/cc-rs) |
| `cesu8` | `1.1.0` | Apache-2.0/MIT | [https://github.com/emk/cesu8-rs](https://github.com/emk/cesu8-rs) |
| `cfg-if` | `1.0.5` | MIT OR Apache-2.0 | [https://github.com/rust-lang/cfg-if](https://github.com/rust-lang/cfg-if) |
| `cfg_aliases` | `0.2.2` | MIT | [https://github.com/katharostech/cfg_aliases](https://github.com/katharostech/cfg_aliases) |
| `chacha20` | `0.10.2` | MIT OR Apache-2.0 | [https://github.com/RustCrypto/stream-ciphers](https://github.com/RustCrypto/stream-ciphers) |
| `chrono` | `0.4.45` | MIT OR Apache-2.0 | [https://github.com/chronotope/chrono](https://github.com/chronotope/chrono) |
| `codespan-reporting` | `0.13.1` | Apache-2.0 | [https://github.com/brendanzab/codespan](https://github.com/brendanzab/codespan) |
| `color` | `0.3.3` | Apache-2.0 OR MIT | [https://github.com/linebender/color](https://github.com/linebender/color) |
| `colorchoice` | `1.0.5` | MIT OR Apache-2.0 | [https://github.com/rust-cli/anstyle.git](https://github.com/rust-cli/anstyle.git) |
| `combine` | `4.6.8` | MIT | [https://github.com/Marwes/combine](https://github.com/Marwes/combine) |
| `concurrent-queue` | `2.5.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/concurrent-queue](https://github.com/smol-rs/concurrent-queue) |
| `core-foundation` | `0.9.4` | MIT OR Apache-2.0 | [https://github.com/servo/core-foundation-rs](https://github.com/servo/core-foundation-rs) |
| `core-foundation-sys` | `0.8.7` | MIT OR Apache-2.0 | [https://github.com/servo/core-foundation-rs](https://github.com/servo/core-foundation-rs) |
| `core-graphics` | `0.23.2` | MIT OR Apache-2.0 | [https://github.com/servo/core-foundation-rs](https://github.com/servo/core-foundation-rs) |
| `core-graphics-types` | `0.1.3` | MIT OR Apache-2.0 | [https://github.com/servo/core-foundation-rs](https://github.com/servo/core-foundation-rs) |
| `cpufeatures` | `0.3.1` | MIT OR Apache-2.0 | [https://github.com/RustCrypto/utils](https://github.com/RustCrypto/utils) |
| `crc32fast` | `1.5.2` | MIT OR Apache-2.0 | [https://github.com/srijs/rust-crc32fast](https://github.com/srijs/rust-crc32fast) |
| `crossbeam-deque` | `0.8.8` | MIT OR Apache-2.0 | [https://github.com/crossbeam-rs/crossbeam](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-epoch` | `0.9.21` | MIT OR Apache-2.0 | [https://github.com/crossbeam-rs/crossbeam](https://github.com/crossbeam-rs/crossbeam) |
| `crossbeam-utils` | `0.8.23` | MIT OR Apache-2.0 | [https://github.com/crossbeam-rs/crossbeam](https://github.com/crossbeam-rs/crossbeam) |
| `crunchy` | `0.2.4` | MIT | [https://github.com/eira-fransham/crunchy](https://github.com/eira-fransham/crunchy) |
| `cursor-icon` | `1.2.0` | MIT OR Apache-2.0 OR Zlib | [https://github.com/rust-windowing/cursor-icon](https://github.com/rust-windowing/cursor-icon) |
| `dary_heap` | `0.3.9` | MIT OR Apache-2.0 | [https://github.com/hanmertens/dary_heap](https://github.com/hanmertens/dary_heap) |
| `defmt` | `1.1.1` | MIT OR Apache-2.0 | [https://github.com/knurling-rs/defmt](https://github.com/knurling-rs/defmt) |
| `defmt-macros` | `1.1.1` | MIT OR Apache-2.0 | [https://github.com/knurling-rs/defmt](https://github.com/knurling-rs/defmt) |
| `defmt-parser` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/knurling-rs/defmt](https://github.com/knurling-rs/defmt) |
| `derive-new` | `0.7.0` | MIT | [https://github.com/nrc/derive-new](https://github.com/nrc/derive-new) |
| `dispatch` | `0.2.0` | MIT | [http://github.com/SSheldon/rust-dispatch](http://github.com/SSheldon/rust-dispatch) |
| `dispatch2` | `0.3.1` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `displaydoc` | `0.2.7` | MIT OR Apache-2.0 | [https://github.com/yaahc/displaydoc](https://github.com/yaahc/displaydoc) |
| `dlib` | `0.5.3` | MIT | [https://github.com/elinorbgr/dlib](https://github.com/elinorbgr/dlib) |
| `document-features` | `0.2.12` | MIT OR Apache-2.0 | [https://github.com/slint-ui/document-features](https://github.com/slint-ui/document-features) |
| `downcast-rs` | `1.2.1` | MIT/Apache-2.0 | [https://github.com/marcianx/downcast-rs](https://github.com/marcianx/downcast-rs) |
| `downcast-rs` | `2.0.2` | MIT OR Apache-2.0 | [https://github.com/marcianx/downcast-rs](https://github.com/marcianx/downcast-rs) |
| `dpi` | `0.1.2` | Apache-2.0 AND MIT | [https://github.com/rust-windowing/winit](https://github.com/rust-windowing/winit) |
| `dyn-clone` | `1.0.20` | MIT OR Apache-2.0 | [https://github.com/dtolnay/dyn-clone](https://github.com/dtolnay/dyn-clone) |
| `dyn-hash` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/dtolnay/dyn-hash](https://github.com/dtolnay/dyn-hash) |
| `ecolor` | `0.36.2` | MIT OR Apache-2.0 | [https://github.com/emilk/egui](https://github.com/emilk/egui) |
| `egui` | `0.36.2` | MIT OR Apache-2.0 | [https://github.com/emilk/egui](https://github.com/emilk/egui) |
| `egui-wgpu` | `0.36.2` | MIT OR Apache-2.0 | [https://github.com/emilk/egui/tree/main/crates/egui-wgpu](https://github.com/emilk/egui/tree/main/crates/egui-wgpu) |
| `egui-winit` | `0.36.2` | MIT OR Apache-2.0 | [https://github.com/emilk/egui/tree/main/crates/egui-winit](https://github.com/emilk/egui/tree/main/crates/egui-winit) |
| `either` | `1.18.0` | MIT OR Apache-2.0 | [https://github.com/rayon-rs/either](https://github.com/rayon-rs/either) |
| `emath` | `0.36.2` | MIT OR Apache-2.0 | [https://github.com/emilk/egui/tree/main/crates/emath](https://github.com/emilk/egui/tree/main/crates/emath) |
| `endi` | `1.1.1` | MIT | [https://github.com/zeenix/endi](https://github.com/zeenix/endi) |
| `enumflags2` | `0.7.12` | MIT OR Apache-2.0 | [https://github.com/meithecatte/enumflags2](https://github.com/meithecatte/enumflags2) |
| `enumflags2_derive` | `0.7.12` | MIT OR Apache-2.0 | [https://github.com/meithecatte/enumflags2](https://github.com/meithecatte/enumflags2) |
| `enumn` | `0.1.14` | MIT OR Apache-2.0 | [https://github.com/dtolnay/enumn](https://github.com/dtolnay/enumn) |
| `env_filter` | `0.1.4` | MIT OR Apache-2.0 | [https://github.com/rust-cli/env_logger](https://github.com/rust-cli/env_logger) |
| `env_filter` | `2.0.0` | MIT OR Apache-2.0 | [https://github.com/rust-cli/env_logger](https://github.com/rust-cli/env_logger) |
| `env_logger` | `0.11.11` | MIT OR Apache-2.0 | [https://github.com/rust-cli/env_logger](https://github.com/rust-cli/env_logger) |
| `epaint` | `0.36.2` | MIT OR Apache-2.0 | [https://github.com/emilk/egui/tree/main/crates/epaint](https://github.com/emilk/egui/tree/main/crates/epaint) |
| `epaint_default_fonts` | `0.36.2` | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | [https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts](https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts) |
| `equator` | `0.4.2` | MIT | [https://github.com/sarah-ek/equator/](https://github.com/sarah-ek/equator/) |
| `equator-macro` | `0.4.2` | MIT | [https://github.com/sarah-ek/equator/](https://github.com/sarah-ek/equator/) |
| `equivalent` | `1.0.2` | Apache-2.0 OR MIT | [https://github.com/indexmap-rs/equivalent](https://github.com/indexmap-rs/equivalent) |
| `erased-serde` | `0.4.10` | MIT OR Apache-2.0 | [https://github.com/dtolnay/erased-serde](https://github.com/dtolnay/erased-serde) |
| `errno` | `0.3.14` | MIT OR Apache-2.0 | [https://github.com/lambda-fairy/rust-errno](https://github.com/lambda-fairy/rust-errno) |
| `euclid` | `0.22.14` | MIT OR Apache-2.0 | [https://github.com/servo/euclid](https://github.com/servo/euclid) |
| `event-listener` | `5.4.2` | Apache-2.0 OR MIT | [https://github.com/smol-rs/event-listener](https://github.com/smol-rs/event-listener) |
| `event-listener-strategy` | `0.5.4` | Apache-2.0 OR MIT | [https://github.com/smol-rs/event-listener-strategy](https://github.com/smol-rs/event-listener-strategy) |
| `fastrand` | `2.5.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/fastrand](https://github.com/smol-rs/fastrand) |
| `fax` | `0.2.7` | MIT | [https://github.com/pdf-rs/fax](https://github.com/pdf-rs/fax) |
| `fdeflate` | `0.3.7` | MIT OR Apache-2.0 | [https://github.com/image-rs/fdeflate](https://github.com/image-rs/fdeflate) |
| `fearless_simd` | `0.4.1` | Apache-2.0 OR MIT | [https://github.com/linebender/fearless_simd](https://github.com/linebender/fearless_simd) |
| `filetime` | `0.2.29` | MIT/Apache-2.0 | [https://github.com/alexcrichton/filetime](https://github.com/alexcrichton/filetime) |
| `find-msvc-tools` | `0.1.12` | MIT OR Apache-2.0 | [https://github.com/rust-lang/cc-rs](https://github.com/rust-lang/cc-rs) |
| `flate2` | `1.1.10` | MIT OR Apache-2.0 | [https://github.com/rust-lang/flate2-rs](https://github.com/rust-lang/flate2-rs) |
| `float-ord` | `0.3.2` | MIT / Apache-2.0 | [https://github.com/notriddle/rust-float-ord](https://github.com/notriddle/rust-float-ord) |
| `fluent-bundle` | `0.16.0` | Apache-2.0 OR MIT | [https://github.com/projectfluent/fluent-rs](https://github.com/projectfluent/fluent-rs) |
| `fluent-langneg` | `0.13.1` | Apache-2.0 OR MIT | [https://github.com/projectfluent/fluent-langneg-rs](https://github.com/projectfluent/fluent-langneg-rs) |
| `fluent-syntax` | `0.12.0` | Apache-2.0 OR MIT | [https://github.com/projectfluent/fluent-rs](https://github.com/projectfluent/fluent-rs) |
| `foldhash` | `0.2.0` | Zlib | [https://github.com/orlp/foldhash](https://github.com/orlp/foldhash) |
| `font-types` | `0.12.5` | MIT OR Apache-2.0 | [https://github.com/googlefonts/fontations](https://github.com/googlefonts/fontations) |
| `foreign-types` | `0.5.0` | MIT/Apache-2.0 | [https://github.com/sfackler/foreign-types](https://github.com/sfackler/foreign-types) |
| `foreign-types-macros` | `0.2.4` | MIT/Apache-2.0 | [https://github.com/sfackler/foreign-types](https://github.com/sfackler/foreign-types) |
| `foreign-types-shared` | `0.3.1` | MIT/Apache-2.0 | [https://github.com/sfackler/foreign-types](https://github.com/sfackler/foreign-types) |
| `futures-core` | `0.3.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/futures-rs](https://github.com/rust-lang/futures-rs) |
| `futures-io` | `0.3.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/futures-rs](https://github.com/rust-lang/futures-rs) |
| `futures-lite` | `2.6.1` | Apache-2.0 OR MIT | [https://github.com/smol-rs/futures-lite](https://github.com/smol-rs/futures-lite) |
| `futures-macro` | `0.3.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/futures-rs](https://github.com/rust-lang/futures-rs) |
| `futures-task` | `0.3.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/futures-rs](https://github.com/rust-lang/futures-rs) |
| `futures-util` | `0.3.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/futures-rs](https://github.com/rust-lang/futures-rs) |
| `gethostname` | `1.1.0` | Apache-2.0 | [https://codeberg.org/swsnr/gethostname.rs.git](https://codeberg.org/swsnr/gethostname.rs.git) |
| `getrandom` | `0.3.4` | MIT OR Apache-2.0 | [https://github.com/rust-random/getrandom](https://github.com/rust-random/getrandom) |
| `getrandom` | `0.4.3` | MIT OR Apache-2.0 | [https://github.com/rust-random/getrandom](https://github.com/rust-random/getrandom) |
| `gimli` | `0.32.3` | MIT OR Apache-2.0 | [https://github.com/gimli-rs/gimli](https://github.com/gimli-rs/gimli) |
| `gl_generator` | `0.14.0` | Apache-2.0 | [https://github.com/brendanzab/gl-rs/](https://github.com/brendanzab/gl-rs/) |
| `glifo` | `0.2.0` | Apache-2.0 OR MIT | [https://github.com/linebender/vello](https://github.com/linebender/vello) |
| `glob` | `0.3.4` | MIT OR Apache-2.0 | [https://github.com/rust-lang/glob](https://github.com/rust-lang/glob) |
| `glow` | `0.17.0` | MIT OR Apache-2.0 OR Zlib | [https://github.com/grovesNL/glow](https://github.com/grovesNL/glow) |
| `glutin_wgl_sys` | `0.6.1` | Apache-2.0 | [https://github.com/rust-windowing/glutin](https://github.com/rust-windowing/glutin) |
| `gpu-allocator` | `0.28.0` | MIT OR Apache-2.0 | [https://github.com/Traverse-Research/gpu-allocator](https://github.com/Traverse-Research/gpu-allocator) |
| `guillotiere` | `0.7.0` | MIT/Apache-2.0 | [https://github.com/nical/guillotiere](https://github.com/nical/guillotiere) |
| `half` | `2.7.1` | MIT OR Apache-2.0 | [https://github.com/VoidStarKat/half-rs](https://github.com/VoidStarKat/half-rs) |
| `harfrust` | `0.12.0` | MIT | [https://github.com/harfbuzz/harfrust](https://github.com/harfbuzz/harfrust) |
| `hashbrown` | `0.16.1` | MIT OR Apache-2.0 | [https://github.com/rust-lang/hashbrown](https://github.com/rust-lang/hashbrown) |
| `hashbrown` | `0.17.1` | MIT OR Apache-2.0 | [https://github.com/rust-lang/hashbrown](https://github.com/rust-lang/hashbrown) |
| `heic-rs` | `0.1.4` | MIT OR Apache-2.0 | [https://github.com/tbraun96/heic-rs](https://github.com/tbraun96/heic-rs) |
| `hermit-abi` | `0.5.3` | MIT OR Apache-2.0 | [https://github.com/hermit-os/hermit-rs](https://github.com/hermit-os/hermit-rs) |
| `hex` | `0.4.3` | MIT OR Apache-2.0 | [https://github.com/KokaKiwi/rust-hex](https://github.com/KokaKiwi/rust-hex) |
| `iana-time-zone` | `0.1.65` | MIT OR Apache-2.0 | [https://github.com/strawlab/iana-time-zone](https://github.com/strawlab/iana-time-zone) |
| `iana-time-zone-haiku` | `0.1.2` | MIT OR Apache-2.0 | [https://github.com/strawlab/iana-time-zone](https://github.com/strawlab/iana-time-zone) |
| `image` | `0.25.10` | MIT OR Apache-2.0 | [https://github.com/image-rs/image](https://github.com/image-rs/image) |
| `image-webp` | `0.2.4` | MIT OR Apache-2.0 | [https://github.com/image-rs/image-webp](https://github.com/image-rs/image-webp) |
| `imgref` | `1.12.3` | CC0-1.0 OR Apache-2.0 | [https://github.com/kornelski/imgref](https://github.com/kornelski/imgref) |
| `indexmap` | `2.14.2` | Apache-2.0 OR MIT | [https://github.com/indexmap-rs/indexmap](https://github.com/indexmap-rs/indexmap) |
| `interpolate_name` | `0.2.4` | MIT | [https://github.com/lu-zero/interpolate_name](https://github.com/lu-zero/interpolate_name) |
| `intl-memoizer` | `0.5.3` | Apache-2.0 OR MIT | [https://github.com/projectfluent/fluent-rs](https://github.com/projectfluent/fluent-rs) |
| `intl_pluralrules` | `7.0.2` | Apache-2.0/MIT | [https://github.com/zbraniecki/pluralrules](https://github.com/zbraniecki/pluralrules) |
| `inventory` | `0.3.24` | MIT OR Apache-2.0 | [https://github.com/dtolnay/inventory](https://github.com/dtolnay/inventory) |
| `is_terminal_polyfill` | `1.70.2` | MIT OR Apache-2.0 | [https://github.com/polyfill-rs/is_terminal_polyfill](https://github.com/polyfill-rs/is_terminal_polyfill) |
| `itertools` | `0.14.0` | MIT OR Apache-2.0 | [https://github.com/rust-itertools/itertools](https://github.com/rust-itertools/itertools) |
| `itertools` | `0.15.0` | MIT OR Apache-2.0 | [https://github.com/rust-itertools/itertools](https://github.com/rust-itertools/itertools) |
| `itoa` | `1.0.18` | MIT OR Apache-2.0 | [https://github.com/dtolnay/itoa](https://github.com/dtolnay/itoa) |
| `jiff` | `0.2.37` | Unlicense OR MIT | [https://github.com/BurntSushi/jiff](https://github.com/BurntSushi/jiff) |
| `jiff-core` | `0.1.1` | Unlicense OR MIT | [https://github.com/BurntSushi/jiff](https://github.com/BurntSushi/jiff) |
| `jiff-static` | `0.2.37` | Unlicense OR MIT | [https://github.com/BurntSushi/jiff](https://github.com/BurntSushi/jiff) |
| `jixel` | `0.3.4` | BSD-3-Clause OR Apache-2.0 | [https://github.com/awxkee/jixel](https://github.com/awxkee/jixel) |
| `jni` | `0.21.1` | MIT/Apache-2.0 | [https://github.com/jni-rs/jni-rs](https://github.com/jni-rs/jni-rs) |
| `jni` | `0.22.4` | MIT OR Apache-2.0 | [https://github.com/jni-rs/jni-rs](https://github.com/jni-rs/jni-rs) |
| `jni-macros` | `0.22.4` | MIT OR Apache-2.0 | [https://github.com/jni-rs/jni-rs](https://github.com/jni-rs/jni-rs) |
| `jni-sys` | `0.3.1` | MIT OR Apache-2.0 | [https://github.com/jni-rs/jni-sys](https://github.com/jni-rs/jni-sys) |
| `jni-sys` | `0.4.1` | MIT OR Apache-2.0 | [https://github.com/jni-rs/jni-sys](https://github.com/jni-rs/jni-sys) |
| `jni-sys-macros` | `0.4.1` | MIT OR Apache-2.0 | [https://github.com/jni-rs/jni-sys](https://github.com/jni-rs/jni-sys) |
| `jobserver` | `0.1.35` | MIT OR Apache-2.0 | [https://github.com/rust-lang/jobserver-rs](https://github.com/rust-lang/jobserver-rs) |
| `jpeg-decoder` | `0.3.2` | MIT OR Apache-2.0 | [https://github.com/image-rs/jpeg-decoder](https://github.com/image-rs/jpeg-decoder) |
| `js-sys` | `0.3.105` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) |
| `jxl-bitstream` | `1.1.0` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-coding` | `1.0.1` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-color` | `0.11.0` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-frame` | `0.13.3` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-grid` | `0.6.2` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-image` | `0.13.0` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-jbr` | `0.2.1` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-modular` | `0.11.3` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-oxide` | `0.12.6` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-oxide-common` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-render` | `0.12.4` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-threadpool` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `jxl-vardct` | `0.11.1` | MIT OR Apache-2.0 | [https://github.com/tirr-c/jxl-oxide.git](https://github.com/tirr-c/jxl-oxide.git) |
| `kamadak-exif` | `0.6.1` | BSD-2-Clause | [https://github.com/kamadak/exif-rs](https://github.com/kamadak/exif-rs) |
| `khronos-egl` | `6.0.0` | MIT/Apache-2.0 | [https://github.com/timothee-haudebourg/khronos-egl](https://github.com/timothee-haudebourg/khronos-egl) |
| `khronos_api` | `3.1.0` | Apache-2.0 | [https://github.com/brendanzab/gl-rs/](https://github.com/brendanzab/gl-rs/) |
| `kurbo` | `0.13.1` | Apache-2.0 OR MIT | [https://github.com/linebender/kurbo](https://github.com/linebender/kurbo) |
| `lazy_static` | `1.5.0` | MIT OR Apache-2.0 | [https://github.com/rust-lang-nursery/lazy-static.rs](https://github.com/rust-lang-nursery/lazy-static.rs) |
| `libc` | `0.2.189` | MIT OR Apache-2.0 | [https://github.com/rust-lang/libc](https://github.com/rust-lang/libc) |
| `libflate` | `2.3.2` | MIT | [https://github.com/sile/libflate](https://github.com/sile/libflate) |
| `libflate_lz77` | `2.3.0` | MIT | [https://github.com/sile/libflate](https://github.com/sile/libflate) |
| `libfuzzer-sys` | `0.4.13` | (MIT OR Apache-2.0) AND NCSA | [https://github.com/rust-fuzz/libfuzzer](https://github.com/rust-fuzz/libfuzzer) |
| `libloading` | `0.8.9` | ISC | [https://github.com/nagisa/rust_libloading/](https://github.com/nagisa/rust_libloading/) |
| `libm` | `0.2.16` | MIT | [https://github.com/rust-lang/compiler-builtins](https://github.com/rust-lang/compiler-builtins) |
| `libredox` | `0.1.24` | MIT | [https://gitlab.redox-os.org/redox-os/libredox.git](https://gitlab.redox-os.org/redox-os/libredox.git) |
| `libwebp-sys` | `0.9.6` | MIT | [https://github.com/NoXF/libwebp-sys](https://github.com/NoXF/libwebp-sys) |
| `linebender_resource_handle` | `0.1.1` | Apache-2.0 OR MIT | [https://github.com/linebender/raw_resource_handle](https://github.com/linebender/raw_resource_handle) |
| `linux-raw-sys` | `0.4.15` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [https://github.com/sunfishcode/linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) |
| `linux-raw-sys` | `0.12.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [https://github.com/sunfishcode/linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) |
| `litrs` | `1.0.0` | MIT OR Apache-2.0 | [https://github.com/LukasKalbertodt/litrs](https://github.com/LukasKalbertodt/litrs) |
| `lock_api` | `0.4.14` | MIT OR Apache-2.0 | [https://github.com/Amanieu/parking_lot](https://github.com/Amanieu/parking_lot) |
| `log` | `0.4.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/log](https://github.com/rust-lang/log) |
| `loop9` | `0.1.5` | MIT | [https://gitlab.com/kornelski/loop9.git](https://gitlab.com/kornelski/loop9.git) |
| `maplit` | `1.0.2` | MIT/Apache-2.0 | [https://github.com/bluss/maplit](https://github.com/bluss/maplit) |
| `matrixmultiply` | `0.3.11` | MIT/Apache-2.0 | [https://github.com/bluss/matrixmultiply/](https://github.com/bluss/matrixmultiply/) |
| `maybe-rayon` | `0.1.1` | MIT | [https://github.com/shssoichiro/maybe-rayon](https://github.com/shssoichiro/maybe-rayon) |
| `md5` | `0.8.1` | Apache-2.0 OR MIT | [https://github.com/stainless-steel/md5](https://github.com/stainless-steel/md5) |
| `memchr` | `2.8.3` | Unlicense OR MIT | [https://github.com/BurntSushi/memchr](https://github.com/BurntSushi/memchr) |
| `memmap2` | `0.9.11` | MIT OR Apache-2.0 | [https://github.com/RazrFalcon/memmap2-rs](https://github.com/RazrFalcon/memmap2-rs) |
| `memo-map` | `0.3.4` | Apache-2.0 | [https://github.com/mitsuhiko/memo-map](https://github.com/mitsuhiko/memo-map) |
| `memoffset` | `0.9.1` | MIT | [https://github.com/Gilnaa/memoffset](https://github.com/Gilnaa/memoffset) |
| `minijinja` | `2.24.0` | Apache-2.0 | [https://github.com/mitsuhiko/minijinja](https://github.com/mitsuhiko/minijinja) |
| `miniz_oxide` | `0.8.9` | MIT OR Zlib OR Apache-2.0 | [https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) |
| `miniz_oxide` | `0.9.1` | MIT OR Zlib OR Apache-2.0 | [https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) |
| `moxcms` | `0.8.1` | BSD-3-Clause OR Apache-2.0 | [https://github.com/awxkee/moxcms.git](https://github.com/awxkee/moxcms.git) |
| `multiversion` | `0.8.0` | MIT OR Apache-2.0 | [https://github.com/calebzulawski/multiversion](https://github.com/calebzulawski/multiversion) |
| `multiversion-macros` | `0.8.0` | MIT OR Apache-2.0 | [https://github.com/calebzulawski/multiversion](https://github.com/calebzulawski/multiversion) |
| `mutate_once` | `0.1.2` | BSD-2-Clause | [https://github.com/kamadak/mutate_once-rs](https://github.com/kamadak/mutate_once-rs) |
| `naga` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `naga-types` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `ndarray` | `0.17.2` | MIT OR Apache-2.0 | [https://github.com/rust-ndarray/ndarray](https://github.com/rust-ndarray/ndarray) |
| `ndk` | `0.9.0` | MIT OR Apache-2.0 | [https://github.com/rust-mobile/ndk](https://github.com/rust-mobile/ndk) |
| `ndk-context` | `0.1.1` | MIT OR Apache-2.0 | [https://github.com/rust-windowing/android-ndk-rs](https://github.com/rust-windowing/android-ndk-rs) |
| `ndk-sys` | `0.6.0+11769913` | MIT OR Apache-2.0 | [https://github.com/rust-mobile/ndk](https://github.com/rust-mobile/ndk) |
| `new_debug_unreachable` | `1.0.6` | MIT | [https://github.com/mbrubeck/rust-debug-unreachable](https://github.com/mbrubeck/rust-debug-unreachable) |
| `no_std_io2` | `0.9.4` | Apache-2.0 OR MIT | [https://github.com/wcampbell0x2a/no-std-io2](https://github.com/wcampbell0x2a/no-std-io2) |
| `nohash-hasher` | `0.2.0` | Apache-2.0 OR MIT | [https://github.com/paritytech/nohash-hasher](https://github.com/paritytech/nohash-hasher) |
| `nom` | `8.0.0` | MIT | [https://github.com/rust-bakery/nom](https://github.com/rust-bakery/nom) |
| `nom-language` | `0.1.0` | MIT | [https://github.com/rust-bakery/nom](https://github.com/rust-bakery/nom) |
| `noop_proc_macro` | `0.3.0` | MIT | [https://github.com/lu-zero/noop_proc_macro](https://github.com/lu-zero/noop_proc_macro) |
| `num` | `0.4.3` | MIT OR Apache-2.0 | [https://github.com/rust-num/num](https://github.com/rust-num/num) |
| `num-bigint` | `0.4.8` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-bigint](https://github.com/rust-num/num-bigint) |
| `num-complex` | `0.4.6` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-complex](https://github.com/rust-num/num-complex) |
| `num-derive` | `0.4.2` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-derive](https://github.com/rust-num/num-derive) |
| `num-integer` | `0.1.47` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-integer](https://github.com/rust-num/num-integer) |
| `num-iter` | `0.1.46` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-iter](https://github.com/rust-num/num-iter) |
| `num-rational` | `0.4.2` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-rational](https://github.com/rust-num/num-rational) |
| `num-traits` | `0.2.19` | MIT OR Apache-2.0 | [https://github.com/rust-num/num-traits](https://github.com/rust-num/num-traits) |
| `num_enum` | `0.7.6` | BSD-3-Clause OR MIT OR Apache-2.0 | [https://github.com/illicitonion/num_enum](https://github.com/illicitonion/num_enum) |
| `num_enum_derive` | `0.7.6` | BSD-3-Clause OR MIT OR Apache-2.0 | [https://github.com/illicitonion/num_enum](https://github.com/illicitonion/num_enum) |
| `objc-sys` | `0.3.5` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2` | `0.5.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2` | `0.6.4` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-app-kit` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-app-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-cloud-kit` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-contacts` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-core-data` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-core-foundation` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-core-graphics` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-core-image` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-core-location` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-encode` | `4.1.0` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-foundation` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-foundation` | `0.3.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-io-surface` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-link-presentation` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-metal` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-metal` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-quartz-core` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-quartz-core` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-symbols` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-ui-kit` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-ui-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-uniform-type-identifiers` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `objc2-user-notifications` | `0.2.2` | MIT | [https://github.com/madsmtm/objc2](https://github.com/madsmtm/objc2) |
| `object` | `0.37.3` | Apache-2.0 OR MIT | [https://github.com/gimli-rs/object](https://github.com/gimli-rs/object) |
| `once_cell` | `1.21.4` | MIT OR Apache-2.0 | [https://github.com/matklad/once_cell](https://github.com/matklad/once_cell) |
| `once_cell_polyfill` | `1.70.2` | MIT OR Apache-2.0 | [https://github.com/polyfill-rs/once_cell_polyfill](https://github.com/polyfill-rs/once_cell_polyfill) |
| `orbclient` | `0.3.55` | MIT | [https://gitlab.redox-os.org/redox-os/orbclient](https://gitlab.redox-os.org/redox-os/orbclient) |
| `ordered-float` | `5.5.0` | MIT | [https://github.com/reem/rust-ordered-float](https://github.com/reem/rust-ordered-float) |
| `ordered-stream` | `0.2.0` | MIT OR Apache-2.0 | [https://github.com/danieldg/ordered-stream](https://github.com/danieldg/ordered-stream) |
| `owned_ttf_parser` | `0.25.1` | Apache-2.0 | [https://github.com/alexheretic/owned-ttf-parser](https://github.com/alexheretic/owned-ttf-parser) |
| `parking` | `2.2.1` | Apache-2.0 OR MIT | [https://github.com/smol-rs/parking](https://github.com/smol-rs/parking) |
| `parking_lot` | `0.12.5` | MIT OR Apache-2.0 | [https://github.com/Amanieu/parking_lot](https://github.com/Amanieu/parking_lot) |
| `parking_lot_core` | `0.9.12` | MIT OR Apache-2.0 | [https://github.com/Amanieu/parking_lot](https://github.com/Amanieu/parking_lot) |
| `paste` | `1.0.15` | MIT OR Apache-2.0 | [https://github.com/dtolnay/paste](https://github.com/dtolnay/paste) |
| `pastey` | `0.1.1` | MIT OR Apache-2.0 | [https://github.com/as1100k/pastey](https://github.com/as1100k/pastey) |
| `pastey` | `0.2.3` | MIT OR Apache-2.0 | [https://github.com/as1100k/pastey](https://github.com/as1100k/pastey) |
| `peniko` | `0.6.1` | Apache-2.0 OR MIT | [https://github.com/linebender/peniko](https://github.com/linebender/peniko) |
| `percent-encoding` | `2.3.2` | MIT OR Apache-2.0 | [https://github.com/servo/rust-url/](https://github.com/servo/rust-url/) |
| `phf` | `0.13.1` | MIT | [https://github.com/rust-phf/rust-phf](https://github.com/rust-phf/rust-phf) |
| `phf_generator` | `0.13.1` | MIT | [https://github.com/rust-phf/rust-phf](https://github.com/rust-phf/rust-phf) |
| `phf_macros` | `0.13.1` | MIT | [https://github.com/rust-phf/rust-phf](https://github.com/rust-phf/rust-phf) |
| `phf_shared` | `0.13.1` | MIT | [https://github.com/rust-phf/rust-phf](https://github.com/rust-phf/rust-phf) |
| `pin-project` | `1.1.13` | Apache-2.0 OR MIT | [https://github.com/taiki-e/pin-project](https://github.com/taiki-e/pin-project) |
| `pin-project-internal` | `1.1.13` | Apache-2.0 OR MIT | [https://github.com/taiki-e/pin-project](https://github.com/taiki-e/pin-project) |
| `pin-project-lite` | `0.2.17` | Apache-2.0 OR MIT | [https://github.com/taiki-e/pin-project-lite](https://github.com/taiki-e/pin-project-lite) |
| `piper` | `0.2.5` | MIT OR Apache-2.0 | [https://github.com/smol-rs/piper](https://github.com/smol-rs/piper) |
| `pkg-config` | `0.3.34` | MIT OR Apache-2.0 | [https://github.com/rust-lang/pkg-config-rs](https://github.com/rust-lang/pkg-config-rs) |
| `plain` | `0.2.3` | MIT/Apache-2.0 | [https://github.com/randomites/plain](https://github.com/randomites/plain) |
| `png` | `0.18.1` | MIT OR Apache-2.0 | [https://github.com/image-rs/image-png](https://github.com/image-rs/image-png) |
| `polling` | `3.11.0` | Apache-2.0 OR MIT | [https://github.com/smol-rs/polling](https://github.com/smol-rs/polling) |
| `pollster` | `0.4.0` | Apache-2.0/MIT | [https://github.com/zesterer/pollster](https://github.com/zesterer/pollster) |
| `pollster` | `1.0.1` | Apache-2.0/MIT | [https://github.com/zesterer/pollster](https://github.com/zesterer/pollster) |
| `polycool` | `0.4.0` | MIT OR Apache-2.0 | [https://github.com/linebender/kurbo](https://github.com/linebender/kurbo) |
| `portable-atomic` | `1.15.0` | Apache-2.0 OR MIT | [https://github.com/taiki-e/portable-atomic](https://github.com/taiki-e/portable-atomic) |
| `portable-atomic-util` | `0.2.8` | Apache-2.0 OR MIT | [https://github.com/taiki-e/portable-atomic-util](https://github.com/taiki-e/portable-atomic-util) |
| `ppv-lite86` | `0.2.21` | MIT OR Apache-2.0 | [https://github.com/cryptocorrosion/cryptocorrosion](https://github.com/cryptocorrosion/cryptocorrosion) |
| `presser` | `0.3.1` | MIT OR Apache-2.0 | [https://github.com/EmbarkStudios/presser](https://github.com/EmbarkStudios/presser) |
| `primal-check` | `0.3.4` | MIT OR Apache-2.0 | [https://github.com/huonw/primal](https://github.com/huonw/primal) |
| `proc-macro-crate` | `3.5.0` | MIT OR Apache-2.0 | [https://github.com/bkchr/proc-macro-crate](https://github.com/bkchr/proc-macro-crate) |
| `proc-macro2` | `1.0.107` | MIT OR Apache-2.0 | [https://github.com/dtolnay/proc-macro2](https://github.com/dtolnay/proc-macro2) |
| `profiling` | `1.0.18` | MIT OR Apache-2.0 | [https://github.com/aclysma/profiling](https://github.com/aclysma/profiling) |
| `profiling-procmacros` | `1.0.18` | MIT OR Apache-2.0 | [https://github.com/aclysma/profiling](https://github.com/aclysma/profiling) |
| `prost` | `0.14.4` | Apache-2.0 | [https://github.com/tokio-rs/prost](https://github.com/tokio-rs/prost) |
| `prost-derive` | `0.14.4` | Apache-2.0 | [https://github.com/tokio-rs/prost](https://github.com/tokio-rs/prost) |
| `pxfm` | `0.1.30` | BSD-3-Clause OR Apache-2.0 | [https://github.com/awxkee/pxfm](https://github.com/awxkee/pxfm) |
| `quick-error` | `2.0.1` | MIT/Apache-2.0 | [http://github.com/tailhook/quick-error](http://github.com/tailhook/quick-error) |
| `quick-xml` | `0.41.0` | MIT | [https://github.com/tafia/quick-xml](https://github.com/tafia/quick-xml) |
| `quote` | `1.0.47` | MIT OR Apache-2.0 | [https://github.com/dtolnay/quote](https://github.com/dtolnay/quote) |
| `r-efi` | `5.3.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | [https://github.com/r-efi/r-efi](https://github.com/r-efi/r-efi) |
| `r-efi` | `6.0.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | [https://github.com/r-efi/r-efi](https://github.com/r-efi/r-efi) |
| `rand` | `0.9.5` | MIT OR Apache-2.0 | [https://github.com/rust-random/rand](https://github.com/rust-random/rand) |
| `rand` | `0.10.3` | MIT OR Apache-2.0 | [https://github.com/rust-random/rand](https://github.com/rust-random/rand) |
| `rand_chacha` | `0.9.0` | MIT OR Apache-2.0 | [https://github.com/rust-random/rand](https://github.com/rust-random/rand) |
| `rand_core` | `0.9.5` | MIT OR Apache-2.0 | [https://github.com/rust-random/rand](https://github.com/rust-random/rand) |
| `rand_core` | `0.10.1` | MIT OR Apache-2.0 | [https://github.com/rust-random/rand_core](https://github.com/rust-random/rand_core) |
| `rand_distr` | `0.6.0` | MIT OR Apache-2.0 | [https://github.com/rust-random/rand_distr](https://github.com/rust-random/rand_distr) |
| `range-alloc` | `0.1.5` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/range-alloc](https://github.com/gfx-rs/range-alloc) |
| `rav1e` | `0.8.1` | BSD-2-Clause | [https://github.com/xiph/rav1e/](https://github.com/xiph/rav1e/) |
| `ravif` | `0.13.0` | BSD-3-Clause | [https://github.com/kornelski/cavif-rs](https://github.com/kornelski/cavif-rs) |
| `raw-window-handle` | `0.6.2` | MIT OR Apache-2.0 OR Zlib | [https://github.com/rust-windowing/raw-window-handle](https://github.com/rust-windowing/raw-window-handle) |
| `raw-window-metal` | `1.1.0` | MIT OR Apache-2.0 | [https://github.com/rust-windowing/raw-window-metal](https://github.com/rust-windowing/raw-window-metal) |
| `rawler` | `0.8.0` | LGPL-2.1 | [https://github.com/dnglab/dnglab](https://github.com/dnglab/dnglab) |
| `rawloader` | `0.37.2` | LGPL-2.1 | [https://github.com/pedrocr/rawloader](https://github.com/pedrocr/rawloader) |
| `rawpointer` | `0.2.1` | MIT/Apache-2.0 | [https://github.com/bluss/rawpointer/](https://github.com/bluss/rawpointer/) |
| `rayon` | `1.12.0` | MIT OR Apache-2.0 | [https://github.com/rayon-rs/rayon](https://github.com/rayon-rs/rayon) |
| `rayon-core` | `1.13.0` | MIT OR Apache-2.0 | [https://github.com/rayon-rs/rayon](https://github.com/rayon-rs/rayon) |
| `read-fonts` | `0.41.0` | MIT OR Apache-2.0 | [https://github.com/googlefonts/fontations](https://github.com/googlefonts/fontations) |
| `redox_syscall` | `0.4.1` | MIT | [https://gitlab.redox-os.org/redox-os/syscall](https://gitlab.redox-os.org/redox-os/syscall) |
| `redox_syscall` | `0.5.18` | MIT | [https://gitlab.redox-os.org/redox-os/syscall](https://gitlab.redox-os.org/redox-os/syscall) |
| `redox_syscall` | `0.9.4` | MIT | [https://gitlab.redox-os.org/redox-os/kernel](https://gitlab.redox-os.org/redox-os/kernel) |
| `regex` | `1.13.1` | MIT OR Apache-2.0 | [https://github.com/rust-lang/regex](https://github.com/rust-lang/regex) |
| `regex-automata` | `0.4.18` | MIT OR Apache-2.0 | [https://github.com/rust-lang/regex](https://github.com/rust-lang/regex) |
| `regex-syntax` | `0.8.11` | MIT OR Apache-2.0 | [https://github.com/rust-lang/regex](https://github.com/rust-lang/regex) |
| `renderdoc-sys` | `1.1.0` | MIT OR Apache-2.0 | [https://github.com/ebkalderon/renderdoc-rs](https://github.com/ebkalderon/renderdoc-rs) |
| `rfd` | `0.17.2` | MIT | [https://github.com/PolyMeilex/rfd](https://github.com/PolyMeilex/rfd) |
| `rgb` | `0.8.53` | MIT | [https://github.com/kornelski/rust-rgb](https://github.com/kornelski/rust-rgb) |
| `rle-decode-fast` | `1.0.3` | MIT OR Apache-2.0 | [https://github.com/WanzenBug/rle-decode-helper](https://github.com/WanzenBug/rle-decode-helper) |
| `rustc-demangle` | `0.1.28` | MIT/Apache-2.0 | [https://github.com/rust-lang/rustc-demangle](https://github.com/rust-lang/rustc-demangle) |
| `rustc-hash` | `1.1.0` | Apache-2.0/MIT | [https://github.com/rust-lang-nursery/rustc-hash](https://github.com/rust-lang-nursery/rustc-hash) |
| `rustc-hash` | `2.1.3` | Apache-2.0 OR MIT | [https://github.com/rust-lang/rustc-hash](https://github.com/rust-lang/rustc-hash) |
| `rustc_version` | `0.4.1` | MIT OR Apache-2.0 | [https://github.com/djc/rustc-version-rs](https://github.com/djc/rustc-version-rs) |
| `rustfft` | `6.4.1` | MIT OR Apache-2.0 | [https://github.com/ejmahler/RustFFT](https://github.com/ejmahler/RustFFT) |
| `rustix` | `0.38.44` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [https://github.com/bytecodealliance/rustix](https://github.com/bytecodealliance/rustix) |
| `rustix` | `1.1.5` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [https://github.com/bytecodealliance/rustix](https://github.com/bytecodealliance/rustix) |
| `rustversion` | `1.0.23` | MIT OR Apache-2.0 | [https://github.com/dtolnay/rustversion](https://github.com/dtolnay/rustversion) |
| `same-file` | `1.0.6` | Unlicense/MIT | [https://github.com/BurntSushi/same-file](https://github.com/BurntSushi/same-file) |
| `scoped-tls` | `1.0.1` | MIT/Apache-2.0 | [https://github.com/alexcrichton/scoped-tls](https://github.com/alexcrichton/scoped-tls) |
| `scopeguard` | `1.2.0` | MIT OR Apache-2.0 | [https://github.com/bluss/scopeguard](https://github.com/bluss/scopeguard) |
| `sctk-adwaita` | `0.10.1` | MIT | [https://github.com/PolyMeilex/sctk-adwaita](https://github.com/PolyMeilex/sctk-adwaita) |
| `self_cell` | `1.3.0` | Apache-2.0 OR GPL-2.0-only | [https://github.com/Voultapher/self_cell](https://github.com/Voultapher/self_cell) |
| `semver` | `1.0.28` | MIT OR Apache-2.0 | [https://github.com/dtolnay/semver](https://github.com/dtolnay/semver) |
| `serde` | `1.0.229` | MIT OR Apache-2.0 | [https://github.com/serde-rs/serde](https://github.com/serde-rs/serde) |
| `serde_core` | `1.0.229` | MIT OR Apache-2.0 | [https://github.com/serde-rs/serde](https://github.com/serde-rs/serde) |
| `serde_derive` | `1.0.229` | MIT OR Apache-2.0 | [https://github.com/serde-rs/serde](https://github.com/serde-rs/serde) |
| `serde_json` | `1.0.151` | MIT OR Apache-2.0 | [https://github.com/serde-rs/json](https://github.com/serde-rs/json) |
| `serde_repr` | `0.1.21` | MIT OR Apache-2.0 | [https://github.com/dtolnay/serde-repr](https://github.com/dtolnay/serde-repr) |
| `serde_spanned` | `0.6.9` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `shlex` | `2.0.1` | MIT OR Apache-2.0 | [https://github.com/comex/rust-shlex](https://github.com/comex/rust-shlex) |
| `signal-hook-registry` | `1.4.8` | MIT OR Apache-2.0 | [https://github.com/vorner/signal-hook](https://github.com/vorner/signal-hook) |
| `simd-adler32` | `0.3.10` | MIT | [https://github.com/mcountryman/simd-adler32](https://github.com/mcountryman/simd-adler32) |
| `simd_cesu8` | `1.2.0` | Apache-2.0 OR MIT | [https://github.com/seancroach/simd_cesu8](https://github.com/seancroach/simd_cesu8) |
| `simd_helpers` | `0.1.0` | MIT | [https://github.com/lu-zero/simd_helpers](https://github.com/lu-zero/simd_helpers) |
| `simdutf8` | `0.1.5` | MIT OR Apache-2.0 | [https://github.com/rusticstuff/simdutf8](https://github.com/rusticstuff/simdutf8) |
| `siphasher` | `1.0.4` | MIT OR Apache-2.0 | [https://github.com/jedisct1/rust-siphash](https://github.com/jedisct1/rust-siphash) |
| `skrifa` | `0.44.0` | MIT OR Apache-2.0 | [https://github.com/googlefonts/fontations](https://github.com/googlefonts/fontations) |
| `slab` | `0.4.12` | MIT | [https://github.com/tokio-rs/slab](https://github.com/tokio-rs/slab) |
| `slotmap` | `1.1.1` | Zlib | [https://github.com/orlp/slotmap](https://github.com/orlp/slotmap) |
| `smallvec` | `1.16.1` | MIT OR Apache-2.0 | [https://github.com/servo/rust-smallvec](https://github.com/servo/rust-smallvec) |
| `smithay-client-toolkit` | `0.19.2` | MIT | [https://github.com/smithay/client-toolkit](https://github.com/smithay/client-toolkit) |
| `smol_str` | `0.2.2` | MIT OR Apache-2.0 | [https://github.com/rust-analyzer/smol_str](https://github.com/rust-analyzer/smol_str) |
| `spirv` | `0.4.0+sdk-1.4.341.0` | Apache-2.0 | [https://github.com/gfx-rs/rspirv](https://github.com/gfx-rs/rspirv) |
| `stable_deref_trait` | `1.2.1` | MIT OR Apache-2.0 | [https://github.com/storyyeller/stable_deref_trait](https://github.com/storyyeller/stable_deref_trait) |
| `static_assertions` | `1.1.0` | MIT OR Apache-2.0 | [https://github.com/nvzqz/static-assertions-rs](https://github.com/nvzqz/static-assertions-rs) |
| `strength_reduce` | `0.2.4` | MIT OR Apache-2.0 | [http://github.com/ejmahler/strength_reduce](http://github.com/ejmahler/strength_reduce) |
| `strict-num` | `0.1.1` | MIT | [https://github.com/RazrFalcon/strict-num](https://github.com/RazrFalcon/strict-num) |
| `string-interner` | `0.20.0` | MIT/Apache-2.0 | [https://github.com/robbepop/string-interner](https://github.com/robbepop/string-interner) |
| `syn` | `2.0.119` | MIT OR Apache-2.0 | [https://github.com/dtolnay/syn](https://github.com/dtolnay/syn) |
| `syn` | `3.0.6` | MIT OR Apache-2.0 | [https://github.com/dtolnay/syn](https://github.com/dtolnay/syn) |
| `tar` | `0.4.46` | MIT OR Apache-2.0 | [https://github.com/composefs/tar-rs](https://github.com/composefs/tar-rs) |
| `target-features` | `0.1.6` | MIT OR Apache-2.0 | [https://github.com/calebzulawski/target-features](https://github.com/calebzulawski/target-features) |
| `tempfile` | `3.27.0` | MIT OR Apache-2.0 | [https://github.com/Stebalien/tempfile](https://github.com/Stebalien/tempfile) |
| `termcolor` | `1.4.1` | Unlicense OR MIT | [https://github.com/BurntSushi/termcolor](https://github.com/BurntSushi/termcolor) |
| `thiserror` | `1.0.69` | MIT OR Apache-2.0 | [https://github.com/dtolnay/thiserror](https://github.com/dtolnay/thiserror) |
| `thiserror` | `2.0.20` | MIT OR Apache-2.0 | [https://github.com/dtolnay/thiserror](https://github.com/dtolnay/thiserror) |
| `thiserror-impl` | `1.0.69` | MIT OR Apache-2.0 | [https://github.com/dtolnay/thiserror](https://github.com/dtolnay/thiserror) |
| `thiserror-impl` | `2.0.20` | MIT OR Apache-2.0 | [https://github.com/dtolnay/thiserror](https://github.com/dtolnay/thiserror) |
| `tiff` | `0.11.3` | MIT | [https://github.com/image-rs/image-tiff](https://github.com/image-rs/image-tiff) |
| `tiny-skia` | `0.11.4` | BSD-3-Clause | [https://github.com/RazrFalcon/tiny-skia](https://github.com/RazrFalcon/tiny-skia) |
| `tiny-skia-path` | `0.11.4` | BSD-3-Clause | [https://github.com/RazrFalcon/tiny-skia/tree/master/path](https://github.com/RazrFalcon/tiny-skia/tree/master/path) |
| `tinystr` | `0.8.4` | Unicode-3.0 | [https://github.com/unicode-org/icu4x](https://github.com/unicode-org/icu4x) |
| `toml` | `0.5.11` | MIT/Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml` | `0.8.23` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml_datetime` | `0.6.11` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml_datetime` | `1.1.1+spec-1.1.0` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml_edit` | `0.22.27` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml_edit` | `0.25.15+spec-1.1.0` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml_parser` | `1.1.3+spec-1.1.0` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `toml_write` | `0.1.2` | MIT OR Apache-2.0 | [https://github.com/toml-rs/toml](https://github.com/toml-rs/toml) |
| `tracing` | `0.1.44` | MIT | [https://github.com/tokio-rs/tracing](https://github.com/tokio-rs/tracing) |
| `tracing-attributes` | `0.1.31` | MIT | [https://github.com/tokio-rs/tracing](https://github.com/tokio-rs/tracing) |
| `tracing-core` | `0.1.36` | MIT | [https://github.com/tokio-rs/tracing](https://github.com/tokio-rs/tracing) |
| `tract-core` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-data` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-extra` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-hir` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-linalg` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-nnef` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-onnx` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-onnx-opl` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `tract-transformers` | `0.23.8` | MIT OR Apache-2.0 | [https://github.com/sonos/tract](https://github.com/sonos/tract) |
| `transpose` | `0.2.3` | MIT OR Apache-2.0 | [https://github.com/ejmahler/transpose](https://github.com/ejmahler/transpose) |
| `ttf-parser` | `0.25.1` | MIT OR Apache-2.0 | [https://github.com/harfbuzz/ttf-parser](https://github.com/harfbuzz/ttf-parser) |
| `type-map` | `0.5.1` | MIT/Apache-2.0 | [https://github.com/kardeiz/type-map](https://github.com/kardeiz/type-map) |
| `typeid` | `1.0.3` | MIT OR Apache-2.0 | [https://github.com/dtolnay/typeid](https://github.com/dtolnay/typeid) |
| `uds_windows` | `1.2.1` | MIT | [https://github.com/haraldh/rust_uds_windows](https://github.com/haraldh/rust_uds_windows) |
| `unic-langid` | `0.9.6` | MIT OR Apache-2.0 | [https://github.com/zbraniecki/unic-locale](https://github.com/zbraniecki/unic-locale) |
| `unic-langid-impl` | `0.9.6` | MIT OR Apache-2.0 | [https://github.com/zbraniecki/unic-locale](https://github.com/zbraniecki/unic-locale) |
| `unicode-general-category` | `1.1.0` | Apache-2.0 | [https://github.com/yeslogic/unicode-general-category](https://github.com/yeslogic/unicode-general-category) |
| `unicode-ident` | `1.0.26` | (MIT OR Apache-2.0) AND Unicode-3.0 | [https://github.com/dtolnay/unicode-ident](https://github.com/dtolnay/unicode-ident) |
| `unicode-segmentation` | `1.13.3` | MIT OR Apache-2.0 | [https://github.com/unicode-rs/unicode-segmentation](https://github.com/unicode-rs/unicode-segmentation) |
| `unicode-width` | `0.2.2` | MIT OR Apache-2.0 | [https://github.com/unicode-rs/unicode-width](https://github.com/unicode-rs/unicode-width) |
| `utf8parse` | `0.2.2` | Apache-2.0 OR MIT | [https://github.com/alacritty/vte](https://github.com/alacritty/vte) |
| `uuid` | `1.26.1` | Apache-2.0 OR MIT | [https://github.com/uuid-rs/uuid](https://github.com/uuid-rs/uuid) |
| `v_frame` | `0.3.9` | BSD-2-Clause | [https://github.com/rust-av/v_frame](https://github.com/rust-av/v_frame) |
| `vello_common` | `0.1.0` | Apache-2.0 OR MIT | [https://github.com/linebender/vello](https://github.com/linebender/vello) |
| `vello_cpu` | `0.1.0` | Apache-2.0 OR MIT | [https://github.com/linebender/vello](https://github.com/linebender/vello) |
| `version_check` | `0.9.5` | MIT/Apache-2.0 | [https://github.com/SergioBenitez/version_check](https://github.com/SergioBenitez/version_check) |
| `walkdir` | `2.5.0` | Unlicense/MIT | [https://github.com/BurntSushi/walkdir](https://github.com/BurntSushi/walkdir) |
| `wasip2` | `1.0.4+wasi-0.2.12` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [https://github.com/bytecodealliance/wasi-rs](https://github.com/bytecodealliance/wasi-rs) |
| `wasm-bindgen` | `0.2.128` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) |
| `wasm-bindgen-futures` | `0.4.78` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures) |
| `wasm-bindgen-macro` | `0.2.128` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) |
| `wasm-bindgen-macro-support` | `0.2.128` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support](https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support) |
| `wasm-bindgen-shared` | `0.2.128` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) |
| `wayland-backend` | `0.3.17` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-client` | `0.31.15` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-csd-frame` | `0.3.0` | MIT | [https://github.com/rust-windowing/wayland-csd-frame](https://github.com/rust-windowing/wayland-csd-frame) |
| `wayland-cursor` | `0.31.14` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-protocols` | `0.32.13` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-protocols-plasma` | `0.3.12` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-protocols-wlr` | `0.3.12` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-scanner` | `0.31.11` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `wayland-sys` | `0.31.11` | MIT | [https://github.com/smithay/wayland-rs](https://github.com/smithay/wayland-rs) |
| `web-sys` | `0.3.105` | MIT OR Apache-2.0 | [https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys) |
| `web-time` | `1.1.0` | MIT OR Apache-2.0 | [https://github.com/daxpedda/web-time](https://github.com/daxpedda/web-time) |
| `webp` | `0.3.1` | MIT OR Apache-2.0 | [https://github.com/jaredforth/webp.git](https://github.com/jaredforth/webp.git) |
| `weezl` | `0.1.12` | MIT OR Apache-2.0 | [https://github.com/image-rs/weezl](https://github.com/image-rs/weezl) |
| `wgpu` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-core` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-core-deps-apple` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-core-deps-emscripten` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-core-deps-wasm` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-core-deps-windows-linux-android` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-hal` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-naga-bridge` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `wgpu-types` | `30.0.1` | MIT OR Apache-2.0 | [https://github.com/gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) |
| `winapi-util` | `0.1.11` | Unlicense OR MIT | [https://github.com/BurntSushi/winapi-util](https://github.com/BurntSushi/winapi-util) |
| `windows` | `0.62.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-collections` | `0.3.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-core` | `0.62.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-future` | `0.3.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-implement` | `0.60.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-interface` | `0.59.3` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-link` | `0.2.1` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-numerics` | `0.3.1` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-result` | `0.4.1` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-strings` | `0.5.1` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-sys` | `0.45.0` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-sys` | `0.52.0` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-sys` | `0.59.0` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-sys` | `0.61.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-targets` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-targets` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows-threading` | `0.2.1` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_aarch64_gnullvm` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_aarch64_gnullvm` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_aarch64_msvc` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_aarch64_msvc` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_i686_gnu` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_i686_gnu` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_i686_gnullvm` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_i686_msvc` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_i686_msvc` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_x86_64_gnu` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_x86_64_gnu` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_x86_64_gnullvm` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_x86_64_gnullvm` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_x86_64_msvc` | `0.42.2` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `windows_x86_64_msvc` | `0.52.6` | MIT OR Apache-2.0 | [https://github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| `winit` | `0.30.13` | Apache-2.0 | [https://github.com/rust-windowing/winit](https://github.com/rust-windowing/winit) |
| `winnow` | `0.7.15` | MIT | [https://github.com/winnow-rs/winnow](https://github.com/winnow-rs/winnow) |
| `winnow` | `1.0.4` | MIT | [https://github.com/winnow-rs/winnow](https://github.com/winnow-rs/winnow) |
| `wit-bindgen` | `0.57.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [https://github.com/bytecodealliance/wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) |
| `x11-dl` | `2.21.0` | MIT | [https://github.com/AltF02/x11-rs.git](https://github.com/AltF02/x11-rs.git) |
| `x11rb` | `0.13.2` | MIT OR Apache-2.0 | [https://github.com/psychon/x11rb](https://github.com/psychon/x11rb) |
| `x11rb-protocol` | `0.13.2` | MIT OR Apache-2.0 | [https://github.com/psychon/x11rb](https://github.com/psychon/x11rb) |
| `xattr` | `1.6.1` | MIT OR Apache-2.0 | [https://github.com/Stebalien/xattr](https://github.com/Stebalien/xattr) |
| `xcursor` | `0.3.11` | MIT | [https://github.com/esposm03/xcursor-rs](https://github.com/esposm03/xcursor-rs) |
| `xkbcommon-dl` | `0.4.2` | MIT | [https://github.com/rust-windowing/xkbcommon-dl](https://github.com/rust-windowing/xkbcommon-dl) |
| `xkeysym` | `0.2.1` | MIT OR Apache-2.0 OR Zlib | [https://github.com/notgull/xkeysym](https://github.com/notgull/xkeysym) |
| `xml-rs` | `0.8.29` | MIT | [https://github.com/kornelski/xml-rs](https://github.com/kornelski/xml-rs) |
| `y4m` | `0.8.0` | MIT | [https://github.com/image-rs/y4m.git](https://github.com/image-rs/y4m.git) |
| `zbus` | `5.19.0` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |
| `zbus-lockstep` | `0.5.2` | MIT | [https://github.com/luukvanderduim/zbus-lockstep](https://github.com/luukvanderduim/zbus-lockstep) |
| `zbus-lockstep-macros` | `0.5.2` | MIT | [https://github.com/luukvanderduim/zbus-lockstep](https://github.com/luukvanderduim/zbus-lockstep) |
| `zbus_macros` | `5.19.0` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |
| `zbus_names` | `4.3.4` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |
| `zbus_xml` | `5.2.1` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |
| `zcheapstr` | `1.1.0` | MIT | [https://github.com/z-galaxy/zcheapstr/](https://github.com/z-galaxy/zcheapstr/) |
| `zerocopy` | `0.8.57` | BSD-2-Clause OR Apache-2.0 OR MIT | [https://github.com/google/zerocopy](https://github.com/google/zerocopy) |
| `zerocopy-derive` | `0.8.57` | BSD-2-Clause OR Apache-2.0 OR MIT | [https://github.com/google/zerocopy](https://github.com/google/zerocopy) |
| `zerofrom` | `0.1.8` | Unicode-3.0 | [https://github.com/unicode-org/icu4x](https://github.com/unicode-org/icu4x) |
| `zerovec` | `0.11.8` | Unicode-3.0 | [https://github.com/unicode-org/icu4x](https://github.com/unicode-org/icu4x) |
| `zlib-rs` | `0.6.8` | Zlib | [https://github.com/trifectatechfoundation/zlib-rs](https://github.com/trifectatechfoundation/zlib-rs) |
| `zmij` | `1.0.23` | MIT | [https://github.com/dtolnay/zmij](https://github.com/dtolnay/zmij) |
| `zune-core` | `0.5.3` | MIT OR Apache-2.0 OR Zlib | [https://github.com/etemesi254/zune-image](https://github.com/etemesi254/zune-image) |
| `zune-jpeg` | `0.5.15` | MIT OR Apache-2.0 OR Zlib | [https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) |
| `zvariant` | `5.15.0` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |
| `zvariant_derive` | `5.15.0` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |
| `zvariant_utils` | `4.2.0` | MIT | [https://github.com/z-galaxy/zbus/](https://github.com/z-galaxy/zbus/) |

---

## Complete License Texts

### MIT

```
The MIT License (MIT)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### Apache-2.0

```
Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.
      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.
      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.
      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity.
      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.
      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.
      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.
      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License.
      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work.
      "Contribution" shall mean any work of authorship that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner.
      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:
      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and
      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and
      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work; and
      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      shall any Contributor be liable to You for damages, including any
      direct, indirect, special, incidental, or consequential damages of any
      character arising as a result of this License or out of the use or
      inability to use the Work.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License.
```

### LGPL-2.1

```
GNU LESSER GENERAL PUBLIC LICENSE
                       Version 2.1, February 1999

 Copyright (C) 1991, 1999 Free Software Foundation, Inc.
 51 Franklin Street, Fifth Floor, Boston, MA  02110-1301  USA
 Everyone is permitted to copy and distribute verbatim copies
 of this license document, but changing it is not allowed.

[This is the first released version of the Lesser GPL.  It also counts
 as the successor of the GNU Library Public License, version 2, hence
 the version number 2.1.]

                            Preamble

  The licenses for most software are designed to take away your
freedom to share and change it.  By contrast, the GNU General Public
Licenses are intended to guarantee your freedom to share and change
free software--to make sure the software is free for all its users.

  This license, the Lesser General Public License, applies to some
specially designated software packages--typically libraries--of the
Free Software Foundation and other authors who decide to use it.  You
can use it too, but we suggest you first think carefully about whether
this license or the ordinary General Public License is the better
strategy to use in any particular case, based on the explanations below.

  When we speak of free software, we are referring to freedom of use,
not price.  Our General Public Licenses are designed to make sure that
you have the freedom to distribute copies of free software (and charge
for this service if you wish); that you receive source code or can get
it if you want it; that you can change the software and use pieces of
it in new free programs; and that you are informed that you can do
these things.

  To protect your rights, we need to make restrictions that forbid
distributors to deny you these rights or to ask you to surrender these
rights.  These restrictions translate to certain responsibilities for
you if you distribute copies of the library or if you modify it.

  For example, if you distribute copies of the library, whether gratis
or for a fee, you must give the recipients all the rights that we gave
you.  You must make sure that they, too, receive or can get the source
code.  If you link other code with the library, you must provide
complete object files to the recipients, so that they can relink them
with the library after making changes to the library and recompiling
it.  And you must show them these terms so they know their rights.

  We protect your rights with a two-step method: (1) we copyright the
library, and (2) we offer you this license, which gives you legal
permission to copy, distribute and/or modify the library.

                  TERMS AND CONDITIONS FOR COPYING,
                    DISTRIBUTION AND MODIFICATION

  0. This License Agreement applies to any software library or other
program which contains a notice placed by the copyright holder or
other authorized party saying it may be distributed under the terms of
this Lesser General Public License (also called "this License").
Each licensee is addressed as "you".

  A "library" means a collection of software functions and/or data
prepared so as to be conveniently linked with application programs
(which use some of those functions and data) to form executables.

  The "Library", below, refers to any such software library or work
which has been distributed under these terms.  A "work based on the
Library" means either the Library or any derivative work under
copyright law: that is to say, a work containing the Library or a
portion of it, either verbatim or with modifications and/or translated
straightforwardly into another language.  (Hereinafter, translation is
included without limitation in the term "modification".)

  "Source code" for a work means the preferred form of the work for
making modifications to it.  For a library, complete source code means
all the source code for all modules it contains, plus any associated
interface definition files, plus the scripts used to control compilation
and installation of the library.

  Activities other than copying, distribution and modification are not
covered by this License; they are outside its scope.  The act of
running a program using the Library is not restricted, and output from
such a program is covered only if its contents constitute a work based
on the Library (independent of the use of the Library in a tool for
writing it).  Whether that is true depends on what the Library does
and what the program that uses the Library does.

  1. You may copy and distribute verbatim copies of the Library's
complete source code as you receive it, in any medium, provided that
you conspicuously and appropriately publish on each copy an appropriate
copyright notice and disclaimer of warranty; keep intact all the
notices that refer to this License and to the absence of any warranty;
and distribute a copy of this License along with the Library.

  2. You may modify your copy or copies of the Library or any portion
of it, thus forming a work based on the Library, and copy and
distribute such modifications under the terms of Section 1 above,
provided that you also meet all of these conditions:
    a) The modified work must itself be a software library.
    b) You must cause the files modified to carry prominent notices
    stating that you changed the files and the date of any change.
    c) You must cause the whole of the work to be licensed at no
    charge to all third parties under the terms of this License.
    d) If a facility in the modified Library refers to a function or a
    table of data to be supplied by an application program that uses
    the facility, other than as an argument passed when the facility
    is invoked, then you must make a good faith effort to ensure that,
    in the event an application does not supply such function or
    table, the facility still operates, and performs whatever part of
    its purpose remains meaningful.

  3. You may opt to apply the terms of the ordinary GNU General Public
License instead of this License to a given copy of the Library.  To do
this, you must alter all the notices that refer to this License, so
that they refer to the ordinary GNU General Public License, version 2,
instead of to this License.

  4. You may copy and distribute the Library (or a portion or
derivative of it, under Section 2) in object code or executable form
under the terms of Sections 1 and 2 above provided that you accompany
it with the complete corresponding machine-readable source code, which
must be distributed under the terms of Sections 1 and 2 above on a
medium customarily used for software interchange.

  5. A program that contains no derivative of any portion of the
Library, but is designed to work with the Library by being compiled or
linked with it, is called a "work that uses the Library".  Such a
work, in isolation, is not a derivative work of the Library, and
therefore falls outside the scope of this License.

  However, linking a "work that uses the Library" with the Library
creates an executable that is a derivative of the Library (because it
contains portions of the Library), rather than a "work that uses the
library".  The executable is therefore covered by this License.
Section 6 states terms for distribution of such executables.

  6. As an exception to the Sections above, you may also combine or
link a "work that uses the Library" with the Library to produce a
work containing portions of the Library, and distribute that work
under terms of your choice, provided that the terms permit
modification of the work for the customer's own use and reverse
engineering for debugging such modifications.

  You must give prominent notice with each copy of the work that the
Library is used in it and that the Library and its use are covered by
this License.  You must supply a copy of this License.  If the work
during execution displays copyright notices, you must include the
copyright notice for the Library among them, as well as a reference
directing the user to the copy of this License.  Also, you must do one
of these things:

    a) Accompany the work with the complete corresponding
    machine-readable source code for the Library including whatever
    changes were used in the work (which must be distributed under
    Sections 1 and 2 above); and, if the work is an executable linked
    with the Library, with the complete machine-readable "work that
    uses the Library", as object code and/or source code, so that the
    user can modify the Library and then relink to produce a modified
    executable containing the modified Library.
    b) Use a suitable shared library mechanism for linking with the
    Library.
    c) Accompany the work with a written offer, valid for at
    least three years, to give the same user the materials
    specified in Subsection 6a above.
    d) If distribution of the work is made by offering access to copy
    from a designated place, offer equivalent access to copy the above
    specified materials from the same place.
    e) Verify that the user has already received a copy of these
    materials or that you have already sent this user a copy.
```

### BSD-3-Clause

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software without
   specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### BSD-2-Clause

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### ISC

```
Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### Zlib

```
This software is provided 'as-is', without any express or implied
warranty. In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

### OFL-1.1

```
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded, 
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may include
source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless prominent written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```


//! Duckfoot Barrel 1 — the photo engine.
//!
//! Decode once on the CPU, upload once to the GPU, demosaic once, then adjust per frame
//! with a single fused fragment pass. Nothing crosses back to the CPU in the interactive
//! path. See DESIGN.md for why that constraint is the whole architecture.
//!
//! This crate owns no window and creates no `Device`. Both are handed to it by the shell,
//! which is what keeps the eventual Android build a new bootstrap rather than a fork.

pub mod curve;
pub mod decode;
pub mod engine;
pub mod export;
pub mod histogram;
pub mod params;

pub use curve::{identity_nodes, CurveNode};
pub use decode::{CfaImage, RawDecoder, RawloaderBackend};
pub use engine::{Engine, LoadedImage, OUTPUT_FORMAT};
pub use export::{ExportFormat, ExportedImage};
pub use histogram::Histogram;
pub use params::{ColorBalance, Exposure, Filmic, Module, PhotoStack, Rgb, WhiteBalance};

/// File extensions the decoder will attempt.
pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef", "raw",
];

/// Extensions for a file-dialog filter, in both cases.
///
/// XDG desktop portal filters are literal globs (`*.arw`) and are matched
/// case-sensitively, so a lowercase-only list hides `DSC08450.ARW` — which is exactly what
/// cameras actually write. Windows and macOS are case-insensitive here; listing both costs
/// nothing there and is the difference between a usable and an empty picker on Linux.
pub fn raw_dialog_extensions() -> Vec<String> {
    RAW_EXTENSIONS
        .iter()
        .flat_map(|e| [e.to_string(), e.to_uppercase()])
        .collect()
}

pub fn is_raw_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| RAW_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn dialog_filter_covers_both_cases() {
        let exts = raw_dialog_extensions();
        // The regression: cameras write uppercase, portal globs are case-sensitive.
        assert!(exts.contains(&"arw".to_string()));
        assert!(exts.contains(&"ARW".to_string()));
        assert!(exts.contains(&"CR3".to_string()));
        assert_eq!(exts.len(), RAW_EXTENSIONS.len() * 2);
    }

    #[test]
    fn recognises_raw_extensions_regardless_of_case() {
        assert!(is_raw_path(Path::new("/tmp/IMG_0001.CR3")));
        assert!(is_raw_path(Path::new("/tmp/shot.nef")));
        assert!(!is_raw_path(Path::new("/tmp/shot.jpg")));
        assert!(!is_raw_path(Path::new("/tmp/noextension")));
    }
}

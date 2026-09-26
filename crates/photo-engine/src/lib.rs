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
pub mod fsio;
pub mod histogram;
pub mod metadata;
pub mod params;
pub mod preview;
pub mod sidecar;
pub mod temperature;
#[doc(hidden)]
pub mod testkit;

pub use curve::{identity_nodes, CurveNode};
pub use decode::{
    decode_raster, decode_raw, raster_orientation, CfaImage, DefaultRawDecoder, RasterImage,
    RawDecoder, RawlerBackend, RawloaderBackend,
};
pub use engine::{
    upload_cfa, upload_raster, Engine, LoadedImage, UploadPool, UploadedImage, MAX_PREVIEW_LEVEL,
    OUTPUT_FORMAT,
};
pub use export::{EncodeOptions, ExportFormat, ExportedImage, JPEG_QUALITY};
pub use histogram::Histogram;
pub use metadata::{file_date_compact, read_info, read_raster_info, read_raw_info, PhotoInfo};
pub use params::{
    Clarity, ColorBalance, ColorBand, ColorEq, Crop, CurveDomain, Dehaze, Exposure, ExposureMode,
    Filmic, Geometry, Module, PhotoStack, Rgb, Sharpen, Tonal, Vibrance, Vignette, WhiteBalance,
};
pub use preview::{
    decode_jpeg_scaled, file_orientation, find_embedded_jpeg, load_embedded_preview, Preview,
};
pub use sidecar::{
    find_sidecar, load_sidecar, load_sidecar_full, save_sidecar, save_sidecar_full, sidecar_path,
    update_sidecar, write_sidecar, AssetMetadata, Flag, SidecarData, Snapshot, SIDECAR_EXT,
    SIDECAR_VERSION,
};
pub use temperature::WhiteScale;

/// File extensions the RAW decoder will attempt.
pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef", "raw",
];

/// File extensions for standard raster formats supported directly.
pub const RASTER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "webp"];

/// Extensions for a RAW-only file-dialog filter, in both cases.
pub fn raw_dialog_extensions() -> Vec<String> {
    RAW_EXTENSIONS
        .iter()
        .flat_map(|e| [e.to_string(), e.to_uppercase()])
        .collect()
}

/// Extensions for all supported image formats (RAW + Raster) in both cases.
pub fn all_image_dialog_extensions() -> Vec<String> {
    RAW_EXTENSIONS
        .iter()
        .chain(RASTER_EXTENSIONS.iter())
        .flat_map(|e| [e.to_string(), e.to_uppercase()])
        .collect()
}

/// Extensions for raster image formats (JPEG, PNG, TIFF, WebP) in both cases.
pub fn raster_dialog_extensions() -> Vec<String> {
    RASTER_EXTENSIONS
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

pub fn is_raster_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| RASTER_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_supported_image(path: &std::path::Path) -> bool {
    is_raw_path(path) || is_raster_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn dialog_filter_covers_both_cases() {
        let exts = raw_dialog_extensions();
        assert!(exts.contains(&"arw".to_string()));
        assert!(exts.contains(&"ARW".to_string()));
        assert!(exts.contains(&"CR3".to_string()));
        assert_eq!(exts.len(), RAW_EXTENSIONS.len() * 2);

        let all_exts = all_image_dialog_extensions();
        assert!(all_exts.contains(&"jpg".to_string()));
        assert!(all_exts.contains(&"PNG".to_string()));
        assert!(all_exts.contains(&"webp".to_string()));
        assert_eq!(
            all_exts.len(),
            (RAW_EXTENSIONS.len() + RASTER_EXTENSIONS.len()) * 2
        );
    }

    #[test]
    fn recognises_raw_and_raster_extensions_regardless_of_case() {
        assert!(is_raw_path(Path::new("/tmp/IMG_0001.CR3")));
        assert!(is_raw_path(Path::new("/tmp/shot.nef")));
        assert!(!is_raw_path(Path::new("/tmp/shot.jpg")));
        assert!(is_raster_path(Path::new("/tmp/shot.jpg")));
        assert!(is_raster_path(Path::new("/tmp/photo.PNG")));
        assert!(is_raster_path(Path::new("/tmp/scan.TIFF")));
        assert!(is_supported_image(Path::new("/tmp/shot.ARW")));
        assert!(is_supported_image(Path::new("/tmp/shot.jpg")));
        assert!(!is_supported_image(Path::new("/tmp/noextension")));
    }

    #[test]
    fn shaders_are_valid_wgsl() {
        let shaders = [
            ("adjust.wgsl", include_str!("shaders/adjust.wgsl")),
            (
                "demosaic_green.wgsl",
                include_str!("shaders/demosaic_green.wgsl"),
            ),
            ("demosaic_rb.wgsl", include_str!("shaders/demosaic_rb.wgsl")),
            ("downsample.wgsl", include_str!("shaders/downsample.wgsl")),
            ("histogram.wgsl", include_str!("shaders/histogram.wgsl")),
        ];

        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );

        for (name, source) in shaders {
            let module = wgpu::naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{name} failed WGSL parse: {e}"));
            validator
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name} failed WGSL validation: {e}"));
        }
    }
}

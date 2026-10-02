//! Export recipes (presets) for multi-export.
//!
//! Inspired by Capture One's process recipes and Lightroom export presets.
//! Allows running multiple export targets in parallel from a single raw decode.

use serde::{Deserialize, Serialize};

/// The engine's own format list: batch and single export share one encoder.
pub type ExportImageFormat = photo_engine::ExportFormat;

/// How big a recipe exports: the engine's, shared with the single export.
pub type SizingOption = photo_engine::Resize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportRecipe {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Format, quality, size, depth, sharpening and metadata: the same settings the
    /// single export uses. Flattened, so recipes saved when they held only format,
    /// quality and size still load, with defaults for the rest.
    #[serde(flatten)]
    pub settings: photo_engine::ExportSettings,
    pub subfolder: Option<String>,
}

impl ExportRecipe {
    fn new(
        id: &str,
        name: &str,
        enabled: bool,
        format: ExportImageFormat,
        quality: u8,
        sizing: SizingOption,
        subfolder: Option<&str>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled,
            settings: photo_engine::ExportSettings {
                quality,
                sizing,
                ..photo_engine::ExportSettings::new(format)
            },
            subfolder: subfolder.map(Into::into),
        }
    }
}

impl ExportRecipe {
    pub fn default_recipes() -> Vec<Self> {
        use ExportImageFormat as F;
        let mut print = Self::new(
            "print_tiff",
            "Print TIFF (16-bit, full resolution)",
            false,
            F::Tiff,
            100,
            SizingOption::Original,
            Some("Print"),
        );
        print.settings.depth = photo_engine::BitDepth::Sixteen;
        let mut web = Self::new(
            "web_jpeg_2048",
            "Web JPEG (2048 px)",
            true,
            F::Jpeg,
            85,
            SizingOption::LongEdge(2048),
            Some("Web"),
        );
        web.settings.sharpen = photo_engine::SharpenFor::Screen;
        let mut social = Self::new(
            "social_webp",
            "Social WebP (1080 px)",
            false,
            F::Webp,
            82,
            SizingOption::LongEdge(1080),
            Some("Social"),
        );
        social.settings.sharpen = photo_engine::SharpenFor::Screen;
        social.settings.metadata = photo_engine::MetadataPolicy::CopyrightOnly;

        let mut ecommerce_png = Self::new(
            "ecommerce_png",
            "E-Commerce Cutout (Transparent PNG)",
            false,
            F::Png,
            100,
            SizingOption::Original,
            Some("Cutouts"),
        );
        ecommerce_png.settings.remove_background = true;

        let mut catalog_white = Self::new(
            "catalog_white",
            "Catalog Pure White (JPEG)",
            false,
            F::Jpeg,
            92,
            SizingOption::LongEdge(2048),
            Some("Catalog"),
        );
        catalog_white.settings.remove_background = true;
        catalog_white.settings.matte_color = Some([255, 255, 255]);
        catalog_white.settings.sharpen = photo_engine::SharpenFor::Screen;

        vec![
            web,
            Self::new(
                "full_png",
                "Lossless PNG (full resolution)",
                false,
                F::Png,
                100,
                SizingOption::Original,
                None,
            ),
            print,
            social,
            ecommerce_png,
            catalog_white,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_saved_before_export_settings_still_load() {
        let old = r#"{"id":"w","name":"Web","enabled":true,"format":"Jpeg","quality":85,
            "sizing":{"LongEdge":2048},"subfolder":"Web"}"#;
        let r: ExportRecipe = serde_json::from_str(old).unwrap();
        assert_eq!(r.settings.format, ExportImageFormat::Jpeg);
        assert_eq!(r.settings.quality, 85);
        assert_eq!(r.settings.sizing, SizingOption::LongEdge(2048));
        assert!(r.settings.strip_location);
        // And round-trip as they are now.
        let again: ExportRecipe =
            serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(again, r);
    }
}

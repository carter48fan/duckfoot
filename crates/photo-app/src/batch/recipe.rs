//! Export recipes (presets) for multi-export.
//!
//! Inspired by Capture One's process recipes and Lightroom export presets.
//! Allows running multiple export targets in parallel from a single raw decode.

use serde::{Deserialize, Serialize};

/// The engine's own format list: batch and single export share one encoder.
pub type ExportImageFormat = photo_engine::ExportFormat;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum SizingOption {
    #[default]
    Original,
    LongEdge(u32),
    ShortEdge(u32),
    FitDimensions {
        width: u32,
        height: u32,
    },
    Percentage(f32),
}

impl SizingOption {
    /// Computes the target dimensions given the source dimensions.
    pub fn calculate_size(&self, src_w: u32, src_h: u32) -> (u32, u32) {
        match *self {
            Self::Original => (src_w, src_h),
            Self::LongEdge(long) => {
                if src_w >= src_h {
                    let h = ((src_h as f32 / src_w as f32) * long as f32).round() as u32;
                    (long.max(1), h.max(1))
                } else {
                    let w = ((src_w as f32 / src_h as f32) * long as f32).round() as u32;
                    (w.max(1), long.max(1))
                }
            }
            Self::ShortEdge(short) => {
                if src_w <= src_h {
                    let h = ((src_h as f32 / src_w as f32) * short as f32).round() as u32;
                    (short.max(1), h.max(1))
                } else {
                    let w = ((src_w as f32 / src_h as f32) * short as f32).round() as u32;
                    (w.max(1), short.max(1))
                }
            }
            Self::FitDimensions { width, height } => {
                let scale_w = width as f32 / src_w as f32;
                let scale_h = height as f32 / src_h as f32;
                let scale = scale_w.min(scale_h);
                (
                    ((src_w as f32 * scale).round() as u32).max(1),
                    ((src_h as f32 * scale).round() as u32).max(1),
                )
            }
            Self::Percentage(pct) => {
                let p = (pct * 0.01).clamp(0.01, 4.0);
                (
                    ((src_w as f32 * p).round() as u32).max(1),
                    ((src_h as f32 * p).round() as u32).max(1),
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportRecipe {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub format: ExportImageFormat,
    /// JPEG / WebP quality (1..=100; WebP at 100 is lossless)
    pub quality: u8,
    pub sizing: SizingOption,
    pub subfolder: Option<String>,
}

impl ExportRecipe {
    pub fn default_recipes() -> Vec<Self> {
        vec![
            Self {
                id: "web_jpeg_2048".into(),
                name: "Web JPEG (2048 px)".into(),
                enabled: true,
                format: ExportImageFormat::Jpeg,
                quality: 85,
                sizing: SizingOption::LongEdge(2048),
                subfolder: Some("Web".into()),
            },
            Self {
                id: "full_png".into(),
                name: "Lossless PNG (full resolution)".into(),
                enabled: false,
                format: ExportImageFormat::Png,
                quality: 100,
                sizing: SizingOption::Original,
                subfolder: None,
            },
            Self {
                id: "print_tiff".into(),
                name: "Print TIFF (full resolution)".into(),
                enabled: false,
                format: ExportImageFormat::Tiff,
                quality: 100,
                sizing: SizingOption::Original,
                subfolder: Some("Print".into()),
            },
            Self {
                id: "social_webp".into(),
                name: "Social WebP (1080 px)".into(),
                enabled: false,
                format: ExportImageFormat::Webp,
                quality: 82,
                sizing: SizingOption::LongEdge(1080),
                subfolder: Some("Social".into()),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing_options_calculate_correct_dimensions() {
        let (src_w, src_h) = (6000, 4000); // 3:2 landscape
        assert_eq!(
            SizingOption::Original.calculate_size(src_w, src_h),
            (6000, 4000)
        );

        // Long edge 2048 on 3:2 landscape -> 2048 x 1365
        let (w, h) = SizingOption::LongEdge(2048).calculate_size(src_w, src_h);
        assert_eq!(w, 2048);
        assert_eq!(h, 1365);

        // Percentage 50% -> 3000 x 2000
        assert_eq!(
            SizingOption::Percentage(50.0).calculate_size(src_w, src_h),
            (3000, 2000)
        );
    }
}

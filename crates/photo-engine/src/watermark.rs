//! A line of text over an export: a name or a copyright, in a corner or the middle.
//!
//! Drawn on the finished pixels after the resize, output sharpening and background
//! removal, so it is sized to the photo as written (the same share of a 1080 px recipe as
//! of a full-size one), is never sharpened, and is never cut away. Like those passes it is
//! not part of the edit and not shown on the stage.
//!
//! Glyphs are rasterised by `ab_glyph`, already in the dependency tree (through winit), in
//! the app's own Hanken Grotesk, so the binary carries the face once more and nothing new.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};

use crate::export::{ExportedImage, Sample};

/// The face the mark is set in: the app's UI text face, which the binary already carries.
const FACE: &[u8] = include_bytes!("../../photo-app/assets/fonts/HankenGrotesk-Variable.ttf");

/// The most text a mark holds, in bytes of UTF-8. Kept short and fixed so the export
/// settings stay `Copy`; a credit line is well under it.
pub const TEXT_MAX: usize = 96;

/// Text of at most [`TEXT_MAX`] bytes, stored inline. Read and written as a string.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ShortText {
    len: u8,
    bytes: [u8; TEXT_MAX],
}

impl ShortText {
    /// `text`, cut at a character boundary to fit, with control characters left out.
    pub fn new(text: &str) -> Self {
        let mut out = Self::default();
        for c in text.chars().filter(|c| !c.is_control()) {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            let at = out.len as usize;
            if at + s.len() > TEXT_MAX {
                break;
            }
            out.bytes[at..at + s.len()].copy_from_slice(s.as_bytes());
            out.len += s.len() as u8;
        }
        out
    }

    pub fn as_str(&self) -> &str {
        // Only ever filled with whole characters, above.
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl Default for ShortText {
    fn default() -> Self {
        Self {
            len: 0,
            bytes: [0; TEXT_MAX],
        }
    }
}

impl std::fmt::Debug for ShortText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl Serialize for ShortText {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ShortText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::new(&String::deserialize(d)?))
    }
}

/// Where the mark sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
    Centre,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Watermark {
    pub text: ShortText,
    pub corner: Corner,
    /// The text's height, % of the image's long edge.
    pub size: f32,
    /// 0..1.
    pub opacity: f32,
    /// The gap to the edges, % of the long edge.
    pub margin: f32,
    /// Dark text, for a light photo; light otherwise.
    pub dark: bool,
}

impl Default for Watermark {
    fn default() -> Self {
        Self {
            text: ShortText::default(),
            corner: Corner::BottomRight,
            size: 3.0,
            opacity: 0.6,
            margin: 2.0,
            dark: false,
        }
    }
}

impl Watermark {
    /// Whether there is anything to draw.
    pub fn is_on(&self) -> bool {
        !self.text.as_str().trim().is_empty() && self.opacity > 0.0 && self.size > 0.0
    }

    /// The rectangle (x, y, width, height, in pixels) the mark covers on a `width` ×
    /// `height` image, with each glyph's coverage in it, row by row. `None` when there is
    /// nothing to draw or it would not fit.
    fn layout(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32, Vec<f32>)> {
        if !self.is_on() || width == 0 || height == 0 {
            return None;
        }
        let font = FontRef::try_from_slice(FACE).ok()?;
        let long = width.max(height) as f32;
        let px = (self.size.clamp(0.5, 20.0) * 0.01 * long).max(6.0);
        let scaled = font.as_scaled(PxScale::from(px));
        let text = self.text.as_str().trim();

        // Pen positions along one baseline, kerned.
        let mut glyphs = Vec::new();
        let mut pen = 0.0f32;
        let mut last = None;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(prev) = last {
                pen += scaled.kern(prev, id);
            }
            glyphs.push(id.with_scale_and_position(px, ab_glyph::point(pen, scaled.ascent())));
            pen += scaled.h_advance(id);
            last = Some(id);
        }
        let (tw, th) = (
            pen.ceil() as u32,
            (scaled.ascent() - scaled.descent()).ceil() as u32,
        );
        let margin = (self.margin.clamp(0.0, 20.0) * 0.01 * long).round() as u32;
        if tw == 0 || tw + 2 * margin > width || th + 2 * margin > height {
            return None;
        }
        let (x, y) = match self.corner {
            Corner::TopLeft => (margin, margin),
            Corner::TopRight => (width - margin - tw, margin),
            Corner::BottomLeft => (margin, height - margin - th),
            Corner::BottomRight => (width - margin - tw, height - margin - th),
            Corner::Centre => ((width - tw) / 2, (height - th) / 2),
        };
        let mut cover = vec![0.0f32; (tw * th) as usize];
        for g in glyphs {
            let Some(outlined) = font.outline_glyph(g) else {
                continue;
            };
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, c| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px >= 0 && py >= 0 && (px as u32) < tw && (py as u32) < th {
                    let i = (py as u32 * tw + px as u32) as usize;
                    cover[i] = (cover[i] + c).min(1.0);
                }
            });
        }
        Some((x, y, tw, th, cover))
    }

    /// Where the mark goes on a `width` × `height` image, normalised (x, y, width,
    /// height): what [`Self::draw`] returns, without drawing.
    pub fn rect(&self, width: u32, height: u32) -> Option<[f32; 4]> {
        let (x, y, tw, th, _) = self.layout(width, height)?;
        let (w, h) = (width as f32, height as f32);
        Some([x as f32 / w, y as f32 / h, tw as f32 / w, th as f32 / h])
    }

    /// Draws the mark on `img`, whose samples are encoded (as written). Where the photo is
    /// see-through (a cutout), the mark is laid over it and its alpha raised to match.
    /// Returns the rectangle drawn, normalised to the image (x, y, width, height), for the
    /// gain map to leave alone.
    pub fn draw<P: Sample>(&self, img: &mut ExportedImage<P>) -> Option<[f32; 4]> {
        let (x, y, tw, th, cover) = self.layout(img.width, img.height)?;
        let ink = if self.dark { 0.0 } else { P::MAX };
        let opacity = self.opacity.clamp(0.0, 1.0);
        for row in 0..th {
            for col in 0..tw {
                let a = cover[(row * tw + col) as usize] * opacity;
                if a <= 0.0 {
                    continue;
                }
                let i = (((y + row) * img.width + x + col) * 4) as usize;
                let under = img.rgba[i + 3].to_f32() / P::MAX;
                let out_a = a + under * (1.0 - a);
                for c in 0..3 {
                    let v = img.rgba[i + c].to_f32();
                    // "Over", in the encoded values, as an editor's text layer composites.
                    let mixed = (ink * a + v * under * (1.0 - a)) / out_a.max(1e-6);
                    img.rgba[i + c] = P::from_f32(mixed);
                }
                img.rgba[i + 3] = P::from_f32(out_a * P::MAX);
            }
        }
        let (w, h) = (img.width as f32, img.height as f32);
        Some([x as f32 / w, y as f32 / h, tw as f32 / w, th as f32 / h])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(w: u32, h: u32) -> ExportedImage<u8> {
        ExportedImage {
            width: w,
            height: h,
            rgba: [90u8, 90, 90, 255].repeat((w * h) as usize),
        }
    }

    fn mark(text: &str) -> Watermark {
        Watermark {
            text: ShortText::new(text),
            ..Default::default()
        }
    }

    #[test]
    fn only_the_marks_rectangle_changes() {
        let mut img = grey(800, 600);
        let before = img.rgba.clone();
        let r = mark("© 2026 A. Photographer")
            .draw(&mut img)
            .expect("drawn");
        let (x0, y0) = ((r[0] * 800.0) as u32, (r[1] * 600.0) as u32);
        let (x1, y1) = (x0 + (r[2] * 800.0) as u32, y0 + (r[3] * 600.0) as u32);
        let mut inside = 0;
        for y in 0..600 {
            for x in 0..800 {
                let i = ((y * 800 + x) * 4) as usize;
                let changed = img.rgba[i..i + 4] != before[i..i + 4];
                let within = x >= x0 && x <= x1 && y >= y0 && y <= y1;
                assert!(!changed || within, "({x}, {y}) changed outside the mark");
                inside += usize::from(changed);
            }
        }
        assert!(inside > 100, "hardly anything drawn: {inside}");
        // Bottom right, by default.
        assert!(x0 > 400 && y0 > 300);
    }

    #[test]
    fn it_scales_with_the_long_edge_and_vanishes_at_no_opacity() {
        let small = mark("Duckfoot").layout(1000, 600).unwrap();
        let large = mark("Duckfoot").layout(2000, 1200).unwrap();
        let ratio = large.2 as f32 / small.2 as f32;
        assert!((1.8..2.2).contains(&ratio), "{ratio}");

        let mut img = grey(400, 300);
        let before = img.rgba.clone();
        let none = Watermark {
            opacity: 0.0,
            ..mark("x")
        };
        assert!(none.draw(&mut img).is_none());
        assert_eq!(img.rgba, before);
    }

    #[test]
    fn sixteen_bit_images_take_it_too() {
        let mut img = ExportedImage::<u16> {
            width: 600,
            height: 400,
            rgba: [20000u16, 20000, 20000, 65535].repeat(600 * 400),
        };
        assert!(mark("16").draw(&mut img).is_some());
        assert!(img.rgba.chunks(4).any(|p| p[0] > 30000));
    }

    #[test]
    fn short_text_is_cut_at_a_character_and_reads_back_as_a_string() {
        let long = "é".repeat(80);
        let t = ShortText::new(&long);
        assert!(t.as_str().len() <= TEXT_MAX);
        assert!(t.as_str().chars().all(|c| c == 'é'));
        let json = serde_json::to_string(&mark("a \"b\"")).unwrap();
        let back: Watermark = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text.as_str(), "a \"b\"");
        let old: Watermark = serde_json::from_str("{}").unwrap();
        assert!(!old.is_on());
    }
}

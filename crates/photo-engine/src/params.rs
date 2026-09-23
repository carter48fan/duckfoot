//! The adjustment stack — plain data, no GPU types.
//!
//! Fixed order, not rearrangeable. That is the line that stops the photo studio becoming
//! a node graph (DESIGN.md), and it is also precisely what lets the whole chain fuse into
//! one shader pass. The product constraint and the fast implementation are the same
//! decision.

use serde::{Deserialize, Serialize};

use crate::curve::{identity_nodes, CurveNode};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn splat(v: f32) -> Self {
        Self { r: v, g: v, b: v }
    }
    pub(crate) fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, 0.0]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WhiteBalance {
    /// Multiplies the as-shot camera neutral; 1.0 leaves the shot as metered.
    pub tint_r: f32,
    pub tint_b: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    pub ev: f32,
    pub black: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Filmic {
    pub white_ev: f32,
    pub black_ev: f32,
    /// Width of the near-linear midtone region, as a **percentage of the log range**.
    ///
    /// The shader divides the normalised log position by `latitude / 100`, so this is the
    /// sigmoid's softness: small values give a steep S-curve, large values flatten it.
    /// Anything much above ~60 compresses the whole image toward mid grey — at 200 the
    /// output spanned only 0.43..0.57 and every photograph came out as a flat slab.
    pub latitude: f32,
    pub contrast: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorBalance {
    pub lift: Rgb,
    pub gamma: Rgb,
    pub gain: Rgb,
    pub offset: Rgb,
}

/// A crop, in normalised coordinates of the **rotated** image, origin top-left.
///
/// Deliberately not applied in the shader. Cropping in the shader would resize the output
/// texture, which means reallocating ~97 MB on every frame of a crop drag. Instead the
/// crop is honoured by display, export and the histogram, and the full frame stays on the
/// GPU — which is also what makes the crop non-destructive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl Crop {
    pub fn is_full_frame(&self) -> bool {
        self.x <= 0.0 && self.y <= 0.0 && self.width >= 1.0 && self.height >= 1.0
    }

    /// Clamped to the frame, and never smaller than a few pixels, so a stray click cannot
    /// produce a zero-area image that nothing downstream can render or encode.
    pub fn sanitised(&self) -> Self {
        const MIN: f32 = 0.01;
        let width = self.width.clamp(MIN, 1.0);
        let height = self.height.clamp(MIN, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - width),
            y: self.y.clamp(0.0, 1.0 - height),
            width,
            height,
        }
    }

    /// Pixel rect within an image of the given size: (x, y, width, height).
    pub fn to_pixels(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let c = self.sanitised();
        let x = (c.x * width as f32).round() as u32;
        let y = (c.y * height as f32).round() as u32;
        let w = ((c.width * width as f32).round() as u32).clamp(1, width - x.min(width - 1));
        let h = ((c.height * height as f32).round() as u32).clamp(1, height - y.min(height - 1));
        (x, y, w, h)
    }
}

/// Orientation and framing. Rotation and mirroring are applied by the shader; the crop is
/// carried alongside because it is expressed in the rotated frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// Right-angle turns clockwise, 0..=3.
    pub quarter_turns: u8,
    pub mirror_h: bool,
    pub mirror_v: bool,
    /// Fine straightening, degrees clockwise. Does not change the output dimensions —
    /// the corners rotate out of frame and a crop is expected to take them.
    pub straighten_deg: f32,
    pub crop: Crop,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            quarter_turns: 0,
            mirror_h: false,
            mirror_v: false,
            straighten_deg: 0.0,
            crop: Crop::default(),
        }
    }
}

impl Geometry {
    /// Right-angle turns swap the axes; straightening does not.
    pub fn swaps_axes(&self) -> bool {
        self.quarter_turns % 2 == 1
    }

    pub fn output_size(&self, sensor_w: u32, sensor_h: u32) -> (u32, u32) {
        if self.swaps_axes() {
            (sensor_h, sensor_w)
        } else {
            (sensor_w, sensor_h)
        }
    }

    pub fn rotate_left(&mut self) {
        self.quarter_turns = (self.quarter_turns + 3) % 4;
        self.crop = Crop::default();
    }

    pub fn rotate_right(&mut self) {
        self.quarter_turns = (self.quarter_turns + 1) % 4;
        self.crop = Crop::default();
    }

    pub fn flip_h(&mut self) {
        self.mirror_h = !self.mirror_h;
        if !self.crop.is_full_frame() {
            let c = self.crop.sanitised();
            self.crop.x = (1.0 - (c.x + c.width)).clamp(0.0, 1.0);
        }
    }

    pub fn flip_v(&mut self) {
        self.mirror_v = !self.mirror_v;
        if !self.crop.is_full_frame() {
            let c = self.crop.sanitised();
            self.crop.y = (1.0 - (c.y + c.height)).clamp(0.0, 1.0);
        }
    }

    pub fn is_identity(&self) -> bool {
        self.quarter_turns == 0
            && !self.mirror_h
            && !self.mirror_v
            && self.straighten_deg == 0.0
            && self.crop.is_full_frame()
    }
}

/// Highlights and shadows tone compression / recovery.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tonal {
    /// -1.0 compresses/recovers bright highlights; +1.0 boosts them.
    pub highlights: f32,
    /// +1.0 lifts deep shadows; -1.0 deepens them.
    pub shadows: f32,
    /// Fine adjustments to extreme white clip.
    pub whites: f32,
    /// Fine adjustments to shadow black floor.
    pub blacks: f32,
}

impl Default for Tonal {
    fn default() -> Self {
        Self {
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
        }
    }
}

/// Color vibrancy and chroma saturation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vibrance {
    /// Smart saturation boosting muted tones more than already saturated tones.
    pub vibrance: f32,
    /// Uniform chroma scaling.
    pub saturation: f32,
}

impl Default for Vibrance {
    fn default() -> Self {
        Self {
            vibrance: 0.0,
            saturation: 0.0,
        }
    }
}

/// Lens falloff and optical vignetting control.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    /// Negative darkens corners; positive lightens them to correct lens falloff.
    pub amount: f32,
    /// Normalized radius where falloff begins.
    pub radius: f32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            amount: 0.0,
            radius: 0.8,
        }
    }
}

/// Capture unsharp masking / detail clarity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sharpen {
    /// Amount of high-frequency detail boost.
    pub amount: f32,
}

impl Default for Sharpen {
    fn default() -> Self {
        Self { amount: 0.0 }
    }
}

/// One module: its parameters plus whether it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module<T> {
    pub enabled: bool,
    pub params: T,
}

impl<T> Module<T> {
    pub fn new(enabled: bool, params: T) -> Self {
        Self { enabled, params }
    }
}

impl<T: Default> Default for Module<T> {
    fn default() -> Self {
        Self {
            enabled: false,
            params: T::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhotoStack {
    /// Not a `Module`: there is no meaningful "disabled" state for orientation, and a
    /// checkbox that silently un-rotates a photograph would be a trap.
    pub geometry: Geometry,
    pub white_balance: Module<WhiteBalance>,
    pub exposure: Module<Exposure>,
    #[serde(default)]
    pub tonal: Module<Tonal>,
    pub tone_curve: Module<Vec<CurveNode>>,
    pub filmic: Module<Filmic>,
    pub color_balance: Module<ColorBalance>,
    #[serde(default)]
    pub vibrance: Module<Vibrance>,
    #[serde(default)]
    pub vignette: Module<Vignette>,
    #[serde(default)]
    pub sharpen: Module<Sharpen>,
}

impl Default for PhotoStack {
    fn default() -> Self {
        Self {
            geometry: Geometry::default(),
            white_balance: Module::new(
                true,
                WhiteBalance {
                    tint_r: 1.0,
                    tint_b: 1.0,
                },
            ),
            exposure: Module::new(
                true,
                Exposure {
                    ev: 0.0,
                    black: 0.0,
                },
            ),
            tonal: Module::new(false, Tonal::default()),
            tone_curve: Module::new(false, identity_nodes()),
            filmic: Module::new(
                false,
                Filmic {
                    white_ev: 3.5,
                    black_ev: -8.0,
                    latitude: 20.0,
                    contrast: 1.1,
                },
            ),
            color_balance: Module::new(
                false,
                ColorBalance {
                    lift: Rgb::splat(0.0),
                    gamma: Rgb::splat(0.0),
                    gain: Rgb::splat(0.0),
                    offset: Rgb::splat(0.0),
                },
            ),
            vibrance: Module::new(false, Vibrance::default()),
            vignette: Module::new(false, Vignette::default()),
            sharpen: Module::new(false, Sharpen::default()),
        }
    }
}

pub(crate) mod flags {
    pub const WB: u32 = 1;
    pub const EXPOSURE: u32 = 2;
    pub const CURVE: u32 = 4;
    pub const FILMIC: u32 = 8;
    pub const BALANCE: u32 = 16;
    pub const TONAL: u32 = 32;
    pub const VIBRANCE: u32 = 64;
    pub const VIGNETTE: u32 = 128;
    pub const SHARPEN: u32 = 256;
}

impl PhotoStack {
    pub(crate) fn bitmask(&self) -> u32 {
        let mut m = 0;
        if self.white_balance.enabled {
            m |= flags::WB;
        }
        if self.exposure.enabled {
            m |= flags::EXPOSURE;
        }
        if self.tonal.enabled {
            m |= flags::TONAL;
        }
        if self.tone_curve.enabled {
            m |= flags::CURVE;
        }
        if self.filmic.enabled {
            m |= flags::FILMIC;
        }
        if self.color_balance.enabled {
            m |= flags::BALANCE;
        }
        if self.vibrance.enabled {
            m |= flags::VIBRANCE;
        }
        if self.vignette.enabled {
            m |= flags::VIGNETTE;
        }
        if self.sharpen.enabled {
            m |= flags::SHARPEN;
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_swap_the_output_axes() {
        let mut g = Geometry::default();
        assert_eq!(g.output_size(6000, 4000), (6000, 4000));
        g.rotate_right();
        assert_eq!(g.output_size(6000, 4000), (4000, 6000));
        g.rotate_right();
        assert_eq!(g.output_size(6000, 4000), (6000, 4000));
    }

    #[test]
    fn rotating_wraps_in_both_directions() {
        let mut g = Geometry::default();
        g.rotate_left();
        assert_eq!(g.quarter_turns, 3);
        g.rotate_right();
        assert_eq!(g.quarter_turns, 0);
        for _ in 0..4 {
            g.rotate_right();
        }
        assert_eq!(g.quarter_turns, 0);
    }

    #[test]
    fn rotating_resets_the_crop() {
        // The crop lives in the rotated frame, so keeping it across a turn would move it
        // to a different part of the picture.
        let mut g = Geometry {
            crop: Crop {
                x: 0.1,
                y: 0.2,
                width: 0.5,
                height: 0.5,
            },
            ..Default::default()
        };
        g.rotate_right();
        assert!(g.crop.is_full_frame());
    }

    #[test]
    fn a_crop_is_clamped_into_the_frame() {
        let c = Crop {
            x: 0.9,
            y: -0.5,
            width: 0.8,
            height: 2.0,
        }
        .sanitised();
        assert!(c.x >= 0.0 && c.y >= 0.0);
        assert!(c.x + c.width <= 1.0 + 1e-6);
        assert!(c.y + c.height <= 1.0 + 1e-6);
    }

    #[test]
    fn a_degenerate_crop_never_reaches_zero_area() {
        let c = Crop {
            x: 0.5,
            y: 0.5,
            width: 0.0,
            height: 0.0,
        }
        .sanitised();
        assert!(c.width > 0.0 && c.height > 0.0);
        let (_, _, w, h) = c.to_pixels(6000, 4000);
        assert!(w >= 1 && h >= 1);
    }

    #[test]
    fn crop_pixels_stay_inside_the_image() {
        let c = Crop {
            x: 0.25,
            y: 0.5,
            width: 0.5,
            height: 0.5,
        };
        let (x, y, w, h) = c.to_pixels(6048, 4024);
        assert_eq!((x, y), (1512, 2012));
        assert!(x + w <= 6048);
        assert!(y + h <= 4024);
    }

    #[test]
    fn identity_geometry_is_recognised() {
        let mut g = Geometry::default();
        assert!(g.is_identity());
        g.mirror_h = true;
        assert!(!g.is_identity());
    }

    #[test]
    fn flip_h_and_v_toggle_and_mirror_crop() {
        let mut g = Geometry {
            crop: Crop {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.4,
            },
            ..Default::default()
        };
        g.flip_h();
        assert!(g.mirror_h);
        // Crop x was 0.1, width 0.3, right was 0.4 -> new x should be 1.0 - 0.4 = 0.6
        assert!((g.crop.x - 0.6).abs() < 1e-5);
        assert!((g.crop.width - 0.3).abs() < 1e-5);

        g.flip_h();
        assert!(!g.mirror_h);
        assert!((g.crop.x - 0.1).abs() < 1e-5);

        g.flip_v();
        assert!(g.mirror_v);
        // Crop y was 0.2, height 0.4, bottom was 0.6 -> new y should be 1.0 - 0.6 = 0.4
        assert!((g.crop.y - 0.4).abs() < 1e-5);
        assert!((g.crop.height - 0.4).abs() < 1e-5);

        g.flip_v();
        assert!(!g.mirror_v);
        assert!((g.crop.y - 0.2).abs() < 1e-5);
    }

    #[test]
    fn new_stack_modules_affect_bitmask() {
        let mut s = PhotoStack::default();
        let base_mask = s.bitmask();
        s.tonal.enabled = true;
        assert_eq!(s.bitmask(), base_mask | flags::TONAL);
        s.vibrance.enabled = true;
        assert_eq!(s.bitmask(), base_mask | flags::TONAL | flags::VIBRANCE);
        s.vignette.enabled = true;
        assert_eq!(
            s.bitmask(),
            base_mask | flags::TONAL | flags::VIBRANCE | flags::VIGNETTE
        );
        s.sharpen.enabled = true;
        assert_eq!(
            s.bitmask(),
            base_mask | flags::TONAL | flags::VIBRANCE | flags::VIGNETTE | flags::SHARPEN
        );
    }
}

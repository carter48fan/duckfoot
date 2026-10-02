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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExposureMode {
    /// Pure linear physical gain (C_out = C_in * 2^EV).
    /// Highlights stay crisp and punchy with no automatic compression (Lightroom Classic behavior).
    #[default]
    ClassicLinear,
    /// Smooth highlight compression knee.
    /// Automatically rolls off bright highlights as exposure increases (Modern Lightroom CC behavior).
    ModernSoft,
    /// Organic S-curve highlight latitude matching traditional analog negative film stocks.
    FilmicShoulder,
    /// Custom user-defined roll-off and knee threshold.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    pub ev: f32,
    pub black: f32,
    #[serde(default)]
    pub mode: ExposureMode,
    /// Highlight shoulder compression (0.0 = linear / Classic, 0.5 = Modern, 1.0 = Filmic).
    #[serde(default)]
    pub rolloff: f32,
    /// Toggle between standard +/-4 EV and studio extended +/-8 EV range.
    #[serde(default)]
    pub extended_range: bool,
}

impl Default for Exposure {
    fn default() -> Self {
        Self {
            ev: 0.0,
            black: 0.0,
            mode: ExposureMode::ClassicLinear,
            rolloff: 0.0,
            extended_range: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Filmic {
    pub white_ev: f32,
    pub black_ev: f32,
    /// Width of the near-linear midtone region, as a **percentage of the log range**.
    ///
    /// The shader divides the normalised log position by `latitude / 100`, so this is the
    /// sigmoid's softness: small values give a steep S-curve, large values flatten it.
    /// The curve is renormalised so `black_ev` always lands on black, `white_ev` on white
    /// and 0.18 on mid grey (adjust.wgsl `filmic_curve`), so a large latitude flattens the
    /// S toward a plain log curve rather than squeezing the picture into grey.
    pub latitude: f32,
    pub contrast: f32,
}

impl Filmic {
    /// The display value (sRGB-encoded, 0..=1) the adjust shader's filmic curve gives a
    /// scene-linear value — the same curve as `filmic_curve` in adjust.wgsl, for the views
    /// that draw it. Kept beside the shader's by a test on both.
    pub fn display_value(&self, v: f32) -> f32 {
        const MID_GREY: f32 = 0.18;
        // Mid grey as it shows with filmic off: 118/255.
        const MID_GREY_DISPLAY: f32 = 0.46135;
        let sigmoid = |x: f32| 1.0 / (1.0 + (-x).exp());
        let black_ev = self.black_ev;
        let range = (self.white_ev - black_ev).max(0.01);
        let k = self.contrast.max(0.01) / (self.latitude / 100.0).max(0.01);
        let x_grey = ((MID_GREY.log2() - black_ev) / range).clamp(0.001, 0.999);
        let s0 = sigmoid(-k * x_grey);
        let s1 = sigmoid(k * (1.0 - x_grey));
        let span = (s1 - s0).max(1e-6);
        let x = ((v.max(1e-10).log2() - black_ev) / range).clamp(0.0, 1.0);
        let y = ((sigmoid(k * (x - x_grey)) - s0) / span).clamp(0.0, 1.0);
        let y_grey = ((0.5 - s0) / span).clamp(1e-4, 1.0 - 1e-4);
        let power = (MID_GREY_DISPLAY.log2() / y_grey.log2()).clamp(0.2, 5.0);
        y.max(1e-7).powf(power)
    }
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
    /// Keystone, -100..=100. Positive vertical widens the top: lines that lean in toward
    /// the top of the frame, as a building's do when shot looking up, stand straight.
    #[serde(default)]
    pub perspective_v: f32,
    /// Positive horizontal widens the right side, for a wall shot at an angle from the left.
    #[serde(default)]
    pub perspective_h: f32,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            quarter_turns: 0,
            mirror_h: false,
            mirror_v: false,
            straighten_deg: 0.0,
            crop: Crop::default(),
            perspective_v: 0.0,
            perspective_h: 0.0,
        }
    }
}

/// A keystone correction as the shader applies it: a homography from the output (centred,
/// in units of half its long side) to the frame before straightening, with the zoom that
/// keeps every corner of the frame on the photo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perspective {
    /// Row-major, acting on (x, y, 1).
    pub m: [[f32; 3]; 3],
    /// Pixels per unit: half the output's long side.
    pub unit: f32,
}

impl Perspective {
    /// Where the output point `p` (pixels from the frame's centre) is taken from.
    pub fn source(&self, p: (f32, f32)) -> (f32, f32) {
        let (x, y) = (p.0 / self.unit, p.1 / self.unit);
        let m = &self.m;
        let w = m[2][0] * x + m[2][1] * y + m[2][2];
        let (sx, sy) = (
            (m[0][0] * x + m[0][1] * y + m[0][2]) / w,
            (m[1][0] * x + m[1][1] * y + m[1][2]) / w,
        );
        (sx * self.unit, sy * self.unit)
    }

    /// The output point that takes from `q`: [`Self::source`]'s inverse.
    pub fn output(&self, q: (f32, f32)) -> (f32, f32) {
        let inv = invert3(&self.m);
        Perspective {
            m: inv,
            unit: self.unit,
        }
        .source(q)
    }
}

fn invert3(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = if det.abs() < 1e-12 { 1.0 } else { 1.0 / det };
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * d,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * d,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * d,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * d,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * d,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * d,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * d,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * d,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * d,
        ],
    ]
}

impl Geometry {
    /// Right-angle turns swap the axes; straightening does not.
    pub fn swaps_axes(&self) -> bool {
        self.quarter_turns % 2 == 1
    }

    /// The keystone correction for an output of `out` pixels, if any is set.
    pub fn perspective(&self, out: (u32, u32)) -> Option<Perspective> {
        let finite = |v: f32| {
            if v.is_finite() {
                v.clamp(-100.0, 100.0)
            } else {
                0.0
            }
        };
        let (kv, kh) = (
            finite(self.perspective_v) / 100.0 * 0.6,
            finite(self.perspective_h) / 100.0 * 0.6,
        );
        if kv == 0.0 && kh == 0.0 {
            return None;
        }
        let unit = out.0.max(out.1) as f32 * 0.5;
        let (hx, hy) = (out.0 as f32 * 0.5 / unit, out.1 as f32 * 0.5 / unit);
        let matrix = |s: f32| [[s, 0.0, 0.0], [0.0, s, 0.0], [-kh * s, -kv * s, 1.0]];
        // The largest zoom at which all four corners still take from inside the frame.
        let fits = |s: f32| {
            let p = Perspective {
                m: matrix(s),
                unit: 1.0,
            };
            [(-hx, -hy), (hx, -hy), (-hx, hy), (hx, hy)]
                .iter()
                .all(|&c| {
                    let (x, y) = p.source(c);
                    x.abs() <= hx * 1.0001 && y.abs() <= hy * 1.0001
                })
        };
        let (mut lo, mut hi) = (0.05f32, 1.0f32);
        if fits(hi) {
            lo = hi;
        } else {
            for _ in 0..24 {
                let mid = (lo + hi) * 0.5;
                if fits(mid) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        }
        Some(Perspective {
            m: matrix(lo),
            unit,
        })
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

/// Multi-band local contrast and texture enhancement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clarity {
    /// Mid-low frequency local contrast boost (-100.0..=100.0).
    pub clarity: f32,
    /// Mid-high frequency texture boost (-100.0..=100.0).
    pub texture: f32,
}

impl Default for Clarity {
    fn default() -> Self {
        Self {
            clarity: 0.0,
            texture: 0.0,
        }
    }
}

/// Atmospheric haze reduction or enhancement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dehaze {
    /// Atmospheric haze reduction (-100.0..=100.0).
    pub amount: f32,
}

impl Default for Dehaze {
    fn default() -> Self {
        Self { amount: 0.0 }
    }
}

/// Noise reduction, as a pre-pass over the sensor (see `shaders/denoise.wgsl`). Sliders
/// are 0..=100; 0 luminance and 0 colour is none.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Denoise {
    /// How much grain is averaged out of brightness.
    pub luminance: f32,
    /// How much fine texture the luminance smoothing keeps; higher keeps more.
    pub detail: f32,
    /// How much colour noise (blotches of false colour) is averaged out.
    pub color: f32,
    /// How wide an area colour is averaged over; higher removes coarser blotches.
    pub smoothness: f32,
}

impl Default for Denoise {
    fn default() -> Self {
        Self {
            luminance: 0.0,
            detail: 50.0,
            color: 25.0,
            smoothness: 50.0,
        }
    }
}

impl Denoise {
    /// A starting point for a photo shot at `iso`: nothing but a little colour at base
    /// ISO, and about a tenth more luminance smoothing per stop from there.
    pub fn for_iso(iso: u32) -> Self {
        let stops = (iso.max(1) as f32 / 200.0).log2().max(0.0);
        Self {
            luminance: (stops * 12.0).clamp(0.0, 70.0).round(),
            color: (25.0 + stops * 8.0).clamp(25.0, 60.0).round(),
            ..Self::default()
        }
    }

    /// Whether it would change a pixel at all.
    pub fn is_neutral(&self) -> bool {
        !(self.luminance > 0.0 || self.color > 0.0)
    }
}

/// Lens corrections: the lens's profile when one is known, and manual ones on top of it.
/// Each slider is -100..=100 and 0 is none; see `lens::LensModel` for what they become.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lens {
    /// Positive straightens barrel distortion (lines bowing out), negative pincushion.
    pub distortion: f32,
    /// Positive shrinks a red image that the lens drew larger than green (red fringes on
    /// the outer side of edges toward the corners), negative the reverse (cyan).
    pub ca_red: f32,
    /// The same for blue (blue outward) against yellow.
    pub ca_blue: f32,
    /// Positive brightens the corners a lens darkened; negative darkens them.
    pub vignetting: f32,
    /// Whether the lens's profile (from the lensfun database, when one matches) is
    /// applied under the sliders. A file that corrects its own lens ignores this.
    #[serde(default = "yes")]
    pub profile: bool,
}

fn yes() -> bool {
    true
}

impl Default for Lens {
    fn default() -> Self {
        Self {
            distortion: 0.0,
            ca_red: 0.0,
            ca_blue: 0.0,
            vignetting: 0.0,
            profile: true,
        }
    }
}

/// Single color band adjustments for 8-band Color EQ.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorBand {
    /// Hue shift (-30.0..=30.0 degrees).
    pub hue: f32,
    /// Saturation adjustment (-100.0..=100.0%).
    pub saturation: f32,
    /// Luminance adjustment (-100.0..=100.0%).
    pub luminance: f32,
}

impl Default for ColorBand {
    fn default() -> Self {
        Self {
            hue: 0.0,
            saturation: 0.0,
            luminance: 0.0,
        }
    }
}

/// 8-band HSL color equalizer for fine-tuned color grading (especially portrait skin tones).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct ColorEq {
    pub red: ColorBand,
    pub orange: ColorBand,
    pub yellow: ColorBand,
    pub green: ColorBand,
    pub aqua: ColorBand,
    pub blue: ColorBand,
    pub purple: ColorBand,
    pub magenta: ColorBand,
}

impl ColorEq {
    pub fn bands_mut(&mut self) -> [(&'static str, &mut ColorBand); 8] {
        [
            ("Red", &mut self.red),
            ("Orange", &mut self.orange),
            ("Yellow", &mut self.yellow),
            ("Green", &mut self.green),
            ("Aqua", &mut self.aqua),
            ("Blue", &mut self.blue),
            ("Purple", &mut self.purple),
            ("Magenta", &mut self.magenta),
        ]
    }

    pub fn bands(&self) -> [(&'static str, &ColorBand); 8] {
        [
            ("Red", &self.red),
            ("Orange", &self.orange),
            ("Yellow", &self.yellow),
            ("Green", &self.green),
            ("Aqua", &self.aqua),
            ("Blue", &self.blue),
            ("Purple", &self.purple),
            ("Magenta", &self.magenta),
        ]
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

/// What the tone curve's axes measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CurveDomain {
    /// Linear scene light, where mid grey sits at 0.18 and half the curve's width is the
    /// top stop and a half. How curves worked before the display domain; a sidecar
    /// without the field was made this way and keeps it, so its look does not change.
    Linear,
    /// Display-encoded values (sRGB), where mid grey sits near the middle and the axes
    /// line up with the histogram drawn under the curve. Every new edit uses this.
    #[default]
    Display,
}

impl CurveDomain {
    fn legacy() -> Self {
        Self::Linear
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
    #[serde(default)]
    pub rgb_curves: Module<crate::curve::RgbCurves>,
    #[serde(default = "CurveDomain::legacy")]
    pub curve_domain: CurveDomain,
    /// Where the adjust pass works (W-09). A sidecar without it was made in linear sRGB
    /// and keeps it, so its look does not change; every new edit is wide.
    #[serde(default = "crate::gamut::WorkingSpace::legacy")]
    pub working_space: crate::gamut::WorkingSpace,
    pub filmic: Module<Filmic>,
    pub color_balance: Module<ColorBalance>,
    #[serde(default)]
    pub vibrance: Module<Vibrance>,
    #[serde(default)]
    pub vignette: Module<Vignette>,
    #[serde(default)]
    pub sharpen: Module<Sharpen>,
    #[serde(default)]
    pub clarity: Module<Clarity>,
    #[serde(default)]
    pub dehaze: Module<Dehaze>,
    #[serde(default)]
    pub color_eq: Module<ColorEq>,
    #[serde(default)]
    pub lens: Module<Lens>,
    #[serde(default)]
    pub denoise: Module<Denoise>,
    #[serde(default)]
    pub local: Module<crate::local::Locals>,
    #[serde(default)]
    pub heal: Module<crate::heal::Spots>,
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
            exposure: Module::new(true, Exposure::default()),
            tonal: Module::new(false, Tonal::default()),
            tone_curve: Module::new(false, identity_nodes()),
            rgb_curves: Module::new(false, crate::curve::RgbCurves::default()),
            curve_domain: CurveDomain::Display,
            working_space: crate::gamut::WorkingSpace::Rec2020,
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
            clarity: Module::new(false, Clarity::default()),
            dehaze: Module::new(false, Dehaze::default()),
            color_eq: Module::new(false, ColorEq::default()),
            lens: Module::new(false, Lens::default()),
            denoise: Module::new(false, Denoise::default()),
            local: Module::new(false, crate::local::Locals::default()),
            heal: Module::new(false, crate::heal::Spots::default()),
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
    pub const CLARITY: u32 = 512;
    pub const DEHAZE: u32 = 1024;
    pub const COLOR_EQ: u32 = 2048;
    pub const LENS: u32 = 4096;
    /// Not read by the adjust shader: noise reduction is a pre-pass. Here so the bitmask
    /// says everything that is on.
    pub const DENOISE: u32 = 8192;
    pub const LOCAL: u32 = 16384;
    pub const HEAL: u32 = 32768;
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
        if self.tone_curve.enabled || self.rgb_curves.enabled {
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
        if self.clarity.enabled {
            m |= flags::CLARITY;
        }
        if self.dehaze.enabled {
            m |= flags::DEHAZE;
        }
        if self.color_eq.enabled {
            m |= flags::COLOR_EQ;
        }
        if self.lens.enabled {
            m |= flags::LENS;
        }
        if self.denoise.enabled {
            m |= flags::DENOISE;
        }
        if self.local.enabled && self.local.params.masks.iter().any(|m| m.on) {
            m |= flags::LOCAL;
        }
        if self.heal.enabled && !self.heal.params.spots.is_empty() {
            m |= flags::HEAL;
        }
        m
    }
}

impl PhotoStack {
    /// This stack with anything a hand-edited or damaged sidecar could hold that the
    /// pipeline cannot — an out-of-range turn count, a degenerate crop, a runaway curve —
    /// brought back to something valid.
    pub fn sanitised(&self) -> Self {
        let mut s = self.clone();
        let g = &mut s.geometry;
        g.quarter_turns %= 4;
        g.straighten_deg = finite_or(g.straighten_deg, 0.0).clamp(-45.0, 45.0);
        g.perspective_v = finite_or(g.perspective_v, 0.0).clamp(-100.0, 100.0);
        g.perspective_h = finite_or(g.perspective_h, 0.0).clamp(-100.0, 100.0);
        g.crop = Crop {
            x: finite_or(g.crop.x, 0.0),
            y: finite_or(g.crop.y, 0.0),
            width: finite_or(g.crop.width, 1.0),
            height: finite_or(g.crop.height, 1.0),
        }
        .sanitised();

        const MAX_NODES: usize = 64;
        let nodes = &mut s.tone_curve.params;
        nodes.truncate(MAX_NODES);
        for n in nodes.iter_mut() {
            n.x = finite_or(n.x, 0.0).clamp(0.0, 1.0);
            n.y = finite_or(n.y, 0.0).clamp(0.0, 1.0);
        }
        nodes.sort_by(|a, b| a.x.total_cmp(&b.x));
        if nodes.len() < 2 {
            *nodes = crate::curve::identity_nodes();
        }
        s.local.params = s.local.params.sanitised();
        s.heal.params = s.heal.params.sanitised();
        s
    }
}

fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denoise_for_iso_grows_with_the_iso() {
        let base = Denoise::for_iso(100);
        assert_eq!((base.luminance, base.color), (0.0, 25.0));
        let high = Denoise::for_iso(3200);
        assert_eq!((high.luminance, high.color), (48.0, 57.0));
        assert!(Denoise::for_iso(204_800).luminance <= 70.0);
        assert_eq!(Denoise::for_iso(0), Denoise::for_iso(1));
    }

    #[test]
    fn a_keystone_keeps_the_frame_on_the_photo_and_inverts() {
        let mut g = Geometry::default();
        assert!(g.perspective((6000, 4000)).is_none());
        g.perspective_v = 60.0;
        let p = g.perspective((6000, 4000)).unwrap();
        // Every corner of the output takes from inside the frame.
        for c in [
            (-3000.0, -2000.0),
            (3000.0, -2000.0),
            (-3000.0, 2000.0),
            (3000.0, 2000.0),
        ] {
            let (x, y) = p.source(c);
            assert!(
                x.abs() <= 3000.5 && y.abs() <= 2000.5,
                "{c:?} -> ({x}, {y})"
            );
        }
        // Widening the top: the top row takes from a narrower strip than the bottom row.
        let top = p.source((3000.0, -2000.0)).0;
        let bottom = p.source((3000.0, 2000.0)).0;
        assert!(top < bottom * 0.9, "top {top} bottom {bottom}");
        let q = p.source((1234.0, -567.0));
        let back = p.output(q);
        assert!(
            (back.0 - 1234.0).abs() < 1e-2 && (back.1 + 567.0).abs() < 1e-2,
            "{back:?}"
        );
    }

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
        s.clarity.enabled = true;
        assert_eq!(
            s.bitmask(),
            base_mask
                | flags::TONAL
                | flags::VIBRANCE
                | flags::VIGNETTE
                | flags::SHARPEN
                | flags::CLARITY
        );
        s.dehaze.enabled = true;
        assert_eq!(
            s.bitmask(),
            base_mask
                | flags::TONAL
                | flags::VIBRANCE
                | flags::VIGNETTE
                | flags::SHARPEN
                | flags::CLARITY
                | flags::DEHAZE
        );
        s.color_eq.enabled = true;
        assert_eq!(
            s.bitmask(),
            base_mask
                | flags::TONAL
                | flags::VIBRANCE
                | flags::VIGNETTE
                | flags::SHARPEN
                | flags::CLARITY
                | flags::DEHAZE
                | flags::COLOR_EQ
        );
    }

    #[test]
    fn exposure_mode_defaults_and_serialization() {
        let exp = Exposure::default();
        assert_eq!(exp.mode, ExposureMode::ClassicLinear);
        assert_eq!(exp.rolloff, 0.0);
        assert!(!exp.extended_range);

        // Deserializing older sidecar json without mode/rolloff succeeds with defaults
        let json = r#"{"ev": 1.5, "black": 0.01}"#;
        let deserialized: Exposure = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.ev, 1.5);
        assert_eq!(deserialized.mode, ExposureMode::ClassicLinear);
        assert_eq!(deserialized.rolloff, 0.0);
    }

    #[test]
    fn curves_saved_before_the_display_domain_stay_linear() {
        let mut json = serde_json::to_value(PhotoStack::default()).unwrap();
        json.as_object_mut().unwrap().remove("curve_domain");
        json.as_object_mut().unwrap().remove("working_space");
        let old: PhotoStack = serde_json::from_value(json).unwrap();
        assert_eq!(old.curve_domain, CurveDomain::Linear);
        assert_eq!(PhotoStack::default().curve_domain, CurveDomain::Display);
        assert_eq!(old.working_space, crate::gamut::WorkingSpace::Srgb);
        assert_eq!(
            PhotoStack::default().working_space,
            crate::gamut::WorkingSpace::Rec2020
        );
    }

    #[test]
    fn filmic_on_the_cpu_lands_where_the_shader_does() {
        // The GPU test pins the shader at these values; this pins the CPU copy the
        // tonemap views draw, so the two cannot drift.
        let f = PhotoStack::default().filmic.params;
        assert!((f.display_value(0.18) * 255.0 - 118.0).abs() <= 1.5);
        assert!(f.display_value(2f32.powf(f.black_ev)) * 255.0 <= 2.0);
        assert!(f.display_value(2f32.powf(f.white_ev)) * 255.0 >= 253.0);
    }
}

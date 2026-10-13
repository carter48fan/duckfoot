//! Soft proofing against a printer's ICC profile (W-10): the preview as the exported file
//! will look once printed, shown on this screen.
//!
//! A printer profile is built from lookup tables, not a matrix, so the shader cannot apply
//! it the way it applies an RGB space. Instead the whole chain is baked on the CPU, once
//! per profile, into a 3D table the shader samples:
//!
//! ```text
//! export file's RGB ──[moxcms]──► printer (CMYK or RGB) ──[moxcms]──► linear Rec.2020
//!     ──[Screen]──► the screen's encoded RGB
//! ```
//!
//! The table's input is what the export writes — values in its colour space, encoded with
//! its curve — so the proof is of the file the printer will be sent. Its alpha marks the
//! colours the printer cannot reach, for the gamut warning.

use anyhow::{bail, Context, Result};
use moxcms::{
    ColorProfile, DataColorSpace, Layout, ProfileClass, RenderingIntent, ToneReprCurve,
    TransformOptions,
};

use crate::gamut::{compose, convert, OutputSpace, Screen, REC2020, SRGB};

/// Samples along each axis. 33 is what lcms and OCIO bake proofing tables at: fine
/// enough that trilinear interpolation between samples is invisible on photographs.
pub const SIZE: usize = 33;

/// How far (CIE ΔE 1976) a colour may move on its way through the printer before the gamut
/// warning marks it, after the printer's black point is allowed for: about where a change
/// is plain side by side.
const WARN_DELTA_E: f32 = 5.0;

/// How colours the printer cannot reach are brought in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Intent {
    /// Colours it can print are printed as they are; the rest are clipped to its gamut.
    /// Paper white is shown as white. Photoshop's default for proofing.
    #[default]
    RelativeColorimetric,
    /// The whole image is compressed into the printer's gamut, as the profile's maker
    /// chose.
    Perceptual,
}

/// A baked proof: `SIZE`³ RGBA texels, red fastest, as half-float bits ready for a 3D
/// texture. RGB is what the screen should show; A is 1 where the printer loses the colour.
#[derive(Clone, PartialEq)]
pub struct ProofTable {
    pub texels: Vec<half::f16>,
}

impl std::fmt::Debug for ProofTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProofTable({SIZE}³)")
    }
}

impl ProofTable {
    /// Bake the proof of a file exported in `export` and printed through `printer_icc`,
    /// for `screen`. The profile is a file the user picked, so a parser that panics on a
    /// malformed one costs this proof, never the app.
    pub fn bake(
        export: OutputSpace,
        printer_icc: &[u8],
        intent: Intent,
        screen: Screen,
    ) -> Result<Self> {
        std::panic::catch_unwind(|| Self::bake_unguarded(export, printer_icc, intent, screen))
            .unwrap_or_else(|_| bail!("the profile could not be read"))
    }

    fn bake_unguarded(
        export: OutputSpace,
        printer_icc: &[u8],
        intent: Intent,
        screen: Screen,
    ) -> Result<Self> {
        let printer = ColorProfile::new_from_slice(printer_icc)
            .map_err(|e| anyhow::anyhow!("{e:?}"))
            .context("reading the printer profile")?;
        let layout = match printer.color_space {
            DataColorSpace::Cmyk => Layout::Rgba,
            DataColorSpace::Rgb => Layout::Rgb,
            other => bail!("a {other:?} profile is not a printer's (CMYK or RGB) one"),
        };
        if !matches!(
            printer.profile_class,
            ProfileClass::OutputDevice | ProfileClass::DisplayDevice | ProfileClass::ColorSpace
        ) {
            bail!("a {:?} profile cannot be printed to", printer.profile_class);
        }
        let source = ColorProfile::new_from_slice(export.icc())
            .map_err(|e| anyhow::anyhow!("{e:?}"))
            .context("reading the export's own profile")?;
        let linear = linear_rec2020();
        let options = |intent| TransformOptions {
            rendering_intent: intent,
            // Fixed point is the default for speed; a table baked once can afford floats.
            prefer_fixed_point: false,
            ..Default::default()
        };
        let printed = match intent {
            Intent::RelativeColorimetric => RenderingIntent::RelativeColorimetric,
            Intent::Perceptual => RenderingIntent::Perceptual,
        };
        let cms = |e: moxcms::CmsError| anyhow::anyhow!("{e:?}");
        let to_printer = source
            .create_transform_f32(Layout::Rgb, &printer, layout, options(printed))
            .map_err(cms)
            .context("export → printer")?;
        // Back from ink to light: relative colorimetric, so what was printed is shown as it
        // is, with paper white as the screen's white.
        let from_printer = printer
            .create_transform_f32(
                layout,
                &linear,
                Layout::Rgb,
                options(RenderingIntent::RelativeColorimetric),
            )
            .map_err(cms)
            .context("printer → screen")?;
        let unprinted = source
            .create_transform_f32(
                Layout::Rgb,
                &linear,
                Layout::Rgb,
                options(RenderingIntent::RelativeColorimetric),
            )
            .map_err(cms)
            .context("export → screen")?;

        let n = SIZE * SIZE * SIZE;
        let step = 1.0 / (SIZE - 1) as f32;
        let mut grid = Vec::with_capacity(n * 3);
        for b in 0..SIZE {
            for g in 0..SIZE {
                for r in 0..SIZE {
                    grid.extend([r as f32 * step, g as f32 * step, b as f32 * step]);
                }
            }
        }
        let mut ink = vec![0.0f32; n * layout.channels()];
        to_printer.transform(&grid, &mut ink).map_err(cms)?;
        let mut printed = vec![0.0f32; n * 3];
        from_printer.transform(&ink, &mut printed).map_err(cms)?;
        let mut wanted = vec![0.0f32; n * 3];
        unprinted.transform(&grid, &mut wanted).map_err(cms)?;

        // The printer's black: the darkest it shows a neutral, which the warning allows for
        // (relative colorimetric maps paper white to white but cannot go below its black).
        let black_l = {
            let mut black = [0.0f32; 3];
            let mut k = vec![0.0f32; layout.channels()];
            to_printer.transform(&[0.0; 3], &mut k).map_err(cms)?;
            from_printer.transform(&k, &mut black).map_err(cms)?;
            lab(black)[0].clamp(0.0, 50.0)
        };

        let to_screen = compose(&screen.from_srgb, &convert(&REC2020, &SRGB));
        let encode = |v: f32| match screen.gamma {
            Some(g) => v.max(0.0).powf(1.0 / g),
            None => linear_to_srgb(v),
        };
        let mut texels = Vec::with_capacity(n * 4);
        for (p, w) in printed.chunks_exact(3).zip(wanted.chunks_exact(3)) {
            let p = [p[0], p[1], p[2]];
            let shown = mul(&to_screen, p).map(|v| encode(v.clamp(0.0, 1.0)));
            let (lp, mut lw) = (lab(p), lab([w[0], w[1], w[2]]));
            // Relative colorimetric keeps every lightness the printer reaches and clips
            // the rest to its black, which is the paper's limit rather than a lost colour.
            lw[0] = lw[0].max(black_l);
            let de = ((lp[0] - lw[0]).powi(2) + (lp[1] - lw[1]).powi(2) + (lp[2] - lw[2]).powi(2))
                .sqrt();
            let warn = if de > WARN_DELTA_E { 1.0 } else { 0.0 };
            texels.extend(
                [shown[0], shown[1], shown[2], warn]
                    .into_iter()
                    .map(half::f16::from_f32),
            );
        }
        Ok(Self { texels })
    }

    #[doc(hidden)]
    pub fn fuzz_bake(printer_icc: &[u8]) -> Result<Self> {
        Self::bake_unguarded(
            OutputSpace::Srgb,
            printer_icc,
            Intent::default(),
            Screen::SRGB,
        )
    }

    /// A table from a function of the encoded input, for tests of the shader's sampling.
    #[doc(hidden)]
    pub fn from_fn(f: impl Fn([f32; 3]) -> [f32; 4]) -> Self {
        let step = 1.0 / (SIZE - 1) as f32;
        let mut texels = Vec::with_capacity(SIZE * SIZE * SIZE * 4);
        for b in 0..SIZE {
            for g in 0..SIZE {
                for r in 0..SIZE {
                    let v = f([r as f32 * step, g as f32 * step, b as f32 * step]);
                    texels.extend(v.into_iter().map(half::f16::from_f32));
                }
            }
        }
        Self { texels }
    }

    /// The texel nearest `rgb` (encoded, 0..1): what the screen shows and the warning.
    pub fn at(&self, rgb: [f32; 3]) -> [f32; 4] {
        let i = rgb.map(|v| (v.clamp(0.0, 1.0) * (SIZE - 1) as f32).round() as usize);
        let at = ((i[2] * SIZE + i[1]) * SIZE + i[0]) * 4;
        std::array::from_fn(|c| self.texels[at + c].to_f32())
    }
}

/// Linear-light Rec.2020: wide enough that nothing a printer holds goes negative on its way
/// to the screen.
fn linear_rec2020() -> ColorProfile {
    let mut p = ColorProfile::new_bt2020();
    let one = || Some(ToneReprCurve::Parametric(vec![1.0]));
    p.red_trc = one();
    p.green_trc = one();
    p.blue_trc = one();
    // The preset carries a CICP tag naming BT.2020's own transfer, which moxcms prefers over
    // the curves above when it is present; without this the "linear" profile is not.
    p.cicp = None;
    p
}

fn mul(m: &crate::gamut::Mat3, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// CIE L*a*b* (D65 white) of linear Rec.2020.
fn lab(rgb: [f32; 3]) -> [f32; 3] {
    // BT.2020 → XYZ, normalised to the white point.
    const M: crate::gamut::Mat3 = [
        [0.636_958, 0.144_617, 0.168_881],
        [0.262_700, 0.677_998, 0.059_302],
        [0.000_000, 0.028_073, 1.060_985],
    ];
    const WHITE: [f32; 3] = [0.950_47, 1.0, 1.088_83];
    let xyz = mul(&M, rgb);
    let f = |t: f32| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let [fx, fy, fz] = [0, 1, 2].map(|i| f(xyz[i] / WHITE[i]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real press profile, where the machine has one (colord ships the FOGRA set); the
    /// repository carries none, as their licences do not allow redistribution in it.
    fn press() -> Option<Vec<u8>> {
        let path = "/usr/share/color/icc/colord/FOGRA39L_coated.icc";
        let bytes = std::fs::read(path).ok();
        if bytes.is_none() {
            eprintln!("{path} is not installed: skipped");
        }
        bytes
    }

    #[test]
    fn printing_to_the_exports_own_space_changes_nothing() {
        // An "RGB printer" with the export's own profile: the table is the identity, for
        // the sRGB screen, and warns about nothing.
        let t = ProofTable::bake(
            OutputSpace::Srgb,
            OutputSpace::Srgb.icc(),
            Intent::RelativeColorimetric,
            Screen::SRGB,
        )
        .unwrap();
        for rgb in [
            [0.0, 0.0, 0.0],
            [0.5, 0.25, 0.75],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0],
        ] {
            let [r, g, b, warn] = t.at(rgb);
            for (got, want) in [r, g, b].into_iter().zip(rgb) {
                assert!(
                    (got - want).abs() < 4e-3,
                    "{rgb:?} came back as {r} {g} {b}"
                );
            }
            assert_eq!(warn, 0.0, "{rgb:?} was warned about");
        }
    }

    #[test]
    fn a_press_keeps_paper_white_and_neutrals_and_loses_saturated_blue() {
        let Some(icc) = press() else { return };
        let t = ProofTable::bake(
            OutputSpace::Srgb,
            &icc,
            Intent::RelativeColorimetric,
            Screen::SRGB,
        )
        .unwrap();
        let [r, g, b, warn] = t.at([1.0, 1.0, 1.0]);
        assert!(
            r > 0.99 && g > 0.99 && b > 0.99 && warn == 0.0,
            "paper white: {r} {g} {b}"
        );
        for v in [0.0, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9] {
            let [r, g, b, warn] = t.at([v; 3]);
            assert_eq!(warn, 0.0, "grey {v} was warned about");
            assert!(
                (r - g).abs() < 0.02 && (g - b).abs() < 0.02,
                "grey {v}: {r} {g} {b}"
            );
        }
        // Ink cannot reach sRGB's blue primary, nor its black.
        let [.., warn] = t.at([0.0, 0.0, 1.0]);
        assert_eq!(warn, 1.0, "sRGB blue was not warned about");
        let [r, ..] = t.at([0.0, 0.0, 0.0]);
        assert!(
            r > 0.05,
            "black printed as screen black ({r}), with no ink limit"
        );
    }

    #[test]
    fn the_shader_samples_the_table_at_its_size() {
        let wgsl = include_str!("shaders/adjust.wgsl");
        let want = format!("const PROOF_SIZE: f32 = {SIZE}.0;");
        assert!(wgsl.contains(&want), "adjust.wgsl does not say `{want}`");
    }

    #[test]
    fn a_profile_that_is_not_a_printers_is_refused() {
        assert!(ProofTable::bake(
            OutputSpace::Srgb,
            b"not a profile",
            Intent::RelativeColorimetric,
            Screen::SRGB
        )
        .is_err());
    }
}

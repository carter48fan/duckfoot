//! One-tap looks — sepia, mono, film and the rest — built only from the modules the
//! pipeline already has, so a look is an ordinary edit: every value it sets can be seen
//! and changed in its module afterwards, and it saves to the sidecar like any other.
//!
//! A look owns the "look" modules (tone, curve, clarity, dehaze, colour, balance,
//! vibrance, vignette) and leaves the photo's own corrections alone: crop, white balance,
//! exposure, filmic and sharpening are the photo's, not the look's.
//!
//! Strength blends from the stack the look was applied over (0) to the look itself (1),
//! so 0 gives back exactly the photo as it was.

use egui::Color32;
use photo_engine::params::{ColorBalance, ColorBand, ColorEq, Rgb, Tonal};
use photo_engine::{CurveNode, PhotoStack};

pub struct Look {
    pub name: &'static str,
    pub tip: &'static str,
    /// Two colours for the tile, to hint at what the look does before it is tapped.
    pub swatch: [Color32; 2],
    build: fn(&mut PhotoStack),
}

/// Every look, in the order the Presets sheet shows them.
pub const LOOKS: &[Look] = &[
    Look {
        name: "vivid",
        tip: "Richer colour and a little punch",
        swatch: [
            Color32::from_rgb(0xff, 0x5a, 0x36),
            Color32::from_rgb(0x1e, 0x9b, 0xff),
        ],
        build: |s| {
            vibrance(s, 0.35, 0.10);
            clarity(s, 12.0, 0.0);
            curve(s, &[(0.25, 0.22), (0.75, 0.79)]);
        },
    },
    Look {
        name: "warm",
        tip: "Golden-hour warmth",
        swatch: [
            Color32::from_rgb(0xff, 0xb3, 0x47),
            Color32::from_rgb(0xc8, 0x5a, 0x1e),
        ],
        build: |s| {
            balance(s, |b| {
                b.gain = rgb(0.07, 0.02, -0.07);
                b.lift = rgb(0.02, 0.0, -0.02);
            });
            vibrance(s, 0.10, 0.0);
        },
    },
    Look {
        name: "cool",
        tip: "Clean, blue-leaning light",
        swatch: [
            Color32::from_rgb(0x8c, 0xd2, 0xff),
            Color32::from_rgb(0x2a, 0x55, 0x9e),
        ],
        build: |s| {
            balance(s, |b| {
                b.gain = rgb(-0.06, 0.0, 0.07);
                b.lift = rgb(-0.01, 0.0, 0.02);
            });
        },
    },
    Look {
        name: "fade",
        tip: "Lifted, matte blacks and softer colour",
        swatch: [
            Color32::from_rgb(0xc9, 0xbf, 0xb5),
            Color32::from_rgb(0x6e, 0x6a, 0x72),
        ],
        build: |s| {
            curve_ends(s, 0.09, 0.95, &[(0.5, 0.53)]);
            vibrance(s, 0.0, -0.22);
        },
    },
    Look {
        name: "film",
        tip: "Gentle S-curve, soft blacks, warm highlights and cool shadows",
        swatch: [
            Color32::from_rgb(0xe8, 0xc9, 0x8a),
            Color32::from_rgb(0x2f, 0x5b, 0x5e),
        ],
        build: |s| {
            curve_ends(s, 0.05, 0.97, &[(0.25, 0.21), (0.75, 0.80)]);
            vibrance(s, 0.0, -0.10);
            balance(s, |b| {
                b.lift = rgb(-0.01, 0.01, 0.035);
                b.gain = rgb(0.035, 0.01, -0.02);
            });
            vignette(s, -0.20, 0.9);
        },
    },
    Look {
        name: "vintage",
        tip: "Faded, warm and a little worn",
        swatch: [
            Color32::from_rgb(0xd9, 0xb3, 0x7a),
            Color32::from_rgb(0x6b, 0x4a, 0x2f),
        ],
        build: |s| {
            curve_ends(s, 0.08, 0.93, &[(0.5, 0.52)]);
            vibrance(s, 0.0, -0.30);
            balance(s, |b| {
                b.gain = rgb(0.06, 0.03, -0.06);
                b.lift = rgb(0.03, 0.01, -0.02);
            });
            vignette(s, -0.30, 0.85);
        },
    },
    Look {
        name: "sepia",
        tip: "Brown-toned black and white",
        swatch: [
            Color32::from_rgb(0xd6, 0xb0, 0x80),
            Color32::from_rgb(0x4a, 0x33, 0x20),
        ],
        build: |s| {
            vibrance(s, 0.0, -1.0);
            balance(s, |b| {
                // Red over green by as much as green over blue: brown, not olive.
                b.gain = rgb(0.14, 0.0, -0.14);
                b.lift = rgb(0.04, 0.0, -0.03);
            });
            curve_ends(s, 0.04, 0.97, &[]);
            vignette(s, -0.15, 0.9);
        },
    },
    Look {
        name: "mono",
        tip: "Neutral black and white",
        swatch: [
            Color32::from_rgb(0xe0, 0xe0, 0xe0),
            Color32::from_rgb(0x30, 0x30, 0x30),
        ],
        build: |s| {
            vibrance(s, 0.0, -1.0);
            clarity(s, 10.0, 0.0);
        },
    },
    Look {
        name: "noir",
        tip: "Hard black and white with deep shadows",
        swatch: [
            Color32::from_rgb(0xf5, 0xf5, 0xf5),
            Color32::from_rgb(0x05, 0x05, 0x05),
        ],
        build: |s| {
            vibrance(s, 0.0, -1.0);
            curve(s, &[(0.25, 0.14), (0.75, 0.88)]);
            clarity(s, 25.0, 0.0);
            vignette(s, -0.35, 0.8);
        },
    },
    Look {
        name: "dramatic",
        tip: "Held highlights, opened shadows, strong local contrast",
        swatch: [
            Color32::from_rgb(0x9a, 0xa8, 0xb8),
            Color32::from_rgb(0x1a, 0x1f, 0x2a),
        ],
        build: |s| {
            tonal(s, -0.45, 0.35, 0.0, -0.1);
            clarity(s, 40.0, 10.0);
            dehaze(s, 15.0);
            vibrance(s, 0.0, -0.15);
            vignette(s, -0.30, 0.85);
        },
    },
    Look {
        name: "high contrast",
        tip: "Bright whites, deep blacks",
        swatch: [
            Color32::from_rgb(0xff, 0xff, 0xff),
            Color32::from_rgb(0x10, 0x10, 0x10),
        ],
        build: |s| {
            curve(s, &[(0.25, 0.15), (0.75, 0.87)]);
            tonal(s, 0.0, 0.0, 0.2, -0.3);
            clarity(s, 15.0, 0.0);
            vibrance(s, 0.10, 0.0);
        },
    },
    Look {
        name: "teal & orange",
        tip: "Warm skin tones against cool shadows",
        swatch: [
            Color32::from_rgb(0xff, 0x9a, 0x4a),
            Color32::from_rgb(0x1f, 0x7a, 0x80),
        ],
        build: |s| {
            balance(s, |b| {
                b.lift = rgb(-0.03, 0.01, 0.04);
                b.gain = rgb(0.05, 0.01, -0.04);
            });
            color_eq(s, |eq| {
                eq.orange.saturation = 15.0;
                eq.blue.hue = -10.0;
                eq.aqua.saturation = 10.0;
                eq.green.saturation = -30.0;
            });
            curve(s, &[(0.25, 0.22), (0.75, 0.79)]);
        },
    },
];

/// `base` with `look` at `strength` (0..=1).
pub fn apply(base: &PhotoStack, look: &Look, strength: f32) -> PhotoStack {
    let t = strength.clamp(0.0, 1.0);
    let d = PhotoStack::default();
    // The look alone: the base with its own look modules cleared, then the look's.
    let mut target = base.clone();
    target.tonal = d.tonal.clone();
    target.tone_curve = d.tone_curve.clone();
    target.clarity = d.clarity.clone();
    target.dehaze = d.dehaze.clone();
    target.color_eq = d.color_eq.clone();
    target.color_balance = d.color_balance.clone();
    target.vibrance = d.vibrance.clone();
    target.vignette = d.vignette.clone();
    (look.build)(&mut target);

    let mut out = base.clone();
    macro_rules! blend {
        ($field:ident, $lerp:expr) => {{
            let from = if base.$field.enabled {
                base.$field.params.clone()
            } else {
                d.$field.params.clone()
            };
            let to = if target.$field.enabled {
                target.$field.params.clone()
            } else {
                d.$field.params.clone()
            };
            out.$field.enabled = base.$field.enabled || target.$field.enabled;
            out.$field.params = $lerp(&from, &to, t);
        }};
    }
    blend!(tonal, lerp_tonal);
    blend!(tone_curve, lerp_curve);
    blend!(clarity, |a: &photo_engine::params::Clarity,
                     b: &photo_engine::params::Clarity,
                     t| {
        photo_engine::params::Clarity {
            clarity: mix(a.clarity, b.clarity, t),
            texture: mix(a.texture, b.texture, t),
        }
    });
    blend!(dehaze, |a: &photo_engine::params::Dehaze,
                    b: &photo_engine::params::Dehaze,
                    t| {
        photo_engine::params::Dehaze {
            amount: mix(a.amount, b.amount, t),
        }
    });
    blend!(color_eq, lerp_color_eq);
    blend!(color_balance, lerp_balance);
    blend!(vibrance, |a: &photo_engine::params::Vibrance,
                      b: &photo_engine::params::Vibrance,
                      t| {
        photo_engine::params::Vibrance {
            vibrance: mix(a.vibrance, b.vibrance, t),
            saturation: mix(a.saturation, b.saturation, t),
        }
    });
    blend!(vignette, |a: &photo_engine::params::Vignette,
                      b: &photo_engine::params::Vignette,
                      t| {
        photo_engine::params::Vignette {
            amount: mix(a.amount, b.amount, t),
            radius: mix(a.radius, b.radius, t),
        }
    });
    out
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rgb(r: f32, g: f32, b: f32) -> Rgb {
    Rgb { r, g, b }
}

fn mix_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    rgb(mix(a.r, b.r, t), mix(a.g, b.g, t), mix(a.b, b.b, t))
}

fn lerp_tonal(a: &Tonal, b: &Tonal, t: f32) -> Tonal {
    Tonal {
        highlights: mix(a.highlights, b.highlights, t),
        shadows: mix(a.shadows, b.shadows, t),
        whites: mix(a.whites, b.whites, t),
        blacks: mix(a.blacks, b.blacks, t),
    }
}

fn lerp_balance(a: &ColorBalance, b: &ColorBalance, t: f32) -> ColorBalance {
    ColorBalance {
        lift: mix_rgb(a.lift, b.lift, t),
        gamma: mix_rgb(a.gamma, b.gamma, t),
        gain: mix_rgb(a.gain, b.gain, t),
        offset: mix_rgb(a.offset, b.offset, t),
    }
}

fn lerp_color_eq(a: &ColorEq, b: &ColorEq, t: f32) -> ColorEq {
    let band = |x: &ColorBand, y: &ColorBand| ColorBand {
        hue: mix(x.hue, y.hue, t),
        saturation: mix(x.saturation, y.saturation, t),
        luminance: mix(x.luminance, y.luminance, t),
    };
    ColorEq {
        red: band(&a.red, &b.red),
        orange: band(&a.orange, &b.orange),
        yellow: band(&a.yellow, &b.yellow),
        green: band(&a.green, &b.green),
        aqua: band(&a.aqua, &b.aqua),
        blue: band(&a.blue, &b.blue),
        purple: band(&a.purple, &b.purple),
        magenta: band(&a.magenta, &b.magenta),
    }
}

/// Curves have different nodes, so blend what they do: every node of either, with its
/// height blended between the two curves at that point.
fn lerp_curve(a: &[CurveNode], b: &[CurveNode], t: f32) -> Vec<CurveNode> {
    if t <= 0.0 {
        return a.to_vec();
    }
    if t >= 1.0 {
        return b.to_vec();
    }
    let (la, lb) = (
        photo_engine::curve::build_lut(a),
        photo_engine::curve::build_lut(b),
    );
    let at =
        |lut: &[f32], x: f32| lut[(x.clamp(0.0, 1.0) * (lut.len() - 1) as f32).round() as usize];
    let mut xs: Vec<f32> = a.iter().chain(b.iter()).map(|n| n.x).collect();
    xs.sort_by(|p, q| p.total_cmp(q));
    xs.dedup_by(|p, q| (*p - *q).abs() < 0.02);
    xs.into_iter()
        .map(|x| CurveNode::new(x, mix(at(&la, x), at(&lb, x), t)))
        .collect()
}

// ── What a look is made of ──────────────────────────────────────────────────────────────

fn tonal(s: &mut PhotoStack, highlights: f32, shadows: f32, whites: f32, blacks: f32) {
    s.tonal.enabled = true;
    s.tonal.params = Tonal {
        highlights,
        shadows,
        whites,
        blacks,
    };
}

fn vibrance(s: &mut PhotoStack, vibrance: f32, saturation: f32) {
    s.vibrance.enabled = true;
    s.vibrance.params.vibrance = vibrance;
    s.vibrance.params.saturation = saturation;
}

fn clarity(s: &mut PhotoStack, clarity: f32, texture: f32) {
    s.clarity.enabled = true;
    s.clarity.params.clarity = clarity;
    s.clarity.params.texture = texture;
}

fn dehaze(s: &mut PhotoStack, amount: f32) {
    s.dehaze.enabled = true;
    s.dehaze.params.amount = amount;
}

fn vignette(s: &mut PhotoStack, amount: f32, radius: f32) {
    s.vignette.enabled = true;
    s.vignette.params.amount = amount;
    s.vignette.params.radius = radius;
}

fn balance(s: &mut PhotoStack, set: impl FnOnce(&mut ColorBalance)) {
    s.color_balance.enabled = true;
    set(&mut s.color_balance.params);
}

fn color_eq(s: &mut PhotoStack, set: impl FnOnce(&mut ColorEq)) {
    s.color_eq.enabled = true;
    set(&mut s.color_eq.params);
}

/// A curve through black, `mid` and white.
fn curve(s: &mut PhotoStack, mid: &[(f32, f32)]) {
    curve_ends(s, 0.0, 1.0, mid);
}

/// A curve from (0, `black`) through `mid` to (1, `white`): lifted blacks fade, lowered
/// whites soften.
fn curve_ends(s: &mut PhotoStack, black: f32, white: f32, mid: &[(f32, f32)]) {
    s.tone_curve.enabled = true;
    let mut nodes = vec![CurveNode::new(0.0, black)];
    nodes.extend(mid.iter().map(|&(x, y)| CurveNode::new(x, y)));
    nodes.push(CurveNode::new(1.0, white));
    s.tone_curve.params = nodes;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_blends_from_the_photo_to_the_look() {
        let mut base = PhotoStack::default();
        base.exposure.params.ev = 0.7;
        base.vibrance.enabled = true;
        base.vibrance.params.saturation = 0.2;
        let sepia = LOOKS.iter().find(|l| l.name == "sepia").unwrap();

        assert_eq!(
            apply(&base, sepia, 0.0).vibrance.params,
            base.vibrance.params
        );
        let full = apply(&base, sepia, 1.0);
        assert_eq!(full.vibrance.params.saturation, -1.0);
        assert!(full.color_balance.enabled && full.color_balance.params.gain.r > 0.0);
        // The photo's own corrections are not the look's to change.
        assert_eq!(full.exposure, base.exposure);
        assert_eq!(full.geometry, base.geometry);
        let half = apply(&base, sepia, 0.5);
        assert!((half.vibrance.params.saturation - (-0.4)).abs() < 1e-5);
    }

    #[test]
    fn every_look_stays_inside_its_controls_ranges() {
        for look in LOOKS {
            let s = apply(&PhotoStack::default(), look, 1.0);
            let t = s.tonal.params;
            for v in [t.highlights, t.shadows, t.whites, t.blacks] {
                assert!((-1.0..=1.0).contains(&v), "{}", look.name);
            }
            let v = s.vibrance.params;
            assert!((-1.0..=1.0).contains(&v.vibrance) && (-1.0..=1.0).contains(&v.saturation));
            let b = s.color_balance.params;
            for c in [b.lift, b.gamma, b.gain, b.offset] {
                for x in [c.r, c.g, c.b] {
                    assert!((-0.5..=0.5).contains(&x), "{}", look.name);
                }
            }
            assert!((-1.0..=1.0).contains(&s.vignette.params.amount));
            for n in &s.tone_curve.params {
                assert!((0.0..=1.0).contains(&n.x) && (0.0..=1.0).contains(&n.y));
            }
        }
    }
}

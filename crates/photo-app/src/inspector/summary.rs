//! What each module is doing, in words: header summaries and History labels.

use crate::*;

/// A module's current value, for its collapsed header, and whether it differs from default.
/// DESIGN.md: every control that changes the image shows its current numeric value.
pub(crate) fn summarise(
    id: InspectorSectionId,
    s: &PhotoStack,
    d: &PhotoStack,
    white: &WhiteScale,
) -> (String, bool) {
    use InspectorSectionId as S;
    let signed = |label: &str, v: f32| format!("{label} {v:+.2}");
    match id {
        S::Scope => (String::new(), false),
        S::Geometry => {
            let g = &s.geometry;
            let mut parts = Vec::new();
            if g.quarter_turns != 0 {
                parts.push(format!("{}°", u32::from(g.quarter_turns) * 90));
            }
            if g.mirror_h {
                parts.push("flip h".to_string());
            }
            if g.mirror_v {
                parts.push("flip v".to_string());
            }
            if !g.crop.is_full_frame() {
                let c = g.crop.sanitised();
                parts.push(format!(
                    "crop {:.0}×{:.0}%",
                    c.width * 100.0,
                    c.height * 100.0
                ));
            }
            if g.perspective_v != 0.0 || g.perspective_h != 0.0 {
                parts.push("keystone".to_string());
            }
            if g.straighten_deg != 0.0 {
                parts.push(format!("{:+.1}°", g.straighten_deg));
            }
            let text = if parts.is_empty() {
                "none".to_string()
            } else {
                parts.join(" · ")
            };
            (text, *g != d.geometry)
        }
        S::WhiteBalance => {
            let p = &s.white_balance.params;
            let modified = *p != d.white_balance.params;
            if !modified {
                return ("as shot".to_string(), false);
            }
            let (k, t) = white.temperature(p);
            (format!("{k:.0} K · {t:+.0} tint"), true)
        }
        S::Exposure => {
            let p = &s.exposure.params;
            let mode_tag = match p.mode {
                photo_engine::params::ExposureMode::ClassicLinear => "",
                photo_engine::params::ExposureMode::ModernSoft => " · modern",
                photo_engine::params::ExposureMode::FilmicShoulder => " · filmic",
                photo_engine::params::ExposureMode::Custom => " · custom",
            };
            (
                format!("{:+.2} EV{mode_tag}", p.ev),
                *p != d.exposure.params,
            )
        }
        S::Tonal => {
            let p = &s.tonal.params;
            let set: Vec<String> = [
                ("hl", p.highlights),
                ("sh", p.shadows),
                ("wh", p.whites),
                ("bl", p.blacks),
            ]
            .into_iter()
            .filter(|(_, v)| *v != 0.0)
            .map(|(l, v)| signed(l, v))
            .collect();
            let text = if set.is_empty() {
                "neutral".to_string()
            } else {
                set.join(" · ")
            };
            (text, *p != d.tonal.params)
        }
        S::ToneCurve => {
            let nodes = &s.tone_curve.params;
            let modified = *nodes != d.tone_curve.params;
            let text = if modified {
                format!("{} pts", nodes.len())
            } else {
                "linear".to_string()
            };
            (text, modified)
        }
        S::Clarity => {
            let p = &s.clarity.params;
            let mut parts = Vec::new();
            if p.clarity != 0.0 {
                parts.push(format!("clr {:+.0}", p.clarity));
            }
            if p.texture != 0.0 {
                parts.push(format!("tex {:+.0}", p.texture));
            }
            let text = if parts.is_empty() {
                "neutral".to_string()
            } else {
                parts.join(" · ")
            };
            (text, *p != d.clarity.params)
        }
        S::Dehaze => {
            let p = &s.dehaze.params;
            let text = if p.amount != 0.0 {
                format!("{:+.0}", p.amount)
            } else {
                "neutral".to_string()
            };
            (text, *p != d.dehaze.params)
        }
        S::ColorEq => {
            let active_bands = s
                .color_eq
                .params
                .bands()
                .iter()
                .filter(|(_, b)| b.hue != 0.0 || b.saturation != 0.0 || b.luminance != 0.0)
                .count();
            let text = if active_bands > 0 {
                format!("{active_bands} active")
            } else {
                "neutral".to_string()
            };
            (text, s.color_eq.params != d.color_eq.params)
        }
        S::Filmic3D => {
            let p = &s.filmic.params;
            (
                format!("latitude {:.0}%", p.latitude),
                *p != d.filmic.params,
            )
        }
        S::ColorBalance => {
            let p = &s.color_balance.params;
            let zero = |c: &photo_engine::params::Rgb| c.r == 0.0 && c.g == 0.0 && c.b == 0.0;
            let set: Vec<&str> = [
                ("lift", &p.lift),
                ("gamma", &p.gamma),
                ("gain", &p.gain),
                ("offset", &p.offset),
            ]
            .into_iter()
            .filter(|(_, c)| !zero(c))
            .map(|(l, _)| l)
            .collect();
            let text = if set.is_empty() {
                "neutral".to_string()
            } else {
                set.join(" · ")
            };
            (text, *p != d.color_balance.params)
        }
        S::Vibrance => {
            let p = &s.vibrance.params;
            (
                format!(
                    "{} · {}",
                    signed("vib", p.vibrance),
                    signed("sat", p.saturation)
                ),
                *p != d.vibrance.params,
            )
        }
        S::Sharpen => {
            let p = &s.sharpen.params;
            (format!("amount {:.2}", p.amount), *p != d.sharpen.params)
        }
        S::Denoise => {
            let p = &s.denoise.params;
            let text = if p.is_neutral() {
                "none".to_string()
            } else {
                format!("lum {:.0} · col {:.0}", p.luminance, p.color)
            };
            (text, *p != d.denoise.params)
        }
        S::Lens => {
            let p = &s.lens.params;
            let parts: Vec<String> = [
                ("dist", p.distortion),
                ("ca", if p.ca_red != 0.0 { p.ca_red } else { p.ca_blue }),
                ("vig", p.vignetting),
            ]
            .into_iter()
            .filter(|(_, v)| *v != 0.0)
            .map(|(name, v)| format!("{name} {v:+.0}"))
            .collect();
            let text = if parts.is_empty() {
                "neutral".to_string()
            } else {
                parts.join(" · ")
            };
            (text, *p != d.lens.params)
        }
        S::Heal => {
            let n = s.heal.params.spots.len();
            let text = match n {
                0 => "none".to_string(),
                1 => "1 spot".to_string(),
                n => format!("{n} spots"),
            };
            (text, n > 0)
        }
        S::Local => {
            let n = s.local.params.masks.len();
            let text = match n {
                0 => "none".to_string(),
                1 => "1 mask".to_string(),
                n => format!("{n} masks"),
            };
            (text, n > 0)
        }
        S::Vignette => {
            let p = &s.vignette.params;
            (
                format!("{:+.2} · r {:.2}", p.amount, p.radius),
                *p != d.vignette.params,
            )
        }
    }
}

// ── Small pieces of layout used above ───────────────────────────────────────────────────

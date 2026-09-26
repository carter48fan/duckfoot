//! What each module is doing, in words: header summaries and History labels.

use crate::*;

pub(crate) fn module_enabled(stack: &mut PhotoStack, id: InspectorSectionId) -> Option<&mut bool> {
    use InspectorSectionId as S;
    match id {
        S::Scope | S::Geometry => None,
        S::WhiteBalance => Some(&mut stack.white_balance.enabled),
        S::Exposure => Some(&mut stack.exposure.enabled),
        S::Tonal => Some(&mut stack.tonal.enabled),
        S::ToneCurve => Some(&mut stack.tone_curve.enabled),
        S::Clarity => Some(&mut stack.clarity.enabled),
        S::Dehaze => Some(&mut stack.dehaze.enabled),
        S::ColorEq => Some(&mut stack.color_eq.enabled),
        S::Filmic3D => Some(&mut stack.filmic.enabled),
        S::ColorBalance => Some(&mut stack.color_balance.enabled),
        S::Vibrance => Some(&mut stack.vibrance.enabled),
        S::Sharpen => Some(&mut stack.sharpen.enabled),
        S::Vignette => Some(&mut stack.vignette.enabled),
    }
}

pub(crate) fn module_on(stack: &PhotoStack, id: InspectorSectionId) -> bool {
    use InspectorSectionId as S;
    match id {
        S::Scope | S::Geometry => true,
        S::WhiteBalance => stack.white_balance.enabled,
        S::Exposure => stack.exposure.enabled,
        S::Tonal => stack.tonal.enabled,
        S::ToneCurve => stack.tone_curve.enabled,
        S::Clarity => stack.clarity.enabled,
        S::Dehaze => stack.dehaze.enabled,
        S::ColorEq => stack.color_eq.enabled,
        S::Filmic3D => stack.filmic.enabled,
        S::ColorBalance => stack.color_balance.enabled,
        S::Vibrance => stack.vibrance.enabled,
        S::Sharpen => stack.sharpen.enabled,
        S::Vignette => stack.vignette.enabled,
    }
}

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
        S::Vignette => {
            let p = &s.vignette.params;
            (
                format!("{:+.2} · r {:.2}", p.amount, p.radius),
                *p != d.vignette.params,
            )
        }
    }
}

/// What changed between two adjacent undo states, for the History list.
pub(crate) fn describe_change(a: &PhotoStack, b: &PhotoStack) -> String {
    let mut names: Vec<String> = Vec::new();
    let (ga, gb) = (&a.geometry, &b.geometry);
    if ga.quarter_turns != gb.quarter_turns {
        names.push("rotate".into());
    }
    if ga.mirror_h != gb.mirror_h || ga.mirror_v != gb.mirror_v {
        names.push("flip".into());
    }
    if ga.straighten_deg != gb.straighten_deg {
        names.push("straighten".into());
    }
    if ga.crop != gb.crop {
        names.push("crop".into());
    }

    fn module<T: PartialEq>(names: &mut Vec<String>, name: &str, a: &Module<T>, b: &Module<T>) {
        if a.enabled != b.enabled {
            names.push(format!("{name} {}", if b.enabled { "on" } else { "off" }));
        } else if a.params != b.params {
            names.push(name.to_string());
        }
    }
    module(
        &mut names,
        "white balance",
        &a.white_balance,
        &b.white_balance,
    );
    module(&mut names, "exposure", &a.exposure, &b.exposure);
    module(&mut names, "shadows & highlights", &a.tonal, &b.tonal);
    module(&mut names, "tone curve", &a.tone_curve, &b.tone_curve);
    module(&mut names, "filmic rgb", &a.filmic, &b.filmic);
    module(
        &mut names,
        "colour balance",
        &a.color_balance,
        &b.color_balance,
    );
    module(&mut names, "vibrance", &a.vibrance, &b.vibrance);
    module(&mut names, "sharpen", &a.sharpen, &b.sharpen);
    module(&mut names, "vignette", &a.vignette, &b.vignette);

    match names.len() {
        0 => "no change".to_string(),
        1 => names.remove(0),
        2 => names.join(" + "),
        n => format!("{n} modules"),
    }
}

// ── Small pieces of layout used above ───────────────────────────────────────────────────

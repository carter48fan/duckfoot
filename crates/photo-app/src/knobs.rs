//! Each module's main controls as values to be turned (G-03): what the phone's on-photo
//! editing chooses with a vertical swipe and changes with a horizontal one, and what ↑/↓
//! and ←/→ reach from the keyboard. The sheet's sliders stay the full set; these are the
//! ones a finger on the photo can reasonably find by name.

use photo_engine::{PhotoStack, WhiteScale};

use crate::InspectorSectionId;

/// A range worked out from the stack.
type Bounds = fn(&PhotoStack) -> (f32, f32);

/// One value of one module.
pub struct Knob {
    pub name: &'static str,
    pub lo: f32,
    pub hi: f32,
    /// Decimals the readout shows.
    pub decimals: usize,
    get: fn(&PhotoStack, &WhiteScale) -> f32,
    set: fn(&mut PhotoStack, &WhiteScale, f32),
    /// A range that depends on the stack (exposure's, with its extended range on).
    bounds: Option<Bounds>,
}

impl Knob {
    /// Its range for this stack.
    pub fn range(&self, stack: &PhotoStack) -> (f32, f32) {
        self.bounds.map_or((self.lo, self.hi), |b| b(stack))
    }

    pub fn get(&self, stack: &PhotoStack, white: &WhiteScale) -> f32 {
        (self.get)(stack, white)
    }

    /// Sets it, within its range, and switches its module on: turning a value on the photo
    /// is asking to see it.
    pub fn set(&self, id: InspectorSectionId, stack: &mut PhotoStack, white: &WhiteScale, v: f32) {
        let (lo, hi) = self.range(stack);
        (self.set)(stack, white, v.clamp(lo, hi));
        if let Some(on) = id.enabled_mut(stack) {
            *on = true;
        }
    }

    /// The value as the readout shows it: "+0.35", "5400".
    pub fn format(&self, v: f32) -> String {
        let signed = self.lo < 0.0;
        match (signed, self.decimals) {
            (true, d) => format!("{v:+.d$}"),
            (false, d) => format!("{v:.d$}"),
        }
    }
}

macro_rules! knob {
    ($name:expr, $lo:expr, $hi:expr, $dec:expr, $($path:ident).+) => {
        Knob {
            name: $name,
            lo: $lo,
            hi: $hi,
            decimals: $dec,
            get: {
                fn get(s: &PhotoStack, _: &WhiteScale) -> f32 {
                    s.$($path).+
                }
                get
            },
            set: {
                fn set(s: &mut PhotoStack, _: &WhiteScale, v: f32) {
                    s.$($path).+ = v;
                }
                set
            },
            bounds: None,
        }
    };
}

fn kelvin(s: &PhotoStack, w: &WhiteScale) -> f32 {
    w.temperature(&s.white_balance.params).0
}

fn set_kelvin(s: &mut PhotoStack, w: &WhiteScale, k: f32) {
    let (_, tint) = w.temperature(&s.white_balance.params);
    if let Some(wb) = w.white_balance(k, tint) {
        s.white_balance.params = wb;
    }
}

fn tint(s: &PhotoStack, w: &WhiteScale) -> f32 {
    w.temperature(&s.white_balance.params).1
}

fn set_tint(s: &mut PhotoStack, w: &WhiteScale, t: f32) {
    let (k, _) = w.temperature(&s.white_balance.params);
    if let Some(wb) = w.white_balance(k, t) {
        s.white_balance.params = wb;
    }
}

fn ev(s: &PhotoStack, _: &WhiteScale) -> f32 {
    s.exposure.params.ev
}

fn set_ev(s: &mut PhotoStack, _: &WhiteScale, v: f32) {
    s.exposure.params.ev = v;
}

/// ±4 EV, or ±8 with the extended range on, as the sheet's slider.
fn ev_range(s: &PhotoStack) -> (f32, f32) {
    let max = if s.exposure.params.extended_range {
        8.0
    } else {
        4.0
    };
    (-max, max)
}

const GEOMETRY: &[Knob] = &[
    knob!("straighten", -15.0, 15.0, 2, geometry.straighten_deg),
    knob!("vertical", -100.0, 100.0, 0, geometry.perspective_v),
    knob!("horizontal", -100.0, 100.0, 0, geometry.perspective_h),
];

const LENS: &[Knob] = &[
    knob!("distortion", -100.0, 100.0, 0, lens.params.distortion),
    knob!("red fringe", -100.0, 100.0, 0, lens.params.ca_red),
    knob!("blue fringe", -100.0, 100.0, 0, lens.params.ca_blue),
    knob!("vignetting", -100.0, 100.0, 0, lens.params.vignetting),
];

const DENOISE: &[Knob] = &[
    knob!("luminance", 0.0, 100.0, 0, denoise.params.luminance),
    knob!("detail", 0.0, 100.0, 0, denoise.params.detail),
    knob!("colour", 0.0, 100.0, 0, denoise.params.color),
    knob!("smoothness", 0.0, 100.0, 0, denoise.params.smoothness),
];

const WHITE_BALANCE: &[Knob] = &[
    Knob {
        name: "temp (K)",
        lo: crate::WB_KELVIN_MIN,
        hi: crate::WB_KELVIN_MAX,
        decimals: 0,
        get: kelvin,
        set: set_kelvin,
        bounds: None,
    },
    Knob {
        name: "tint",
        lo: -150.0,
        hi: 150.0,
        decimals: 0,
        get: tint,
        set: set_tint,
        bounds: None,
    },
];

const EXPOSURE: &[Knob] = &[
    Knob {
        name: "EV",
        lo: -8.0,
        hi: 8.0,
        decimals: 2,
        get: ev,
        set: set_ev,
        bounds: Some(ev_range),
    },
    knob!("black", -0.1, 0.1, 4, exposure.params.black),
];

const TONAL: &[Knob] = &[
    knob!("highlights", -1.0, 1.0, 2, tonal.params.highlights),
    knob!("shadows", -1.0, 1.0, 2, tonal.params.shadows),
    knob!("whites", -1.0, 1.0, 2, tonal.params.whites),
    knob!("blacks", -1.0, 1.0, 2, tonal.params.blacks),
];

const CLARITY: &[Knob] = &[
    knob!("clarity", -100.0, 100.0, 1, clarity.params.clarity),
    knob!("texture", -100.0, 100.0, 1, clarity.params.texture),
];

const DEHAZE: &[Knob] = &[knob!("amount", -100.0, 100.0, 1, dehaze.params.amount)];

const FILMIC: &[Knob] = &[
    knob!("white EV", 0.0, 8.0, 2, filmic.params.white_ev),
    knob!("black EV", -16.0, -1.0, 2, filmic.params.black_ev),
    knob!("latitude %", 5.0, 80.0, 1, filmic.params.latitude),
    knob!("contrast", 0.1, 3.0, 2, filmic.params.contrast),
];

const VIBRANCE: &[Knob] = &[
    knob!("vibrance", -1.0, 1.0, 2, vibrance.params.vibrance),
    knob!("saturation", -1.0, 1.0, 2, vibrance.params.saturation),
];

const SHARPEN: &[Knob] = &[knob!("amount", 0.0, 2.0, 2, sharpen.params.amount)];

const LOOK: &[Knob] = &[knob!("amount", 0.0, 1.0, 2, look.params.amount)];

const VIGNETTE: &[Knob] = &[
    knob!("amount", -1.0, 1.0, 2, vignette.params.amount),
    knob!("radius", 0.1, 1.5, 2, vignette.params.radius),
];

/// The module's knobs, in the order its card lists them. Empty for the modules whose
/// controls are not values along a line (the curve, the colour wheels and bands, masks,
/// spots, the scopes).
pub fn knobs(id: InspectorSectionId) -> &'static [Knob] {
    use InspectorSectionId as I;
    match id {
        I::Geometry => GEOMETRY,
        I::Lens => LENS,
        I::Denoise => DENOISE,
        I::WhiteBalance => WHITE_BALANCE,
        I::Exposure => EXPOSURE,
        I::Tonal => TONAL,
        I::Clarity => CLARITY,
        I::Dehaze => DEHAZE,
        I::Filmic3D => FILMIC,
        I::Vibrance => VIBRANCE,
        I::Sharpen => SHARPEN,
        I::Vignette => VIGNETTE,
        I::Look => LOOK,
        I::Scope | I::ToneCurve | I::ColorEq | I::ColorBalance | I::Local | I::Heal => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_knob_reads_back_what_it_sets_and_switches_its_module_on() {
        let white = WhiteScale::srgb();
        for &id in InspectorSectionId::all() {
            for k in knobs(id) {
                let mut stack = PhotoStack::default();
                if let Some(on) = id.enabled_mut(&mut stack) {
                    *on = false;
                }
                let want = k.lo + (k.hi - k.lo) * 0.3;
                k.set(id, &mut stack, &white, want);
                let got = k.get(&stack, &white);
                let tolerance = (k.hi - k.lo) * 0.01;
                assert!(
                    (got - want).abs() <= tolerance,
                    "{} {}: set {want}, read {got}",
                    id.title(),
                    k.name
                );
                assert!(id.is_on(&stack), "{} is on after {}", id.title(), k.name);
                // Beyond the range: held at its end.
                k.set(id, &mut stack, &white, k.hi + 1e6);
                assert!(k.get(&stack, &white) <= k.hi + tolerance);
            }
        }
    }
}

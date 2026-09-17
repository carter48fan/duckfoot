//! The adjustment stack — plain data, no GPU types.
//!
//! Fixed order, not rearrangeable. That is the line that stops the photo studio becoming
//! a node graph (DESIGN.md), and it is also precisely what lets the whole chain fuse into
//! one shader pass. The product constraint and the fast implementation are the same
//! decision.

use crate::curve::{identity_nodes, CurveNode};

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhiteBalance {
    /// Multiplies the as-shot camera neutral; 1.0 leaves the shot as metered.
    pub tint_r: f32,
    pub tint_b: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exposure {
    pub ev: f32,
    pub black: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filmic {
    pub white_ev: f32,
    pub black_ev: f32,
    pub latitude: f32,
    pub contrast: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorBalance {
    pub lift: Rgb,
    pub gamma: Rgb,
    pub gain: Rgb,
    pub offset: Rgb,
}

/// One module: its parameters plus whether it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Module<T> {
    pub enabled: bool,
    pub params: T,
}

impl<T> Module<T> {
    pub fn new(enabled: bool, params: T) -> Self {
        Self { enabled, params }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhotoStack {
    pub white_balance: Module<WhiteBalance>,
    pub exposure: Module<Exposure>,
    pub tone_curve: Module<Vec<CurveNode>>,
    pub filmic: Module<Filmic>,
    pub color_balance: Module<ColorBalance>,
}

impl Default for PhotoStack {
    fn default() -> Self {
        Self {
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
            tone_curve: Module::new(false, identity_nodes()),
            filmic: Module::new(
                false,
                Filmic {
                    white_ev: 3.5,
                    black_ev: -8.0,
                    latitude: 200.0,
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
        }
    }
}

pub(crate) mod flags {
    pub const WB: u32 = 1;
    pub const EXPOSURE: u32 = 2;
    pub const CURVE: u32 = 4;
    pub const FILMIC: u32 = 8;
    pub const BALANCE: u32 = 16;
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
        if self.tone_curve.enabled {
            m |= flags::CURVE;
        }
        if self.filmic.enabled {
            m |= flags::FILMIC;
        }
        if self.color_balance.enabled {
            m |= flags::BALANCE;
        }
        m
    }
}

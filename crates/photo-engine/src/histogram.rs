//! GPU histogram with an asynchronous readback.
//!
//! The 4 KB of bins is the only thing that comes back from the GPU while the user is
//! interacting (DESIGN.md invariant 4). Even that is never waited on: mapping is requested
//! after the dispatch and collected on a later frame, so a slider drag is never blocked on
//! a GPU sync. A stale histogram for one frame is invisible; a stall is not.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

pub const BINS: usize = 256;
const CHANNELS: usize = 4;
pub const BUFFER_BYTES: u64 = (BINS * CHANNELS * std::mem::size_of::<u32>()) as u64;

/// Counts per level, 0..=255.
#[derive(Clone)]
pub struct Histogram {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub luma: Vec<u32>,
    /// Largest count in any bin of any channel — what a plot should scale against.
    pub peak: u32,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            red: vec![0; BINS],
            green: vec![0; BINS],
            blue: vec![0; BINS],
            luma: vec![0; BINS],
            peak: 0,
        }
    }
}

impl Histogram {
    pub fn from_bins(raw: &[u32]) -> Self {
        let take = |i: usize| raw[i * BINS..(i + 1) * BINS].to_vec();
        let (red, green, blue, luma) = (take(0), take(1), take(2), take(3));
        // The extremes are excluded from the peak: a large flat region of pure black or
        // pure white would otherwise flatten the whole plot into an unreadable line.
        let peak = [&red, &green, &blue, &luma]
            .iter()
            .flat_map(|c| c[1..BINS - 1].iter())
            .copied()
            .max()
            .unwrap_or(0);
        Self {
            red,
            green,
            blue,
            luma,
            peak,
        }
    }

    /// Fraction of sampled pixels sitting at the very top of the range.
    pub fn clipped_high(&self) -> f32 {
        let total: u64 = self.luma.iter().map(|v| *v as u64).sum();
        if total == 0 {
            return 0.0;
        }
        self.luma[BINS - 1] as f32 / total as f32
    }

    pub fn clipped_low(&self) -> f32 {
        let total: u64 = self.luma.iter().map(|v| *v as u64).sum();
        if total == 0 {
            return 0.0;
        }
        self.luma[0] as f32 / total as f32
    }
}

/// Where the readback has got to. Shared with the map callback, which runs on whichever
/// thread wgpu chooses.
#[derive(Clone)]
pub(crate) struct ReadbackState(Arc<AtomicU8>);

const IDLE: u8 = 0;
const PENDING: u8 = 1;
const READY: u8 = 2;

impl ReadbackState {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU8::new(IDLE)))
    }
    pub(crate) fn is_idle(&self) -> bool {
        self.0.load(Ordering::Acquire) == IDLE
    }
    pub(crate) fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire) == READY
    }
    pub(crate) fn set_pending(&self) {
        self.0.store(PENDING, Ordering::Release);
    }
    pub(crate) fn set_ready(&self) {
        self.0.store(READY, Ordering::Release);
    }
    pub(crate) fn set_idle(&self) {
        self.0.store(IDLE, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_raw_buffer_into_four_channels() {
        let mut raw = vec![0u32; BINS * CHANNELS];
        raw[10] = 5; // red
        raw[BINS + 20] = 7; // green
        raw[2 * BINS + 30] = 9; // blue
        raw[3 * BINS + 40] = 11; // luma
        let h = Histogram::from_bins(&raw);
        assert_eq!(h.red[10], 5);
        assert_eq!(h.green[20], 7);
        assert_eq!(h.blue[30], 9);
        assert_eq!(h.luma[40], 11);
        assert_eq!(h.peak, 11);
    }

    #[test]
    fn peak_ignores_the_clipped_extremes() {
        let mut raw = vec![0u32; BINS * CHANNELS];
        raw[0] = 100_000; // a huge pure-black region
        raw[BINS - 1] = 100_000; // and pure white
        raw[128] = 42;
        let h = Histogram::from_bins(&raw);
        // Scaling to 100_000 would render everything else as a flat line.
        assert_eq!(h.peak, 42);
    }

    #[test]
    fn reports_clipping_as_a_fraction() {
        let mut raw = vec![0u32; BINS * CHANNELS];
        raw[3 * BINS] = 25; // luma bin 0
        raw[3 * BINS + BINS - 1] = 75; // luma bin 255
        let h = Histogram::from_bins(&raw);
        assert!((h.clipped_low() - 0.25).abs() < 1e-6);
        assert!((h.clipped_high() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn an_empty_histogram_reports_no_clipping() {
        let h = Histogram::default();
        assert_eq!(h.clipped_low(), 0.0);
        assert_eq!(h.clipped_high(), 0.0);
        assert_eq!(h.peak, 0);
    }
}

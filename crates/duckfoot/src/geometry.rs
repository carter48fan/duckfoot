//! Where the window was, so the next launch opens there. Desktop only.
//!
//! Kept in its own file beside `settings.json` rather than in it: the app writes that
//! file, and two writers of one file would race. The shell owns the window, so the shell
//! owns this.
//!
//! Position is in physical pixels, as winit and the monitor list both give it; size is in
//! logical pixels, so a window keeps its apparent size across a change of scale. Wayland
//! will not tell a client where its window is, so there only the size and the maximised
//! state are kept.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::window::{Window, WindowAttributes};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub position: Option<(i32, i32)>,
    pub size: (f64, f64),
    pub maximized: bool,
}

/// A monitor as the check below needs it: physical position and size.
pub type Monitor = ((i32, i32), (u32, u32));

impl Geometry {
    pub fn load(dir: &Path) -> Option<Self> {
        let bytes = std::fs::read(file(dir)).ok()?;
        let g: Self = serde_json::from_slice(&bytes).ok()?;
        // A hand-edited or corrupt file must not open a window of no size, or of
        // millions of pixels.
        let sane = |v: f64| v.is_finite() && (200.0..=20_000.0).contains(&v);
        (sane(g.size.0) && sane(g.size.1)).then_some(g)
    }

    pub fn save(&self, dir: &Path) {
        let result = std::fs::create_dir_all(dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| {
                let json = serde_json::to_vec_pretty(self)?;
                photo_engine::fsio::atomic_write_bytes(&file(dir), &json)
            });
        if let Err(e) = result {
            log::warn!("could not save the window's place: {e:#}");
        }
    }

    /// What the window is now. A maximised window keeps the size and place it had before
    /// (`previous`), so un-maximising after the next launch goes back to it.
    pub fn of(window: &Window, previous: Option<Self>) -> Self {
        let maximized = window.is_maximized();
        if maximized {
            if let Some(previous) = previous {
                return Self {
                    maximized,
                    ..previous
                };
            }
        }
        let size: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());
        Self {
            position: window.outer_position().ok().map(|p| (p.x, p.y)),
            size: (size.width, size.height),
            maximized,
        }
    }

    /// `attributes`, opened where this says, as far as the monitors there now allow.
    pub fn apply(
        &self,
        mut attributes: WindowAttributes,
        monitors: &[Monitor],
    ) -> WindowAttributes {
        attributes = attributes
            .with_inner_size(LogicalSize::new(self.size.0, self.size.1))
            .with_maximized(self.maximized);
        if let Some(position) = self.position.filter(|&p| visible_on(p, monitors)) {
            attributes = attributes.with_position(PhysicalPosition::new(position.0, position.1));
        }
        attributes
    }
}

/// Whether a window whose top-left corner is at `at` would have that corner on some
/// monitor: a monitor unplugged since the last run must not leave the window off screen.
/// The corner, with a margin, rather than the whole window: a window larger than the
/// monitor it moved to is still reachable by its title bar.
pub fn visible_on(at: (i32, i32), monitors: &[Monitor]) -> bool {
    const MARGIN: i64 = 48;
    monitors.iter().any(|&((x, y), (w, h))| {
        let (ax, ay) = (at.0 as i64, at.1 as i64);
        let (x, y, w, h) = (x as i64, y as i64, w as i64, h as i64);
        ax >= x - MARGIN && ax < x + w - MARGIN && ay >= y - MARGIN && ay < y + h - MARGIN
    })
}

fn file(dir: &Path) -> PathBuf {
    dir.join("window.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_opens_only_where_a_monitor_is() {
        let monitors = [((0, 0), (1920, 1080)), ((1920, 0), (2560, 1440))];
        assert!(visible_on((100, 100), &monitors));
        assert!(visible_on((2000, 50), &monitors));
        // Just above the screen, as some window managers place a title bar.
        assert!(visible_on((100, -20), &monitors));
        // On a monitor no longer there.
        assert!(!visible_on((5000, 100), &monitors));
        assert!(!visible_on((100, 2000), &monitors));
        assert!(!visible_on((100, 100), &[]));
    }

    #[test]
    fn a_corrupt_size_is_not_used() {
        let dir = std::env::temp_dir().join(format!("duckfoot-geometry-{}", std::process::id()));
        Geometry {
            position: None,
            size: (0.0, f64::NAN),
            maximized: false,
        }
        .save(&dir);
        assert!(Geometry::load(&dir).is_none());
        let good = Geometry {
            position: Some((10, 20)),
            size: (1200.0, 800.0),
            maximized: true,
        };
        good.save(&dir);
        assert_eq!(Geometry::load(&dir), Some(good));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

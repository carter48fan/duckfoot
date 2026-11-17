//! Masks found in the photo: the subject (by the bundled U²-Net model) or the sky (by a
//! heuristic). The photo is read back once, small, on the UI thread; finding runs on a
//! thread of its own, and the mask lands as one undo step when it is done.

use crate::*;
use photo_engine::local::{Mask, Matte, MatteSource, MATTE_SIDE, MAX_MASKS};
use std::sync::mpsc;

/// A mask being found.
pub(crate) struct Finding {
    path: PathBuf,
    source: MatteSource,
    rx: mpsc::Receiver<Result<Matte, String>>,
}

impl PhotoApp {
    /// Starts finding `source` in the open photo.
    /// The frame loop polls every 100 ms while it runs (see `update`).
    pub(crate) fn find_mask(&mut self, gpu: &Gpu, source: MatteSource) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        if self.finding.is_some() || self.stack.local.params.masks.len() >= MAX_MASKS {
            return;
        }
        let seen = match self
            .engine
            .mask_source(gpu.device, gpu.queue, &self.stack, MATTE_SIDE)
        {
            Ok(img) => img,
            Err(e) => {
                self.set_status(format!("Could not read the photo to find a mask: {e:#}"));
                return;
            }
        };
        // Found upright (the model knows people standing up), stored as the sensor is.
        let turns = self.stack.geometry.quarter_turns % 4;
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("duckfoot-find-mask".into())
            .spawn(move || {
                let found = std::panic::catch_unwind(|| find(&seen, turns, source))
                    .unwrap_or_else(|_| Err("finding the mask crashed".into()));
                let _ = tx.send(found);
            });
        if spawned.is_err() {
            self.set_status("Could not start finding the mask");
            return;
        }
        self.set_status(match source {
            MatteSource::Subject => "Finding the subject…",
            MatteSource::Sky => "Finding the sky…",
        });
        self.finding = Some(Finding { path, source, rx });
    }

    /// A mask is being found.
    pub(crate) fn finding_mask(&self) -> bool {
        self.finding.is_some()
    }

    /// Takes a found mask in, as one undo step, if it is for the photo still open.
    pub(crate) fn poll_found_mask(&mut self) {
        let Some(f) = &self.finding else { return };
        let result = match f.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("the mask finder stopped".into()),
        };
        let Some(f) = self.finding.take() else { return };
        if self.loaded_path.as_ref() != Some(&f.path) {
            return;
        }
        match result {
            Ok(matte) => {
                let n = self.stack.local.params.masks.len();
                if n >= MAX_MASKS {
                    return;
                }
                self.edit(|s| {
                    s.local.params.masks.push(Mask::matte(matte));
                    s.local.enabled = true;
                });
                self.selected_mask = Some(n);
                self.set_status(match f.source {
                    MatteSource::Subject => "Subject found: set what the mask does",
                    MatteSource::Sky => {
                        "Sky found (a guess): tidy it with a range or invert if need be"
                    }
                });
            }
            Err(e) => self.set_status(format!("No mask found: {e}")),
        }
    }
}

/// The matte for `source` in `seen` (sensor orientation), found with the photo turned
/// `turns` quarters clockwise upright.
fn find(seen: &image::RgbImage, turns: u8, source: MatteSource) -> Result<Matte, String> {
    use photo_engine::sky::turned;
    let upright = turned(seen, turns);
    let (w, h) = upright.dimensions();
    let coverage = match source {
        MatteSource::Subject => {
            let session = photo_engine::rembg::RembgSession::auto()
                .map_err(|e| format!("the subject model would not load: {e}"))?;
            let pred = session
                .raw_prediction_320(&image::DynamicImage::ImageRgb8(upright))
                .map_err(|e| format!("{e}"))?;
            photo_engine::rembg::RembgSession::upscale_prediction(&pred, w, h)
        }
        MatteSource::Sky => photo_engine::sky::sky_coverage(&upright),
    };
    if coverage.pixels().all(|p| p.0[0] < 8) {
        return Err(match source {
            MatteSource::Subject => "no subject stood out".into(),
            MatteSource::Sky => "no sky reached the top of the frame".into(),
        });
    }
    let back = turned(&coverage, (4 - turns) % 4);
    Matte::new(source, &back).ok_or_else(|| "the mask could not be stored".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bright subject on a plain ground is found, and the matte comes back in the
    /// sensor's orientation even when the photo was found upright from a turned one.
    #[test]
    fn a_subject_and_a_sky_are_found_and_stored_as_the_sensor_is() {
        // A tall subject, as it stands upright.
        let upright = image::RgbImage::from_fn(120, 200, |x, y| {
            let inside = (35..85).contains(&x) && (40..180).contains(&y);
            image::Rgb(if inside {
                [220, 160, 40]
            } else {
                [245, 245, 245]
            })
        });
        // The sensor holds it turned a quarter anticlockwise; the edit turns it back.
        let sensor = photo_engine::sky::turned(&upright, 3);
        assert_eq!(sensor.dimensions(), (200, 120));
        let matte = find(&sensor, 1, MatteSource::Subject).expect("found");
        assert_eq!((matte.width, matte.height), (200, 120));
        let cover = matte.coverage().unwrap();
        // The subject's middle, in sensor coordinates, against a corner.
        let (mx, my) = (110, 60);
        assert!(
            cover.get_pixel(mx, my).0[0] > cover.get_pixel(5, 5).0[0],
            "the middle is not more subject than the corner"
        );

        let sky = image::RgbImage::from_fn(160, 100, |_, y| {
            image::Rgb(if y < 40 {
                [100, 150, 230]
            } else {
                [70, 100, 50]
            })
        });
        let matte = find(&sky, 0, MatteSource::Sky).expect("found");
        let cover = matte.coverage().unwrap();
        assert!(cover.get_pixel(80, 10).0[0] > 200);
        assert!(cover.get_pixel(80, 90).0[0] < 30);
    }
}

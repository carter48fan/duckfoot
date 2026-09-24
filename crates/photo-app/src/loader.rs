//! Decoding off the UI thread: the photo being opened, and filmstrip thumbnails.
//!
//! A 24 MP RAW takes ~100 ms to decode. On the UI thread that is a visible freeze every
//! time you step through a roll, so both jobs run on worker threads and hand their results
//! back over a channel. The GPU upload stays on the UI thread — it has to, the device is
//! the UI's — but it is a copy, not a decode.
//!
//! Thumbnails are built on the CPU straight from the mosaic or embedded fast JPEG preview:
//! a block average per colour, the camera's own white balance and matrix, and the sRGB curve.
//! No GPU readback is involved (DESIGN.md invariant 4), and none of the user's edits are applied — a
//! thumbnail shows the file as shot. Only the open photo's tile shows the live render.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use photo_engine::{
    decode_raster, extract_embedded_thumbnail, is_raster_path, CfaImage, Geometry, RasterImage,
    RawDecoder, RawloaderBackend,
};

/// Long side of a thumbnail, in pixels. A tile is 112×70 points; this covers it at 2×.
const THUMB_SIDE: usize = 256;

pub enum LoadedSource {
    Raw(CfaImage),
    Raster(RasterImage),
}

impl LoadedSource {
    pub fn orientation(&self) -> Geometry {
        match self {
            Self::Raw(c) => c.orientation,
            Self::Raster(r) => r.orientation,
        }
    }
}

pub enum Done {
    Opened {
        path: PathBuf,
        /// Matches the request that asked for it; anything older is stale and dropped.
        generation: u64,
        result: Result<LoadedSource, String>,
        decode_ms: f32,
    },
    Thumbnail {
        path: PathBuf,
        image: egui::ColorImage,
    },
}

pub struct Loader {
    open_tx: Sender<(PathBuf, u64)>,
    thumb_tx: Sender<PathBuf>,
    pub done: Receiver<Done>,
    /// Set on the first frame; lets a worker wake the UI when its result lands, since
    /// the event loop otherwise sleeps until the next input event.
    repaint: Arc<Mutex<Option<egui::Context>>>,
}

impl Loader {
    pub fn new() -> Self {
        let (done_tx, done) = mpsc::channel();
        let repaint: Arc<Mutex<Option<egui::Context>>> = Arc::default();

        let (open_tx, open_rx) = mpsc::channel::<(PathBuf, u64)>();
        {
            let done_tx = done_tx.clone();
            let repaint = repaint.clone();
            std::thread::Builder::new()
                .name("duckfoot-open".into())
                .spawn(move || {
                    while let Ok(mut job) = open_rx.recv() {
                        // Stepping quickly through a roll queues several opens; only the
                        // last one is still wanted.
                        while let Ok(newer) = open_rx.try_recv() {
                            job = newer;
                        }
                        let (path, generation) = job;
                        let started = Instant::now();

                        let result: Result<LoadedSource, String> = if is_raster_path(&path) {
                            fs::read(&path)
                                .map_err(|e| format!("{e:#}"))
                                .and_then(|bytes| {
                                    decode_raster(&bytes)
                                        .map(LoadedSource::Raster)
                                        .map_err(|e| format!("{e:#}"))
                                })
                        } else {
                            RawloaderBackend
                                .decode_file(&path)
                                .map(LoadedSource::Raw)
                                .map_err(|e| format!("{e:#}"))
                        };

                        let decode_ms = started.elapsed().as_secs_f32() * 1000.0;
                        let _ = done_tx.send(Done::Opened {
                            path,
                            generation,
                            result,
                            decode_ms,
                        });
                        wake(&repaint);
                    }
                })
                .expect("could not start the decode thread");
        }

        let (thumb_tx, thumb_rx) = mpsc::channel::<PathBuf>();
        {
            let repaint = repaint.clone();
            std::thread::Builder::new()
                .name("duckfoot-thumbs".into())
                .spawn(move || {
                    while let Ok(path) = thumb_rx.recv() {
                        if let Some(image) = generate_thumbnail(&path) {
                            let _ = done_tx.send(Done::Thumbnail { path, image });
                            wake(&repaint);
                        }
                    }
                })
                .expect("could not start the thumbnail thread");
        }

        Self {
            open_tx,
            thumb_tx,
            done,
            repaint,
        }
    }

    pub fn set_context(&self, ctx: &egui::Context) {
        if let Ok(mut slot) = self.repaint.lock() {
            if slot.is_none() {
                *slot = Some(ctx.clone());
            }
        }
    }

    pub fn open(&self, path: PathBuf, generation: u64) {
        let _ = self.open_tx.send((path, generation));
    }

    pub fn thumbnail(&self, path: PathBuf) {
        let _ = self.thumb_tx.send(path);
    }
}

fn wake(repaint: &Mutex<Option<egui::Context>>) {
    if let Ok(slot) = repaint.lock() {
        if let Some(ctx) = slot.as_ref() {
            ctx.request_repaint();
        }
    }
}

fn generate_thumbnail(path: &Path) -> Option<egui::ColorImage> {
    if is_raster_path(path) {
        if let Ok(bytes) = fs::read(path) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                return Some(downsample_to_thumb(img));
            }
        }
    } else {
        // First try fast embedded thumbnail extraction (< 2ms)
        if let Ok(bytes) = fs::read(path) {
            if let Some(jpeg_bytes) = extract_embedded_thumbnail(&bytes) {
                if let Ok(img) = image::load_from_memory(&jpeg_bytes) {
                    return Some(downsample_to_thumb(img));
                }
            }
        }
        // Fall back to CFA block-average if embedded preview is absent
        if let Ok(cfa) = RawloaderBackend.decode_file(path) {
            return Some(thumbnail(&cfa));
        }
    }
    None
}

fn downsample_to_thumb(img: image::DynamicImage) -> egui::ColorImage {
    let (orig_w, orig_h) = (img.width(), img.height());
    let max_dim = orig_w.max(orig_h);
    let (tw, th) = if max_dim > THUMB_SIDE as u32 {
        let scale = THUMB_SIDE as f32 / max_dim as f32;
        (
            ((orig_w as f32 * scale).round() as u32).max(1),
            ((orig_h as f32 * scale).round() as u32).max(1),
        )
    } else {
        (orig_w.max(1), orig_h.max(1))
    };
    let resized = img.resize(tw, th, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8();
    let pixels: Vec<egui::Color32> = rgba
        .pixels()
        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();
    egui::ColorImage::new([tw as usize, th as usize], pixels)
}

/// A small sRGB rendering of the mosaic as shot: block-averaged per CFA colour, white
/// balanced, highlights kept neutral where the sensor clipped, converted with the camera's
/// matrix, then oriented the way the camera says.
fn thumbnail(cfa: &CfaImage) -> egui::ColorImage {
    let (w, h) = (cfa.width as usize, cfa.height as usize);
    // Whole 2×2 quads per cell, so every cell sees all three colours.
    let cell = w.max(h).div_ceil(THUMB_SIDE).max(2).next_multiple_of(2);
    let (tw, th) = ((w / cell).max(1), (h / cell).max(1));
    // Averaging every photosite is ~24M reads; a few quads per cell is indistinguishable
    // at this size and several times faster.
    let quad_step = (cell / 2 / 4).max(1) * 2;

    let mut pixels = Vec::with_capacity(tw * th);
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = [0.0f32; 3];
            let mut n = [0u32; 3];
            let mut y = ty * cell;
            while y + 1 < ((ty + 1) * cell).min(h) {
                let mut x = tx * cell;
                while x + 1 < ((tx + 1) * cell).min(w) {
                    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        let c = cfa.cfa2x2[dy * 2 + dx] as usize;
                        sum[c] += cfa.data[(y + dy) * w + x + dx] as f32;
                        n[c] += 1;
                    }
                    x += quad_step;
                }
                y += quad_step;
            }

            let mut v = [0.0f32; 3];
            for c in 0..3 {
                let avg = sum[c] / n[c].max(1) as f32;
                v[c] = (avg - cfa.black[c]).max(0.0) / (cfa.white[c] - cfa.black[c]).max(1.0);
            }
            let peak = v[0].max(v[1]).max(v[2]);
            let mut bal = [v[0] * cfa.wb[0], v[1] * cfa.wb[1], v[2] * cfa.wb[2]];
            // The same neutral collapse the shader does for clipped photosites.
            let t = smoothstep(0.93, 1.0, peak);
            let lo = bal[0].min(bal[1]).min(bal[2]);
            for b in &mut bal {
                *b += (lo - *b) * t;
            }
            let m = cfa.cam_to_srgb;
            let rgb = [0, 1, 2].map(|r| {
                let lin = m[r][0] * bal[0] + m[r][1] * bal[1] + m[r][2] * bal[2];
                (srgb(lin.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8
            });
            pixels.push(egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
    orient(egui::ColorImage::new([tw, th], pixels), &cfa.orientation)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Turns first, then mirrors in screen space — the same order the shader uses.
fn orient(mut img: egui::ColorImage, g: &Geometry) -> egui::ColorImage {
    for _ in 0..g.quarter_turns % 4 {
        let [w, h] = img.size;
        let mut out = vec![egui::Color32::BLACK; w * h];
        for y in 0..h {
            for x in 0..w {
                // Clockwise: source (x, y) lands at (h-1-y, x) in an h×w image.
                out[x * h + (h - 1 - y)] = img.pixels[y * w + x];
            }
        }
        img = egui::ColorImage::new([h, w], out);
    }
    let [w, h] = img.size;
    if g.mirror_h {
        for row in img.pixels.chunks_exact_mut(w) {
            row.reverse();
        }
    }
    if g.mirror_v {
        let rows: Vec<Vec<egui::Color32>> = img
            .pixels
            .chunks_exact(w)
            .rev()
            .map(<[_]>::to_vec)
            .collect();
        img.pixels = rows.concat();
    }
    debug_assert_eq!(img.pixels.len(), w * h);
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(w: usize, h: usize) -> egui::ColorImage {
        let pixels = (0..w * h)
            .map(|i| egui::Color32::from_rgb(i as u8, 0, 0))
            .collect();
        egui::ColorImage::new([w, h], pixels)
    }

    #[test]
    fn a_clockwise_turn_moves_the_top_left_to_the_top_right() {
        let img = orient(
            tagged(3, 2),
            &Geometry {
                quarter_turns: 1,
                ..Default::default()
            },
        );
        assert_eq!(img.size, [2, 3]);
        // Source (0,0) is the top-left; after a clockwise turn it is the top-right.
        assert_eq!(img.pixels[1].r(), 0);
        // Source bottom-left (0,1) = index 3 ends up top-left.
        assert_eq!(img.pixels[0].r(), 3);
    }

    #[test]
    fn four_turns_are_the_identity() {
        let src = tagged(4, 3);
        let mut img = src.clone();
        for _ in 0..4 {
            img = orient(
                img,
                &Geometry {
                    quarter_turns: 1,
                    ..Default::default()
                },
            );
        }
        assert_eq!(img.size, src.size);
        assert_eq!(img.pixels, src.pixels);
    }
}

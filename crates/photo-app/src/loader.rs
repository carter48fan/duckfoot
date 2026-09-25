//! Decoding off the UI thread: the photo being opened, and filmstrip thumbnails.
//!
//! A 24 MP RAW takes ~100 ms to decode. On the UI thread that is a visible freeze every
//! time you step through a roll, so both jobs run on worker threads and hand their results
//! back over a channel. The open thread also copies the result into GPU memory
//! (`photo_engine::upload_cfa`): wgpu's device and queue are shared handles, and that copy
//! is 66–200 MB, so all the UI thread records is the demosaic.
//!
//! Thumbnails come from the JPEG preview the camera embedded, decoded at reduced size
//! straight out of the DCT (`photo_engine::preview`) — a few milliseconds, against tens to
//! hundreds for the mosaic. Only a file with no usable preview is built from the mosaic:
//! a block average per colour, the camera's own white balance and matrix, and the sRGB
//! curve. No GPU readback is involved (DESIGN.md invariant 4), and none of the user's
//! edits are applied — a thumbnail shows the file as shot. Only the open photo's tile
//! shows the live render.
//!
//! Opening a photo does two things at once: the full decode, and the same embedded
//! preview at stage size, which is on screen long before the decode lands.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use photo_engine::{
    decode_jpeg_scaled, decode_raster, decode_raw, is_raster_path, load_embedded_preview,
    upload_cfa, upload_raster, CfaImage, Geometry, Preview, RasterImage, UploadPool, UploadedImage,
};

/// Long side of a thumbnail, in pixels. A tile is 112×70 points; this covers it at 2×.
const THUMB_SIDE: usize = 256;

/// Long side of the stand-in shown on the stage while a photo decodes. Most cameras embed
/// something between 1600 px and full resolution; this takes the former as it is and
/// decodes the latter at 1/2 or 1/4.
const STAGE_PREVIEW_SIDE: u32 = 2048;

pub enum Done {
    Opened {
        path: PathBuf,
        /// Matches the request that asked for it; anything older is stale and dropped.
        generation: u64,
        /// Decoded and already copied into GPU memory: the UI thread only has to demosaic.
        result: Result<Box<UploadedImage>, String>,
        decode_ms: f32,
    },
    /// The camera's embedded rendering of a photo still being opened, to show until the
    /// decode lands. Arrives before `Opened` for the same generation, or not at all.
    Preview {
        path: PathBuf,
        generation: u64,
        image: egui::ColorImage,
    },
    Thumbnail {
        path: PathBuf,
        /// `None` if nothing could be built; the request is still answered, so the UI
        /// knows it is no longer outstanding.
        image: Option<egui::ColorImage>,
    },
}

enum Decoded {
    Raw(CfaImage),
    Raster(RasterImage),
}

pub struct Loader {
    open_tx: Sender<(PathBuf, u64)>,
    thumb_tx: Sender<PathBuf>,
    pub done: Receiver<Done>,
    /// Set on the first frame; lets a worker wake the UI when its result lands, since
    /// the event loop otherwise sleeps until the next input event.
    repaint: Arc<Mutex<Option<egui::Context>>>,
}

/// Closed while a photo is being opened. Thumbnail workers wait at it between jobs, so
/// the photo the user asked for never shares the CPU, the disk or rawler's thread pool
/// with a filmstrip full of photos they did not.
#[derive(Default)]
struct Gate {
    opening: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn set_opening(&self, opening: bool) {
        if let Ok(mut state) = self.opening.lock() {
            *state = opening;
        }
        self.changed.notify_all();
    }

    fn wait_until_idle(&self) {
        let Ok(mut state) = self.opening.lock() else {
            return;
        };
        while *state {
            state = match self.changed.wait(state) {
                Ok(s) => s,
                Err(_) => return,
            };
        }
    }
}

impl Loader {
    /// `device` and `queue` are the UI's own. Uploading from the decode thread is what
    /// keeps a 66–200 MB copy per open off the UI thread; wgpu's device and queue are
    /// shared handles built for exactly that.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, pool: UploadPool) -> Self {
        let (done_tx, done) = mpsc::channel();
        let repaint: Arc<Mutex<Option<egui::Context>>> = Arc::default();
        let gate = Arc::new(Gate::default());

        let (open_tx, open_rx) = mpsc::channel::<(PathBuf, u64)>();
        {
            let done_tx = done_tx.clone();
            let repaint = repaint.clone();
            let gate = gate.clone();
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
                        gate.set_opening(true);
                        let started = Instant::now();

                        std::thread::scope(|scope| {
                            // The stand-in, alongside the decode rather than ahead of it:
                            // it is only worth showing if it beats the real thing.
                            if !is_raster_path(&path) {
                                let (path, done_tx, repaint) = (&path, &done_tx, &repaint);
                                scope.spawn(move || {
                                    let preview = load_embedded_preview(path, STAGE_PREVIEW_SIDE);
                                    if let Some(preview) = preview {
                                        let _ = done_tx.send(Done::Preview {
                                            path: path.clone(),
                                            generation,
                                            image: preview_image(preview, None),
                                        });
                                        wake(repaint);
                                    }
                                });
                            }

                            let decoded = if is_raster_path(&path) {
                                fs::read(&path)
                                    .map_err(anyhow::Error::from)
                                    .and_then(|bytes| decode_raster(&bytes))
                                    .map(Decoded::Raster)
                            } else {
                                decode_raw(&path).map(Decoded::Raw)
                            };
                            let decode_ms = started.elapsed().as_secs_f32() * 1000.0;

                            let uploading = Instant::now();
                            // A decode error already names the file; an upload error (a
                            // photo larger than this GPU's textures) does not.
                            let result = decoded
                                .map_err(|e| format!("{e:#}"))
                                .and_then(|decoded| {
                                    match decoded {
                                        Decoded::Raw(cfa) => {
                                            upload_cfa(&device, &queue, &pool, cfa)
                                        }
                                        Decoded::Raster(raster) => {
                                            upload_raster(&device, &queue, raster)
                                        }
                                    }
                                    .map_err(|e| format!("{}: {e:#}", path.display()))
                                })
                                .map(Box::new);
                            log::debug!(
                                "{}: decoded in {decode_ms:.0} ms, uploaded in {:.0} ms",
                                path.display(),
                                uploading.elapsed().as_secs_f32() * 1000.0
                            );
                            let _ = done_tx.send(Done::Opened {
                                path: path.clone(),
                                generation,
                                result,
                                decode_ms,
                            });
                            wake(&repaint);
                        });
                        gate.set_opening(false);
                    }
                })
                .expect("could not start the decode thread");
        }

        // Thumbnails are independent small jobs: one worker per spare core. A preview is
        // a few milliseconds of mostly single-threaded JPEG work, so a single worker
        // leaves every other core idle while a folder's filmstrip fills in one by one.
        let (thumb_tx, thumb_rx) = mpsc::channel::<PathBuf>();
        let thumb_rx = Arc::new(Mutex::new(thumb_rx));
        let workers = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(1)
            .clamp(1, 8);
        for i in 0..workers {
            let (done_tx, repaint, gate, thumb_rx) = (
                done_tx.clone(),
                repaint.clone(),
                gate.clone(),
                thumb_rx.clone(),
            );
            std::thread::Builder::new()
                .name(format!("duckfoot-thumbs-{i}"))
                .spawn(move || loop {
                    gate.wait_until_idle();
                    let job = match thumb_rx.lock() {
                        Ok(rx) => rx.recv(),
                        Err(_) => return,
                    };
                    let Ok(path) = job else { return };
                    // An open may have started while this worker sat waiting for a job.
                    gate.wait_until_idle();
                    let image = generate_thumbnail(&path);
                    let _ = done_tx.send(Done::Thumbnail { path, image });
                    wake(&repaint);
                })
                .expect("could not start a thumbnail thread");
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
        let bytes = fs::read(path).ok()?;
        // A JPEG is decoded at reduced size in the DCT, like a RAW's preview is; a 24 MP
        // camera JPEG decoded at full size and then shrunk is ~10× the work.
        if let Some((width, height, rgb)) = decode_jpeg_scaled(&bytes, THUMB_SIDE as u32) {
            let preview = Preview {
                width,
                height,
                rgb,
                orientation: Geometry::default(),
            };
            return Some(preview_image(preview, Some(THUMB_SIDE)));
        }
        let img = image::load_from_memory(&bytes).ok()?;
        return Some(downsample_to_thumb(img));
    }
    if let Some(preview) = load_embedded_preview(path, THUMB_SIDE as u32) {
        return Some(preview_image(preview, Some(THUMB_SIDE)));
    }
    // No preview this can read: build one from the mosaic.
    decode_raw(path).ok().map(|cfa| thumbnail(&cfa))
}

/// A camera preview as an egui image, shrunk to `side` on its long edge if given, and
/// turned the way the camera says to view it.
fn preview_image(preview: Preview, side: Option<usize>) -> egui::ColorImage {
    let Preview {
        width,
        height,
        rgb,
        orientation,
    } = preview;
    let img = image::RgbImage::from_raw(width, height, rgb)
        .map(image::DynamicImage::ImageRgb8)
        .unwrap_or_else(|| image::DynamicImage::new_rgb8(1, 1));
    let img = match side {
        Some(side) => downsample_to_thumb_side(img, side),
        None => to_color_image(&img),
    };
    orient(img, &orientation)
}

fn downsample_to_thumb(img: image::DynamicImage) -> egui::ColorImage {
    downsample_to_thumb_side(img, THUMB_SIDE)
}

fn downsample_to_thumb_side(img: image::DynamicImage, side: usize) -> egui::ColorImage {
    let (orig_w, orig_h) = (img.width(), img.height());
    let max_dim = orig_w.max(orig_h);
    if max_dim <= side as u32 {
        return to_color_image(&img);
    }
    let scale = side as f32 / max_dim as f32;
    let (tw, th) = (
        ((orig_w as f32 * scale).round() as u32).max(1),
        ((orig_h as f32 * scale).round() as u32).max(1),
    );
    to_color_image(&img.resize_exact(tw, th, image::imageops::FilterType::Triangle))
}

fn to_color_image(img: &image::DynamicImage) -> egui::ColorImage {
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
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

    /// A TIFF-structured "RAW": IFD0 holds only an orientation, and an ordinary JPEG
    /// preview of `w × h` follows, the way cameras lay out their files.
    fn tiff_raw_with_preview(orientation: u16, w: u32, h: u32) -> Vec<u8> {
        let mut file = b"II*\0".to_vec();
        file.extend(8u32.to_le_bytes());
        file.extend(1u16.to_le_bytes());
        file.extend(0x0112u16.to_le_bytes());
        file.extend(3u16.to_le_bytes());
        file.extend(1u32.to_le_bytes());
        file.extend((orientation as u32).to_le_bytes());
        file.extend(0u32.to_le_bytes());
        let img = image::RgbImage::from_fn(w, h, |x, _| image::Rgb([x as u8, 90, 200]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 85)
            .encode_image(&img)
            .unwrap();
        file
    }

    #[test]
    fn a_raw_thumbnail_comes_from_its_preview_turned_upright() {
        let dir = std::env::temp_dir().join(format!("duckfoot-thumb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (landscape, portrait) = (dir.join("a.arw"), dir.join("b.arw"));
        fs::write(&landscape, tiff_raw_with_preview(1, 1616, 1080)).unwrap();
        // EXIF 6: shot with the camera turned, viewed after a clockwise quarter turn.
        fs::write(&portrait, tiff_raw_with_preview(6, 1616, 1080)).unwrap();

        let thumb = generate_thumbnail(&landscape).expect("no thumbnail");
        assert_eq!(thumb.size, [THUMB_SIDE, 171]);
        let thumb = generate_thumbnail(&portrait).expect("no thumbnail");
        assert_eq!(thumb.size, [171, THUMB_SIDE]);
        let _ = fs::remove_dir_all(&dir);
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

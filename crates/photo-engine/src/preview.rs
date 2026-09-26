//! The JPEG previews cameras embed in their RAW files, found and decoded without touching
//! the sensor data.
//!
//! Every mainstream RAW carries at least one ordinary JPEG rendering of the frame, and
//! decoding it — at reduced size, straight out of the DCT — is one to two orders of
//! magnitude cheaper than decoding the mosaic. It is what a filmstrip shows, and what the
//! stage shows for the fraction of a second before the real decode lands.
//!
//! Containers are not parsed. A RAW is scanned for JPEG start-of-image markers and each
//! candidate is walked marker by marker: that reads its dimensions from the frame header
//! without decoding, rejects the lossless-JPEG streams many formats store the *sensor* in
//! (DNG, CR2 and Sony's lossless ARW tile their mosaic as SOF3 JPEGs), and finds the real
//! end of the stream rather than the end of an EXIF thumbnail nested inside it.

use std::fs::File;
use std::path::Path;

use crate::params::Geometry;

/// An 8-bit sRGB preview, as the camera rendered it.
pub struct Preview {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGB, 3 bytes per pixel, row-major.
    pub rgb: Vec<u8>,
    /// How the frame is meant to be viewed. The preview itself is stored as the sensor
    /// saw it, exactly like the mosaic.
    pub orientation: Geometry,
}

/// A JPEG stream found inside a larger file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddedJpeg<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl EmbeddedJpeg<'_> {
    fn long_side(&self) -> u32 {
        self.width.max(self.height)
    }
}

/// The best embedded JPEG for showing at `want` pixels on the long side: the first one
/// found that is at least that large, otherwise the largest there is.
///
/// Stopping at the first large-enough stream keeps the scan — and so the I/O, when the
/// file is memory-mapped — to the front of the file for every common format: cameras
/// write their previews ahead of the sensor data.
pub fn find_embedded_jpeg(bytes: &[u8], want: u32) -> Option<EmbeddedJpeg<'_>> {
    let mut best: Option<EmbeddedJpeg> = None;
    let mut from = 0;
    // A corrupt length can point past the end of the file; that ends the scan, not us.
    while let Some(found) = bytes
        .get(from..)
        .and_then(|rest| memchr::memmem::find(rest, &[0xFF, 0xD8, 0xFF]))
    {
        let start = from + found;
        match walk(bytes, start) {
            Walk::Image(jpeg) => {
                if jpeg.long_side() >= want {
                    return Some(jpeg);
                }
                if best.is_none_or(|b| jpeg.long_side() > b.long_side()) {
                    best = Some(jpeg);
                }
                // Whatever sits inside this stream (an EXIF thumbnail) is smaller than it.
                from = start + jpeg.data.len();
            }
            Walk::Skip(to) => from = to.max(start + 2),
        }
    }
    best
}

enum Walk<'a> {
    /// A complete DCT-coded JPEG.
    Image(EmbeddedJpeg<'a>),
    /// Not a usable image; resume scanning at this offset.
    Skip(usize),
}

/// Follow the marker structure of the JPEG starting at `start`.
fn walk(b: &[u8], start: usize) -> Walk<'_> {
    let be16 = |at: usize| -> Option<usize> {
        Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as usize)
    };
    let mut p = start + 2;
    let mut size: Option<(u32, u32)> = None;
    loop {
        // Every marker is 0xFF followed by a code; any run of extra 0xFF is fill.
        if b.get(p) != Some(&0xFF) {
            return Walk::Skip(p);
        }
        while b.get(p) == Some(&0xFF) {
            p += 1;
        }
        let Some(&marker) = b.get(p) else {
            return Walk::Skip(p);
        };
        p += 1;
        match marker {
            // End of image before any scan: nothing to show.
            0xD9 => return Walk::Skip(p),
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        let Some(len) = be16(p).filter(|l| *l >= 2) else {
            return Walk::Skip(p);
        };
        match marker {
            // Baseline, extended and progressive Huffman: what previews are.
            0xC0..=0xC2 => {
                let (Some(h), Some(w)) = (be16(p + 3), be16(p + 5)) else {
                    return Walk::Skip(p);
                };
                let components = b.get(p + 7).copied().unwrap_or(0);
                // 1 = greyscale, 3 = YCbCr. Anything else is not a preview.
                if w == 0 || h == 0 || !(components == 1 || components == 3) {
                    return Walk::Skip(p + len);
                }
                size = Some((w as u32, h as u32));
            }
            // Lossless (SOF3, the sensor tiles), hierarchical and arithmetic-coded
            // frames: none of them a preview, and SOF3 is by far the most common.
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Walk::Skip(p + len),
            0xDA => {
                if size.is_none() {
                    return Walk::Skip(p + len);
                }
                // Entropy-coded data: a literal 0xFF is always stuffed as FF 00, so the
                // next real marker is the next 0xFF not followed by 00 or a restart.
                let mut q = p + len;
                loop {
                    let Some(off) = memchr::memchr(0xFF, b.get(q..).unwrap_or(&[])) else {
                        return Walk::Skip(b.len());
                    };
                    q += off;
                    match b.get(q + 1) {
                        None => return Walk::Skip(b.len()),
                        Some(0x00) | Some(0xD0..=0xD7) | Some(0xFF) => q += 1,
                        Some(0xD9) => {
                            let (width, height) = size.unwrap_or_default();
                            return Walk::Image(EmbeddedJpeg {
                                data: &b[start..q + 2],
                                width,
                                height,
                            });
                        }
                        // Another segment: a progressive JPEG's next scan, or tables.
                        Some(_) => break,
                    }
                }
                p = q;
                continue;
            }
            _ => {}
        }
        p += len;
    }
}

/// Decode a JPEG to RGB at no less than `want` pixels on the long side, letting the
/// decoder skip DCT work (1/2, 1/4 or 1/8 scale) wherever the stream is larger than that.
pub fn decode_jpeg_scaled(jpeg: &[u8], want: u32) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = jpeg_decoder::Decoder::new(jpeg);
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    if w == 0 || h == 0 {
        return None;
    }
    // Ask for the size that makes the long side `want`; the decoder rounds up to the
    // nearest scale it supports, so the result is never smaller than asked.
    let k = (want as f32 / w.max(h) as f32).min(1.0);
    let req = |v: u32| ((v as f32 * k).ceil() as u32).clamp(1, u16::MAX as u32) as u16;
    decoder.scale(req(w), req(h)).ok()?;
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    let rgb = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels,
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&l| [l, l, l]).collect(),
        _ => return None,
    };
    (rgb.len() == (w * h * 3) as usize).then_some((w, h, rgb))
}

/// The embedded preview of `path` at roughly `want` pixels on the long side, or `None` if
/// the file has no JPEG preview this can read.
///
/// The file is memory-mapped rather than read, so a preview near the front of a 60 MB RAW
/// costs only the pages it sits in.
pub fn load_embedded_preview(path: &Path, want: u32) -> Option<Preview> {
    let file = File::open(path).ok()?;
    // SAFETY: the mapping is read-only and lives only for this call. A file truncated
    // underneath us can fault, which is the accepted risk of every mmap-based RAW reader
    // (rawler maps the same files for decoding).
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    let bytes: &[u8] = &map;
    let jpeg = find_embedded_jpeg(bytes, want)?;
    let (width, height, rgb) = decode_jpeg_scaled(jpeg.data, want)?;
    // CR3 and RAF keep their orientation outside any TIFF header; without the fallback
    // their thumbnails and stand-ins came out unrotated.
    let orientation = crate::decode::tiff_orientation(bytes)
        .or_else(|| crate::metadata::read_raw_info(path).map(|i| i.orientation))
        .unwrap_or_default();
    Some(Preview {
        width,
        height,
        rgb,
        orientation,
    })
}

/// How the camera meant `path` to be viewed, read from its header without decoding any
/// pixels: the first megabyte holds the orientation tag of every TIFF-based RAW, and
/// rawler's metadata parser finds it in the containers that keep it elsewhere (CR3, RAF).
pub fn file_orientation(path: &Path) -> Geometry {
    use std::io::Read;
    if crate::is_raster_path(path) {
        return std::fs::read(path)
            .map(|bytes| crate::decode::raster_orientation(&bytes))
            .unwrap_or_default();
    }
    let mut head = Vec::new();
    let read = File::open(path).and_then(|f| f.take(1 << 20).read_to_end(&mut head));
    if read.is_err() {
        return Geometry::default();
    }
    crate::decode::tiff_orientation(&head)
        .or_else(|| crate::metadata::read_raw_info(path).map(|i| i.orientation))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real baseline JPEG of `w × h`.
    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([x as u8, y as u8, 128]));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
            .encode_image(&img)
            .unwrap();
        out
    }

    /// The header of a lossless (SOF3) JPEG, as DNG and CR2 store their sensor tiles.
    fn lossless_tile() -> Vec<u8> {
        let mut t = vec![
            0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B, 14, 0x01, 0x00, 0x02, 0x00, 1,
        ];
        t.extend_from_slice(&[1, 0x11, 0]);
        t.extend(std::iter::repeat_n(0x5A, 4000));
        t
    }

    #[test]
    fn finds_a_preview_between_sensor_tiles() {
        let preview = jpeg(640, 480);
        let mut file = b"II*\0".to_vec();
        file.extend(lossless_tile());
        file.extend(&preview);
        file.extend(lossless_tile());
        let found = find_embedded_jpeg(&file, 256).expect("preview not found");
        assert_eq!((found.width, found.height), (640, 480));
        assert_eq!(found.data, &preview[..]);
    }

    #[test]
    fn a_segment_length_past_the_end_of_the_file_is_not_a_crash() {
        let mut file = jpeg(64, 64);
        // A truncated second stream whose APP0 claims 65 kB it does not have.
        file.extend([0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xF0, 0x00]);
        assert_eq!(find_embedded_jpeg(&file, 256).map(|j| j.width), Some(64));
        for cut in 0..file.len() {
            let _ = find_embedded_jpeg(&file[..cut], 256);
        }
    }

    #[test]
    fn a_lossless_tile_is_never_a_preview() {
        let mut file = lossless_tile();
        file.extend(lossless_tile());
        assert!(find_embedded_jpeg(&file, 256).is_none());
    }

    #[test]
    fn an_exif_thumbnail_nested_in_a_preview_does_not_truncate_it() {
        // APP1 carrying its own complete JPEG, the way camera previews embed EXIF.
        let thumb = jpeg(160, 120);
        let big = jpeg(1024, 683);
        let mut app1 = vec![0xFF, 0xE1];
        app1.extend(((thumb.len() + 2) as u16).to_be_bytes());
        app1.extend(&thumb);
        let mut preview = big[..2].to_vec();
        preview.extend(&app1);
        preview.extend(&big[2..]);
        let found = find_embedded_jpeg(&preview, 256).unwrap();
        assert_eq!((found.width, found.height), (1024, 683));
        assert_eq!(found.data.len(), preview.len());
    }

    #[test]
    fn prefers_the_first_preview_large_enough_else_the_largest() {
        let (small, medium, large) = (jpeg(160, 120), jpeg(400, 300), jpeg(1200, 800));
        let file = [small.clone(), medium.clone(), large.clone()].concat();
        assert_eq!(find_embedded_jpeg(&file, 256).unwrap().width, 400);
        assert_eq!(find_embedded_jpeg(&file, 1000).unwrap().width, 1200);
        assert_eq!(find_embedded_jpeg(&file, 5000).unwrap().width, 1200);
    }

    #[test]
    fn scaled_decode_is_never_smaller_than_asked() {
        let big = jpeg(1600, 1000);
        let (w, h, rgb) = decode_jpeg_scaled(&big, 256).unwrap();
        // 1/4 would be 400 × 250; 1/8 would be 200, under the 256 asked for.
        assert_eq!((w, h), (400, 250));
        assert_eq!(rgb.len(), 400 * 250 * 3);
        let (w, _, _) = decode_jpeg_scaled(&big, 5000).unwrap();
        assert_eq!(w, 1600);
    }
}

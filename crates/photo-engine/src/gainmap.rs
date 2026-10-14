//! HDR gain map JPEGs (ISO 21496-1, Android's "Ultra HDR", Apple's adaptive HDR): the SDR
//! export as written, followed by a small grey JPEG saying, place by place, how many stops
//! brighter an HDR display may show it. A viewer without HDR shows the SDR image and never
//! sees the map; one with headroom brightens what the SDR export took to white, as far as
//! the display can.
//!
//! The map is measured against an HDR rendition of the same edit whose white is
//! [`HDR_WHITE`] times the export's (see `adjust.wgsl`'s `finish`), so where nothing passes
//! white the map is 0 and the file shows as the SDR export everywhere.
//!
//! The metadata and the container (the ISO 21496-1 payload, Adobe's `hdrgm` and Apple's
//! XMP, the MPF index) are adapted from LightCraft's `crates/codecs/src/gainmap.rs` (at
//! storytold/lightcraft b3d392b4), used under its MIT licence: Copyright (c) 2026 ArtCraft
//! Team and the LightCraft contributors (see THIRD_PARTY_LICENSES.md). Changed here: one
//! luminance channel only, the map measured as the export's bands come back rather than
//! from two whole renditions, and the JPEGs written by Duckfoot's own encoder.

use anyhow::{anyhow, bail, Result};

/// The HDR rendition's white, as a multiple of the export's: two stops, about the
/// headroom of a phone or laptop HDR screen at ordinary brightness.
pub const HDR_WHITE: f32 = 4.0;

/// The map is this many times smaller than the image each way, as cameras write theirs.
const SCALE: u32 = 4;

/// Added to both luminances before their ratio, so a black stays finite and noise in the
/// shadows does not become gain.
const OFFSET: f32 = 1.0 / 64.0;

/// The ISO 21496-1 namespace that opens its APP2 payloads, NUL included.
const ISO_URN: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";

/// The fixed denominator of the metadata's rationals.
const DENOMINATOR: u32 = 1_000_000;

/// How a viewer turns the map's codes back into gains: `min + (max − min) · code^(1/gamma)`
/// stops, applied as `(sdr + base_offset) · 2^gain − alternate_offset`, in full on a screen
/// with `alternate_headroom` stops above white and not at all at `base_headroom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainMapMeta {
    pub min: f32,
    pub max: f32,
    pub gamma: f32,
    pub base_offset: f32,
    pub alternate_offset: f32,
    pub base_headroom: f32,
    pub alternate_headroom: f32,
}

/// A measured gain map: 8-bit codes, a quarter the image's size each way.
#[derive(Debug, Clone, PartialEq)]
pub struct GainMap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub meta: GainMapMeta,
}

/// A gain map being measured from the bands of a render: the sum of the log gains of each
/// block of the image, and how many pixels it has seen.
pub(crate) struct Measure {
    width: u32,
    columns: u32,
    rows: u32,
    sums: Vec<f32>,
    counts: Vec<u32>,
}

impl Measure {
    /// For an image `width` × `height`.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let (columns, rows) = (width.div_ceil(SCALE).max(1), height.div_ceil(SCALE).max(1));
        let blocks = columns as usize * rows as usize;
        Self {
            width: width.max(1),
            columns,
            rows,
            sums: vec![0.0; blocks],
            counts: vec![0; blocks],
        }
    }

    /// The pixels of whole rows starting at row `first`: each one's luminance in the
    /// export (white at 1) and in the HDR rendition.
    pub(crate) fn add(&mut self, first: u32, pixels: impl Iterator<Item = (f32, f32)>) {
        for (i, (sdr, hdr)) in pixels.enumerate() {
            let (x, y) = (i as u32 % self.width, first + i as u32 / self.width);
            let block = (y / SCALE).min(self.rows - 1) as usize * self.columns as usize
                + (x / SCALE).min(self.columns - 1) as usize;
            let gain = ((hdr.max(0.0) + OFFSET) / (sdr.max(0.0) + OFFSET)).log2();
            if gain.is_finite() {
                self.sums[block] += gain;
                self.counts[block] += 1;
            }
        }
    }

    /// The map: each block's mean gain, as codes over the range the image uses (which
    /// always includes 0, so a block with no HDR in it is exactly no change).
    pub(crate) fn finish(self) -> GainMap {
        let gains: Vec<f32> = self
            .sums
            .iter()
            .zip(&self.counts)
            .map(|(s, &n)| if n > 0 { s / n as f32 } else { 0.0 })
            .collect();
        let min = gains.iter().copied().fold(0.0f32, f32::min);
        let max = gains.iter().copied().fold(0.0f32, f32::max).max(min + 1e-4);
        let meta = GainMapMeta {
            min,
            max,
            gamma: 1.0,
            base_offset: OFFSET,
            alternate_offset: OFFSET,
            base_headroom: 0.0,
            alternate_headroom: max.max(1e-3),
        };
        let data = gains
            .iter()
            .map(|g| (((g - min) / (max - min)).clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        GainMap {
            width: self.columns,
            height: self.rows,
            data,
            meta,
        }
    }
}

impl GainMapMeta {
    /// The gain map image's ISO 21496-1 APP2 payload, namespace included.
    fn to_iso(self) -> Vec<u8> {
        let mut p = ISO_URN.to_vec();
        p.extend_from_slice(&0u16.to_be_bytes()); // minimum version, which readers gate on
        p.extend_from_slice(&0u16.to_be_bytes()); // writer version
        p.push(0x40); // one channel, applied in the base image's colour space
        let unsigned = |p: &mut Vec<u8>, v: f32| {
            let n = (f64::from(v) * f64::from(DENOMINATOR))
                .round()
                .clamp(0.0, f64::from(u32::MAX)) as u32;
            p.extend_from_slice(&n.to_be_bytes());
            p.extend_from_slice(&DENOMINATOR.to_be_bytes());
        };
        let signed = |p: &mut Vec<u8>, v: f32| {
            let n = (f64::from(v) * f64::from(DENOMINATOR))
                .round()
                .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
            p.extend_from_slice(&n.to_be_bytes());
            p.extend_from_slice(&DENOMINATOR.to_be_bytes());
        };
        unsigned(&mut p, self.base_headroom.max(0.0));
        unsigned(&mut p, self.alternate_headroom.max(0.0));
        signed(&mut p, self.min);
        signed(&mut p, self.max);
        unsigned(&mut p, self.gamma);
        signed(&mut p, self.base_offset);
        signed(&mut p, self.alternate_offset);
        p
    }

    /// The gain map image's XMP: Adobe's `hdrgm` parameters, which Android and Chrome read
    /// in place of the ISO payload, and Apple's description of the same map.
    fn to_xmp(self) -> String {
        let mut s = String::from(XMP_OPEN);
        s.push_str(concat!(
            r#"<rdf:Description rdf:about="" xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/""#,
            r#" hdrgm:Version="1.0" hdrgm:BaseRenditionIsHDR="False""#
        ));
        for (name, v) in [
            ("HDRCapacityMin", self.base_headroom),
            ("HDRCapacityMax", self.alternate_headroom),
            ("GainMapMin", self.min),
            ("GainMapMax", self.max),
            ("Gamma", self.gamma),
            ("OffsetSDR", self.base_offset),
            ("OffsetHDR", self.alternate_offset),
        ] {
            s.push_str(&format!(r#" hdrgm:{name}="{}""#, num(v)));
        }
        s.push_str("/>");
        s.push_str(concat!(
            r#"<rdf:Description rdf:about="" xmlns:apdi="http://ns.apple.com/pixeldatainfo/1.0/""#,
            r#" xmlns:HDRGainMap="http://ns.apple.com/HDRGainMap/1.0/""#,
            r#" xmlns:HDRToneMap="http://ns.apple.com/HDRToneMap/1.0/">"#,
            "<apdi:AuxiliaryImageType>urn:com:apple:photo:2020:aux:hdrgainmap</apdi:AuxiliaryImageType>",
            // 'L008': 8-bit luminance, as stored.
            "<apdi:NativeFormat>1278226488</apdi:NativeFormat>",
            "<apdi:StoredFormat>1278226488</apdi:StoredFormat>",
            "<HDRGainMap:HDRGainMapVersion>131072</HDRGainMap:HDRGainMapVersion>",
        ));
        // Apple's headroom is the multiplier itself, not stops.
        s.push_str(&format!(
            "<HDRGainMap:HDRGainMapHeadroom>{}</HDRGainMap:HDRGainMapHeadroom>",
            num(self.alternate_headroom.exp2())
        ));
        s.push_str(&format!(
            "<HDRToneMap:AlternateHeadroom>{}</HDRToneMap:AlternateHeadroom>",
            num(self.alternate_headroom)
        ));
        s.push_str(&format!(
            concat!(
                r#"<HDRToneMap:ChannelMetadata><rdf:Seq><rdf:li rdf:parseType="Resource">"#,
                "<HDRToneMap:GainMapMin>{}</HDRToneMap:GainMapMin>",
                "<HDRToneMap:GainMapMax>{}</HDRToneMap:GainMapMax>",
                "<HDRToneMap:Gamma>{}</HDRToneMap:Gamma>",
                "<HDRToneMap:BaseOffset>{}</HDRToneMap:BaseOffset>",
                "<HDRToneMap:AlternateOffset>{}</HDRToneMap:AlternateOffset>",
                "</rdf:li></rdf:Seq></HDRToneMap:ChannelMetadata>"
            ),
            num(self.min),
            num(self.max),
            num(self.gamma),
            num(self.base_offset),
            num(self.alternate_offset)
        ));
        s.push_str(&format!(
            "<HDRToneMap:BaseHeadroom>{}</HDRToneMap:BaseHeadroom>",
            num(self.base_headroom)
        ));
        s.push_str(concat!(
            "<HDRToneMap:BaseColorIsWorkingColor>True</HDRToneMap:BaseColorIsWorkingColor>",
            "<HDRToneMap:Version>1</HDRToneMap:Version></rdf:Description>"
        ));
        s.push_str(XMP_CLOSE);
        s
    }
}

impl GainMap {
    /// The map as its own JPEG, at `quality`, with its metadata in it.
    pub(crate) fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>> {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode(
                &self.data,
                self.width,
                self.height,
                image::ExtendedColorType::L8,
            )
            .map_err(|e| anyhow!("encoding the gain map: {e}"))?;
        let mut xmp = crate::export::JPEG_XMP.to_vec();
        xmp.extend_from_slice(self.meta.to_xmp().as_bytes());
        insert_segments(&jpeg, &[(0xE1, xmp), (0xE2, self.meta.to_iso())])
    }
}

/// `xmp` (the export's packet, if it has one) with the container description added that
/// marks the file as a gain map image and says how long the map is.
pub(crate) fn container_xmp(xmp: Option<&str>, gain_len: usize) -> String {
    let description = format!(
        concat!(
            r#"<rdf:Description rdf:about="" xmlns:Container="http://ns.google.com/photos/1.0/container/""#,
            r#" xmlns:Item="http://ns.google.com/photos/1.0/container/item/""#,
            r#" xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/" hdrgm:Version="1.0">"#,
            r#"<Container:Directory><rdf:Seq>"#,
            r#"<rdf:li rdf:parseType="Resource"><Container:Item Item:Semantic="Primary" Item:Mime="image/jpeg"/></rdf:li>"#,
            r#"<rdf:li rdf:parseType="Resource"><Container:Item Item:Semantic="GainMap" Item:Mime="image/jpeg" Item:Length="{}"/></rdf:li>"#,
            r#"</rdf:Seq></Container:Directory></rdf:Description>"#
        ),
        gain_len
    );
    if let Some(at) = xmp.and_then(|x| x.rfind("</rdf:RDF>").map(|i| (x, i))) {
        let (x, i) = at;
        return format!("{}{description}{}", &x[..i], &x[i..]);
    }
    format!("{XMP_OPEN}{description}{XMP_CLOSE}")
}

/// The MPF payload's index entries, counted from its byte-order mark: the TIFF header (8),
/// the tag count (2), three tags (36) and the next-IFD offset (4).
const MPF_ENTRIES: usize = 8 + 2 + 3 * 12 + 4;

/// `primary` (the export, a whole JPEG) with `gain` (the map's) after it: the ISO 21496-1
/// marker and an MPF index (CIPA DC-007) that says where the map is are added to the
/// primary's application segments.
pub(crate) fn with_gain_map(primary: &[u8], gain: &[u8]) -> Result<Vec<u8>> {
    let mut marker = ISO_URN.to_vec();
    marker.extend_from_slice(&[0, 0, 0, 0]); // versions only: "this image has a gain map"
    let at = app_end(primary)?;
    let mut out = insert_segments(primary, &[(0xE2, marker), (0xE2, mpf())])?;
    // Where the MPF payload's byte-order mark landed: after the ISO segment (marker,
    // length, payload) and the MPF segment's own marker, length and "MPF\0".
    let endian = at + (4 + ISO_URN.len() + 4) + 4 + 4;
    let entries = endian + MPF_ENTRIES;
    let (len, gain_len) = (out.len(), gain.len());
    let gain_at = len - endian; // from the byte-order mark, as MPF counts
    for (field, v) in [
        (entries + 4, len),
        (entries + 20, gain_len),
        (entries + 24, gain_at),
    ] {
        let v = u32::try_from(v).map_err(|_| anyhow!("a gain map JPEG past 4 GiB"))?;
        out[field..field + 4].copy_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(gain);
    Ok(out)
}

/// The primary image's MPF payload: two images, the primary (shown) and the gain map, with
/// their sizes and the map's offset left 0 for [`with_gain_map`] to fill in.
fn mpf() -> Vec<u8> {
    let mut p = b"MPF\0MM".to_vec();
    p.extend_from_slice(&42u16.to_be_bytes());
    p.extend_from_slice(&8u32.to_be_bytes()); // the index IFD, right after the header
    p.extend_from_slice(&3u16.to_be_bytes());
    let tag = |p: &mut Vec<u8>, tag: u16, ty: u16, count: u32, value: [u8; 4]| {
        p.extend_from_slice(&tag.to_be_bytes());
        p.extend_from_slice(&ty.to_be_bytes());
        p.extend_from_slice(&count.to_be_bytes());
        p.extend_from_slice(&value);
    };
    tag(&mut p, 0xB000, 7, 4, *b"0100"); // MPFVersion
    tag(&mut p, 0xB001, 4, 1, 2u32.to_be_bytes()); // NumberOfImages
    tag(&mut p, 0xB002, 7, 32, (MPF_ENTRIES as u32).to_be_bytes()); // MPEntry
    p.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
                                              // The primary: baseline (0x030000), the representative image (bit 29); then the map
                                              // (0x050000, a gain map). Each: attributes, size, offset, two dependent images.
    for attributes in [0x2003_0000u32, 0x0005_0000] {
        p.extend_from_slice(&attributes.to_be_bytes());
        p.extend_from_slice(&[0; 12]);
    }
    p
}

/// Where a JPEG's leading application segments (APP0 to APP15) end.
fn app_end(jpeg: &[u8]) -> Result<usize> {
    if jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
        bail!("the JPEG encoder wrote something that is not a JPEG");
    }
    let mut at = 2;
    while let (Some(&0xFF), Some(&m)) = (jpeg.get(at), jpeg.get(at + 1)) {
        if !(0xE0..=0xEF).contains(&m) {
            break;
        }
        let seg = jpeg
            .get(at + 2..at + 4)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            .ok_or_else(|| anyhow!("truncated JPEG segment"))?;
        at += 2 + seg;
    }
    Ok(at.min(jpeg.len()))
}

/// `jpeg` with `segments` (marker, payload) added after its application segments.
pub(crate) fn insert_segments(jpeg: &[u8], segments: &[(u8, Vec<u8>)]) -> Result<Vec<u8>> {
    let at = app_end(jpeg)?;
    let mut out =
        Vec::with_capacity(jpeg.len() + segments.iter().map(|s| s.1.len() + 4).sum::<usize>());
    out.extend_from_slice(&jpeg[..at]);
    for (marker, payload) in segments {
        let len = u16::try_from(payload.len() + 2)
            .map_err(|_| anyhow!("a {}-byte JPEG segment", payload.len()))?;
        out.extend_from_slice(&[0xFF, *marker]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
    }
    out.extend_from_slice(&jpeg[at..]);
    Ok(out)
}

const XMP_OPEN: &str = "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">";
const XMP_CLOSE: &str = "</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>";

/// A short decimal: six places at most, trailing zeros dropped.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map of an 8 × 4 image whose right half is two stops brighter in HDR.
    fn measured() -> GainMap {
        let mut m = Measure::new(8, 4);
        // In two bands, as the render hands them back.
        for (first, rows) in [(0, 3), (3, 1)] {
            let px = (0..8 * rows).map(|i| if i % 8 >= 4 { (1.0, 4.0) } else { (0.5, 0.5) });
            m.add(first, px);
        }
        m.finish()
    }

    #[test]
    fn the_map_holds_each_blocks_gain() {
        let g = measured();
        assert_eq!((g.width, g.height), (2, 1));
        assert_eq!(g.meta.min, 0.0);
        let want = ((4.0 + OFFSET) / (1.0 + OFFSET)).log2();
        assert!((g.meta.max - want).abs() < 1e-5, "{}", g.meta.max);
        assert_eq!(g.data, [0, 255]);
        assert_eq!(g.meta.alternate_headroom, g.meta.max);
    }

    #[test]
    fn an_image_with_nothing_past_white_has_a_map_of_no_change() {
        let mut m = Measure::new(5, 5);
        m.add(0, (0..25).map(|_| (0.3, 0.3)));
        let g = m.finish();
        assert!(g.data.iter().all(|&c| c == 0));
        assert_eq!(g.meta.min, 0.0);
        assert!(g.meta.max > g.meta.min);
    }

    #[test]
    fn the_iso_payload_reads_back() {
        let meta = measured().meta;
        let p = meta.to_iso();
        assert!(p.starts_with(ISO_URN));
        let r = &p[ISO_URN.len()..];
        assert_eq!(&r[..4], &[0, 0, 0, 0]);
        assert_eq!(r[4], 0x40);
        let rational = |i: usize| {
            let at = 5 + i * 8;
            let n = i32::from_be_bytes(r[at..at + 4].try_into().unwrap());
            let d = u32::from_be_bytes(r[at + 4..at + 8].try_into().unwrap());
            n as f32 / d as f32
        };
        // Headrooms, then min, max, gamma, the two offsets.
        assert_eq!(rational(0), 0.0);
        assert!((rational(1) - meta.alternate_headroom).abs() < 1e-5);
        assert!((rational(3) - meta.max).abs() < 1e-5);
        assert_eq!(rational(4), 1.0);
        assert_eq!(r.len(), 5 + 7 * 8);
    }

    #[test]
    fn the_mpf_index_points_at_the_gain_map() {
        let g = measured();
        let gain = g.to_jpeg(90).unwrap();
        let mut primary = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut primary, 90)
            .encode(&[128u8; 8 * 4 * 3], 8, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        let file = with_gain_map(&primary, &gain).unwrap();

        let at = file.windows(4).position(|w| w == b"MPF\0").unwrap() + 4;
        let entries = at + MPF_ENTRIES;
        let be = |i: usize| u32::from_be_bytes(file[i..i + 4].try_into().unwrap()) as usize;
        let (primary_len, gain_len, gain_at) =
            (be(entries + 4), be(entries + 20), be(entries + 24));
        assert_eq!(be(entries + 16) & 0xFF_FFFF, 0x05_0000);
        assert_eq!(primary_len + gain_len, file.len());
        assert_eq!(&file[at + gain_at..], gain.as_slice());
        assert_eq!(&file[primary_len..primary_len + 2], &[0xFF, 0xD8]);

        // Both images decode, the primary as itself and the map as its codes.
        let base = image::load_from_memory(&file).unwrap();
        assert_eq!((base.width(), base.height()), (8, 4));
        let map = image::load_from_memory(&file[primary_len..])
            .unwrap()
            .to_luma8();
        assert_eq!((map.width(), map.height()), (2, 1));
        assert!(map.get_pixel(0, 0)[0] < 8 && map.get_pixel(1, 0)[0] > 247);
        let map_xmp = String::from_utf8_lossy(&file[primary_len..]);
        assert!(map_xmp.contains(r#"hdrgm:GainMapMax=""#));
        assert!(map_xmp.contains("HDRGainMapHeadroom"));
    }

    #[test]
    fn the_container_goes_into_the_exports_own_xmp() {
        let own = "<x:xmpmeta><rdf:RDF><rdf:Description a=\"1\"/></rdf:RDF></x:xmpmeta>";
        let x = container_xmp(Some(own), 1234);
        assert!(x.contains("a=\"1\""));
        assert!(x.contains(r#"Item:Length="1234""#));
        assert!(x.find("Container:Directory") < x.find("</rdf:RDF>"));
        assert!(container_xmp(None, 9).starts_with("<?xpacket"));
    }
}

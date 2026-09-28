//! The EXIF an export carries: how the photo was taken, who made it, and — only if asked
//! — where. Written as a little-endian TIFF structure, which is what JPEG's APP1, PNG's
//! eXIf and WebP's EXIF chunk all hold; a TIFF export gets the same fields as tags of its
//! own (see `export`).

use serde::{Deserialize, Serialize};

use crate::metadata::PhotoInfo;

/// How much of the photo's metadata an export keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MetadataPolicy {
    /// Camera, lens, exposure, time, artist and copyright (and location, unless stripped).
    #[default]
    All,
    /// Artist and copyright only: the credit, and nothing about the camera or the moment.
    CopyrightOnly,
    None,
}

impl MetadataPolicy {
    pub const ALL: [MetadataPolicy; 3] = [Self::All, Self::CopyrightOnly, Self::None];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::CopyrightOnly => "Copyright only",
            Self::None => "None",
        }
    }
}

/// One TIFF field's value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Ascii(String),
    Short(Vec<u16>),
    Long(u32),
    Rational(Vec<(u32, u32)>),
    Byte(Vec<u8>),
    Undefined(Vec<u8>),
}

impl Value {
    /// TIFF field type, element count and the bytes, little-endian.
    fn encode(&self) -> (u16, u32, Vec<u8>) {
        match self {
            Value::Ascii(s) => {
                let mut b = s.as_bytes().to_vec();
                b.push(0);
                (2, b.len() as u32, b)
            }
            Value::Short(v) => (
                3,
                v.len() as u32,
                v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            ),
            Value::Long(v) => (4, 1, v.to_le_bytes().to_vec()),
            Value::Rational(v) => (
                5,
                v.len() as u32,
                v.iter()
                    .flat_map(|(n, d)| n.to_le_bytes().into_iter().chain(d.to_le_bytes()))
                    .collect(),
            ),
            Value::Byte(v) => (1, v.len() as u32, v.clone()),
            Value::Undefined(v) => (7, v.len() as u32, v.clone()),
        }
    }
}

/// The fields, by directory. Tags are sorted within each, as TIFF requires.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Fields {
    pub ifd0: Vec<(u16, Value)>,
    pub exif: Vec<(u16, Value)>,
    pub gps: Vec<(u16, Value)>,
}

impl Fields {
    pub fn is_empty(&self) -> bool {
        self.ifd0.is_empty() && self.exif.is_empty() && self.gps.is_empty()
    }
}

pub(crate) const TAG_EXIF_IFD: u16 = 34665;
pub(crate) const TAG_GPS_IFD: u16 = 34853;

/// A rational close to `v`, over a denominator that keeps it exact for the values
/// cameras write (f/2.8, 35.5 mm).
fn rational(v: f64, denom: u32) -> (u32, u32) {
    ((v.max(0.0) * denom as f64).round() as u32, denom)
}

/// What an export of a photo with `info` records under `policy`. `now` is the export's
/// own time, `YYYY:MM:DD HH:MM:SS`.
pub(crate) fn fields(
    info: Option<&PhotoInfo>,
    policy: MetadataPolicy,
    strip_location: bool,
    now: &str,
) -> Fields {
    let mut f = Fields::default();
    if policy == MetadataPolicy::None {
        return f;
    }
    let text = |s: &str| Value::Ascii(s.trim().to_string());
    let credit = |f: &mut Fields| {
        if let Some(info) = info {
            if let Some(a) = info.artist.as_deref().filter(|s| !s.trim().is_empty()) {
                f.ifd0.push((315, text(a)));
            }
            if let Some(c) = info.copyright.as_deref().filter(|s| !s.trim().is_empty()) {
                f.ifd0.push((33432, text(c)));
            }
        }
    };
    if policy == MetadataPolicy::CopyrightOnly {
        credit(&mut f);
        return f;
    }

    if let Some(info) = info {
        if !info.make.trim().is_empty() {
            f.ifd0.push((271, text(&info.make)));
        }
        if !info.model.trim().is_empty() {
            f.ifd0.push((272, text(&info.model)));
        }
    }
    // The pixels are already turned the way the photo is meant to be seen.
    f.ifd0.push((274, Value::Short(vec![1])));
    f.ifd0.push((305, text("Duckfoot")));
    f.ifd0.push((306, text(now)));
    credit(&mut f);

    f.exif.push((36864, Value::Undefined(b"0232".to_vec())));
    if let Some(info) = info {
        if let Some((n, d)) = info.exposure_time.filter(|(n, d)| *n > 0 && *d > 0) {
            f.exif.push((33434, Value::Rational(vec![(n, d)])));
        }
        if let Some(fnum) = info.f_number.filter(|v| *v > 0.0) {
            f.exif
                .push((33437, Value::Rational(vec![rational(fnum as f64, 10)])));
        }
        if let Some(iso) = info.iso.filter(|v| *v > 0) {
            f.exif
                .push((34855, Value::Short(vec![iso.min(65535) as u16])));
        }
        if let Some(when) = info.captured.as_deref() {
            f.exif.push((36867, text(when)));
            f.exif.push((36868, text(when)));
        }
        if let Some(mm) = info.focal_length.filter(|v| *v > 0.0) {
            f.exif
                .push((37386, Value::Rational(vec![rational(mm as f64, 10)])));
        }
        if let Some(lens) = info.lens.as_deref().filter(|s| !s.trim().is_empty()) {
            f.exif.push((42036, text(lens)));
        }
        if let (Some((lat, lon)), false) = (info.gps, strip_location) {
            let dms = |v: f64| {
                let v = v.abs();
                let d = v.floor();
                let m = ((v - d) * 60.0).floor();
                let s = (v - d - m / 60.0) * 3600.0;
                Value::Rational(vec![(d as u32, 1), (m as u32, 1), rational(s, 1000)])
            };
            f.gps.push((0, Value::Byte(vec![2, 3, 0, 0])));
            f.gps.push((1, text(if lat >= 0.0 { "N" } else { "S" })));
            f.gps.push((2, dms(lat)));
            f.gps.push((3, text(if lon >= 0.0 { "E" } else { "W" })));
            f.gps.push((4, dms(lon)));
        }
    }
    for dir in [&mut f.ifd0, &mut f.exif, &mut f.gps] {
        dir.sort_by_key(|(tag, _)| *tag);
    }
    f
}

/// One directory at `base`: entry count, entries, next-IFD pointer (0), then the values
/// too large to sit in an entry.
fn directory(entries: &[(u16, Value)], base: u32) -> Vec<u8> {
    let head = 2 + 12 * entries.len() + 4;
    let mut table = Vec::with_capacity(head);
    let mut data = Vec::new();
    table.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, value) in entries {
        let (kind, count, bytes) = value.encode();
        table.extend_from_slice(&tag.to_le_bytes());
        table.extend_from_slice(&kind.to_le_bytes());
        table.extend_from_slice(&count.to_le_bytes());
        if bytes.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..bytes.len()].copy_from_slice(&bytes);
            table.extend_from_slice(&inline);
        } else {
            let at = base + head as u32 + data.len() as u32;
            table.extend_from_slice(&at.to_le_bytes());
            data.extend_from_slice(&bytes);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    table.extend_from_slice(&0u32.to_le_bytes());
    table.extend_from_slice(&data);
    table
}

/// The whole block, as JPEG, PNG and WebP embed it. `None` when there is nothing to say.
pub(crate) fn block(fields: &Fields) -> Option<Vec<u8>> {
    if fields.is_empty() {
        return None;
    }
    // Directory sizes do not depend on the pointer values, so lay them out with
    // placeholders first, then for real.
    let with_pointers = |exif_at: u32, gps_at: u32| {
        let mut ifd0 = fields.ifd0.clone();
        if !fields.exif.is_empty() {
            ifd0.push((TAG_EXIF_IFD, Value::Long(exif_at)));
        }
        if !fields.gps.is_empty() {
            ifd0.push((TAG_GPS_IFD, Value::Long(gps_at)));
        }
        ifd0.sort_by_key(|(tag, _)| *tag);
        ifd0
    };
    let ifd0_len = directory(&with_pointers(0, 0), 8).len() as u32;
    let exif_at = 8 + ifd0_len;
    let exif = if fields.exif.is_empty() {
        Vec::new()
    } else {
        directory(&fields.exif, exif_at)
    };
    let gps_at = exif_at + exif.len() as u32;
    let gps = if fields.gps.is_empty() {
        Vec::new()
    } else {
        directory(&fields.gps, gps_at)
    };

    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&directory(&with_pointers(exif_at, gps_at), 8));
    out.extend_from_slice(&exif);
    out.extend_from_slice(&gps);
    Some(out)
}

/// Now, in EXIF's format, in UTC.
pub(crate) fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = crate::metadata::civil_from_days((secs / 86_400) as i64);
    let t = secs % 86_400;
    format!(
        "{y:04}:{m:02}:{d:02} {:02}:{:02}:{:02}",
        t / 3600,
        t / 60 % 60,
        t % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> PhotoInfo {
        PhotoInfo {
            make: "SONY".into(),
            model: "ILCE-7M4".into(),
            lens: Some("FE 35mm F1.8".into()),
            exposure_time: Some((1, 250)),
            f_number: Some(2.8),
            iso: Some(400),
            focal_length: Some(35.0),
            captured: Some("2026:05:04 10:11:12".into()),
            gps: Some((48.8584, -2.2945)),
            artist: Some("A. Photographer".into()),
            copyright: Some("(c) 2026 A. Photographer".into()),
            ..Default::default()
        }
    }

    fn read(block: &[u8]) -> exif::Exif {
        exif::Reader::new().read_raw(block.to_vec()).unwrap()
    }

    #[test]
    fn every_field_reads_back() {
        let info = sample();
        let f = fields(
            Some(&info),
            MetadataPolicy::All,
            false,
            "2026:09:26 12:00:00",
        );
        let exif = read(&block(&f).unwrap());
        let get = |tag, ifd| {
            exif.get_field(tag, ifd)
                .map(|f| f.display_value().to_string())
        };
        use exif::{In, Tag};
        assert_eq!(get(Tag::Make, In::PRIMARY).as_deref(), Some("\"SONY\""));
        assert_eq!(
            get(Tag::Model, In::PRIMARY).as_deref(),
            Some("\"ILCE-7M4\"")
        );
        assert_eq!(
            get(Tag::Orientation, In::PRIMARY).as_deref(),
            Some("row 0 at top and column 0 at left")
        );
        assert_eq!(
            get(Tag::ExposureTime, In::PRIMARY).as_deref(),
            Some("1/250")
        );
        assert_eq!(get(Tag::FNumber, In::PRIMARY).as_deref(), Some("2.8"));
        assert_eq!(
            get(Tag::PhotographicSensitivity, In::PRIMARY).as_deref(),
            Some("400")
        );
        assert_eq!(get(Tag::FocalLength, In::PRIMARY).as_deref(), Some("35"));
        assert_eq!(
            get(Tag::DateTimeOriginal, In::PRIMARY).as_deref(),
            Some("2026-05-04 10:11:12")
        );
        assert_eq!(
            get(Tag::LensModel, In::PRIMARY).as_deref(),
            Some("\"FE 35mm F1.8\"")
        );
        assert!(get(Tag::Copyright, In::PRIMARY).is_some());
        assert!(get(Tag::GPSLatitude, In::PRIMARY).is_some());
        assert_eq!(get(Tag::GPSLongitudeRef, In::PRIMARY).as_deref(), Some("W"));
    }

    #[test]
    fn location_is_stripped_and_copyright_only_is_only_the_credit() {
        let info = sample();
        let stripped = fields(Some(&info), MetadataPolicy::All, true, "x");
        assert!(stripped.gps.is_empty());
        let exif = read(&block(&stripped).unwrap());
        assert!(exif
            .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
            .is_none());

        let credit = fields(Some(&info), MetadataPolicy::CopyrightOnly, false, "x");
        let tags: Vec<u16> = credit.ifd0.iter().map(|(t, _)| *t).collect();
        assert_eq!(tags, [315, 33432]);
        assert!(credit.exif.is_empty() && credit.gps.is_empty());

        assert!(block(&fields(Some(&info), MetadataPolicy::None, false, "x")).is_none());
    }

    #[test]
    fn a_photo_with_no_metadata_still_says_it_is_upright() {
        let f = fields(None, MetadataPolicy::All, true, "2026:09:26 12:00:00");
        let exif = read(&block(&f).unwrap());
        let o = exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap();
        assert_eq!(o.value.get_uint(0), Some(1));
    }
}

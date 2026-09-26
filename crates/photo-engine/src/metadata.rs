//! What a photo's file says about how it was taken: camera, lens, exposure, time, place.
//!
//! Read from the header only — no pixels are decoded — so it is cheap enough to do for
//! every photo on the shelf, on a worker thread. RAW files go through rawler's own
//! metadata parser, which knows every container it can decode (including CR3 and RAF,
//! whose orientation does not live in a TIFF header).

use std::path::Path;

use crate::params::Geometry;

/// The facts the Info panel, the renamer and the exporter's EXIF block need.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhotoInfo {
    pub make: String,
    pub model: String,
    pub lens: Option<String>,
    /// Seconds, as the camera wrote it: numerator and denominator (1/250 s is (1, 250)).
    pub exposure_time: Option<(u32, u32)>,
    pub f_number: Option<f32>,
    pub iso: Option<u32>,
    /// Millimetres, actual (not 35 mm equivalent).
    pub focal_length: Option<f32>,
    /// `YYYY:MM:DD HH:MM:SS`, EXIF's own format, in the camera's local time.
    pub captured: Option<String>,
    /// Decimal degrees, north and east positive.
    pub gps: Option<(f64, f64)>,
    pub artist: Option<String>,
    pub copyright: Option<String>,
    /// How the camera meant the frame to be viewed.
    pub orientation: Geometry,
}

impl PhotoInfo {
    /// "1/250 s", "2 s", "0.8 s".
    pub fn shutter(&self) -> Option<String> {
        let (n, d) = self.exposure_time?;
        if n == 0 || d == 0 {
            return None;
        }
        let seconds = n as f64 / d as f64;
        Some(if seconds >= 0.5 {
            let s = format!("{seconds:.1}");
            format!("{} s", s.trim_end_matches(".0"))
        } else {
            format!("1/{:.0} s", 1.0 / seconds)
        })
    }

    /// `YYYYMMDD` of the capture, for filenames.
    pub fn capture_date_compact(&self) -> Option<String> {
        let date = self.captured.as_deref()?.get(0..10)?;
        let digits: String = date.chars().filter(|c| c.is_ascii_digit()).collect();
        (digits.len() == 8).then_some(digits)
    }
}

/// The metadata of the RAW at `path`, or `None` if its container cannot be read. A
/// parser that panics on a hostile header costs this one photo its Info, nothing more.
pub fn read_raw_info(path: &Path) -> Option<PhotoInfo> {
    std::panic::catch_unwind(|| raw_info(path)).ok().flatten()
}

fn raw_info(path: &Path) -> Option<PhotoInfo> {
    use rawler::decoders::RawDecodeParams;
    let source = rawler::rawsource::RawSource::new(path).ok()?;
    let decoder = rawler::get_decoder(&source).ok()?;
    let meta = decoder
        .raw_metadata(&source, &RawDecodeParams::default())
        .ok()?;
    let exif = &meta.exif;
    let ratio = |r: &rawler::formats::tiff::Rational| (r.d != 0).then(|| r.n as f32 / r.d as f32);
    let lens = meta
        .lens
        .as_ref()
        .map(|l| {
            format!("{} {}", l.lens_make, l.lens_model)
                .trim()
                .to_string()
        })
        .or_else(|| exif.lens_model.clone())
        .filter(|l| !l.is_empty());
    Some(PhotoInfo {
        make: meta.make.clone(),
        model: meta.model.clone(),
        lens,
        exposure_time: exif.exposure_time.map(|r| (r.n, r.d)),
        f_number: exif.fnumber.as_ref().and_then(ratio),
        iso: exif
            .iso_speed_ratings
            .map(u32::from)
            .or(exif.recommended_exposure_index)
            .or(exif.iso_speed),
        focal_length: exif.focal_length.as_ref().and_then(ratio),
        captured: exif
            .date_time_original
            .clone()
            .or_else(|| exif.create_date.clone()),
        gps: exif.gps.as_ref().and_then(gps_degrees),
        artist: exif.artist.clone().filter(|s| !s.trim().is_empty()),
        copyright: exif.copyright.clone().filter(|s| !s.trim().is_empty()),
        orientation: exif
            .orientation
            .map(crate::decode::orientation_from_exif)
            .unwrap_or_default(),
    })
}

fn gps_degrees(gps: &rawler::exif::ExifGPS) -> Option<(f64, f64)> {
    let dms = |v: &[rawler::formats::tiff::Rational; 3]| -> Option<f64> {
        let part =
            |r: &rawler::formats::tiff::Rational| (r.d != 0).then(|| r.n as f64 / r.d as f64);
        Some(part(&v[0])? + part(&v[1])? / 60.0 + part(&v[2])? / 3600.0)
    };
    let mut lat = dms(gps.gps_latitude.as_ref()?)?;
    let mut lon = dms(gps.gps_longitude.as_ref()?)?;
    if gps.gps_latitude_ref.as_deref() == Some("S") {
        lat = -lat;
    }
    if gps.gps_longitude_ref.as_deref() == Some("W") {
        lon = -lon;
    }
    Some((lat, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter_speeds_read_the_way_cameras_print_them() {
        let with = |n, d| PhotoInfo {
            exposure_time: Some((n, d)),
            ..Default::default()
        };
        assert_eq!(with(1, 250).shutter().as_deref(), Some("1/250 s"));
        assert_eq!(with(2, 1).shutter().as_deref(), Some("2 s"));
        assert_eq!(with(8, 10).shutter().as_deref(), Some("0.8 s"));
        assert_eq!(with(0, 1).shutter(), None);
    }

    #[test]
    fn a_capture_time_becomes_a_filename_date() {
        let info = PhotoInfo {
            captured: Some("2026:09:24 17:03:12".into()),
            ..Default::default()
        };
        assert_eq!(info.capture_date_compact().as_deref(), Some("20260924"));
        assert_eq!(PhotoInfo::default().capture_date_compact(), None);
    }

    #[test]
    fn a_dng_header_is_read_without_decoding_it() {
        let dir = std::env::temp_dir().join(format!("duckfoot-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.dng");
        std::fs::write(&path, crate::testkit::synth_dng(64, 48, false).unwrap()).unwrap();
        let info = read_raw_info(&path).expect("the DNG's metadata");
        assert_eq!(info.orientation, Geometry::default());
        // Not a RAW at all: no metadata, and no panic.
        std::fs::write(&path, b"II*\0not a raw").unwrap();
        assert_eq!(read_raw_info(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

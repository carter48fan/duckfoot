//! Lens profiles from the lensfun database (DEC-8: the data only, CC BY-SA 3.0, never the
//! library). `tools/lensfun_extract.py` distils the XML into `data/lensfun.tsv.z`; this
//! reads it, finds the photo's camera and lens, and gives the corrections at the photo's
//! focal length and aperture as a [`FileWarp`] the adjust pass applies like a DNG's own.
//!
//! The models, as lensfun defines them (all map a corrected point to where the lens put
//! it, which is the direction the shader works in):
//! - distortion, `ptlens`: r' = r·(a·r³ + b·r² + c·r + 1 − a − b − c); `poly3`:
//!   r' = r·(1 − k1 + k1·r²); `poly5`: r' = r·(1 + k1·r² + k2·r⁴). r = 1 is half the
//!   calibration image's short side.
//! - TCA, applied after distortion: `linear`: r' = k·r; `poly3`: r' = r·(v + c·r + b·r²),
//!   per red and blue, in the same units.
//! - vignetting, `pa`: what was recorded is the scene times 1 + k1·r² + k2·r⁴ + k3·r⁶,
//!   r = 1 at the calibration image's corner; correcting divides by it.
//!
//! A calibration made on another sensor size carries over through physical size: both
//! radii are fixed millimetres on the calibration sensor, and a photo's pixels are placed
//! in millimetres by its own crop factor.

use std::sync::OnceLock;

use crate::lens::FileWarp;

/// Half the diagonal of a 36 × 24 mm frame.
const FULL_FRAME_HALF_DIAGONAL: f32 = 21.633_308;

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub maker: String,
    pub model: String,
    pub mount: String,
    pub crop: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Distortion {
    PtLens([f32; 3]),
    Poly3(f32),
    Poly5([f32; 2]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tca {
    Linear {
        red: f32,
        blue: f32,
    },
    /// v, c, b for red, then for blue.
    Poly3 {
        red: [f32; 3],
        blue: [f32; 3],
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lens {
    pub maker: String,
    pub model: String,
    pub mounts: Vec<String>,
    /// The crop factor and aspect ratio of the camera the calibration was made on.
    pub crop: f32,
    pub aspect: f32,
    distortion: Vec<(f32, Distortion)>,
    tca: Vec<(f32, Tca)>,
    /// Focal, aperture, k1..k3.
    vignetting: Vec<(f32, f32, [f32; 3])>,
}

#[derive(Debug, Default)]
pub struct Database {
    pub cameras: Vec<Camera>,
    pub lenses: Vec<Lens>,
}

/// The bundled database, read on first use (a quarter of a megabyte, inflated once).
pub fn database() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    DB.get_or_init(|| {
        let packed = include_bytes!("../data/lensfun.tsv.z");
        match miniz_oxide::inflate::decompress_to_vec_zlib(packed) {
            Ok(bytes) => Database::parse(&String::from_utf8_lossy(&bytes)),
            Err(e) => {
                log::error!("the lens database did not inflate: {e:?}");
                Database::default()
            }
        }
    })
}

impl Database {
    pub fn parse(text: &str) -> Self {
        let mut db = Database::default();
        let f = |s: Option<&str>| s.and_then(|v| v.parse::<f32>().ok());
        for line in text.lines() {
            let mut t = line.split('\t');
            match t.next() {
                Some("C") => {
                    let (maker, model, mount, crop) = (t.next(), t.next(), t.next(), f(t.next()));
                    if let (Some(maker), Some(model), Some(mount), Some(crop)) =
                        (maker, model, mount, crop)
                    {
                        db.cameras.push(Camera {
                            maker: maker.into(),
                            model: model.into(),
                            mount: mount.into(),
                            crop,
                        });
                    }
                }
                Some("L") => {
                    let (maker, model, mounts) = (t.next(), t.next(), t.next());
                    let (crop, aspect) = (f(t.next()), f(t.next()));
                    if let (Some(maker), Some(model), Some(mounts), Some(crop), Some(aspect)) =
                        (maker, model, mounts, crop, aspect)
                    {
                        db.lenses.push(Lens {
                            maker: maker.into(),
                            model: model.into(),
                            mounts: mounts.split(';').map(String::from).collect(),
                            crop,
                            aspect: aspect.max(1.0),
                            distortion: Vec::new(),
                            tca: Vec::new(),
                            vignetting: Vec::new(),
                        });
                    }
                }
                Some(kind @ ("D" | "T" | "V")) => {
                    let Some(lens) = db.lenses.last_mut() else {
                        continue;
                    };
                    let fields: Vec<&str> = t.collect();
                    let n = |i: usize| f(fields.get(i).copied());
                    let Some(focal) = n(0) else { continue };
                    match (kind, fields.get(1).copied()) {
                        ("D", Some("ptlens")) => {
                            if let (Some(a), Some(b), Some(c)) = (n(2), n(3), n(4)) {
                                lens.distortion.push((focal, Distortion::PtLens([a, b, c])));
                            }
                        }
                        ("D", Some("poly3")) => {
                            if let Some(k1) = n(2) {
                                lens.distortion.push((focal, Distortion::Poly3(k1)));
                            }
                        }
                        ("D", Some("poly5")) => {
                            if let (Some(k1), Some(k2)) = (n(2), n(3)) {
                                lens.distortion.push((focal, Distortion::Poly5([k1, k2])));
                            }
                        }
                        ("T", Some("linear")) => {
                            if let (Some(red), Some(blue)) = (n(2), n(3)) {
                                lens.tca.push((focal, Tca::Linear { red, blue }));
                            }
                        }
                        ("T", Some("poly3")) => {
                            let k: Option<Vec<f32>> = (2..8).map(n).collect();
                            if let Some(k) = k {
                                lens.tca.push((
                                    focal,
                                    Tca::Poly3 {
                                        red: [k[0], k[1], k[2]],
                                        blue: [k[3], k[4], k[5]],
                                    },
                                ));
                            }
                        }
                        ("V", _) => {
                            if let (Some(aperture), Some(k1), Some(k2), Some(k3)) =
                                (n(1), n(2), n(3), n(4))
                            {
                                lens.vignetting.push((focal, aperture, [k1, k2, k3]));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        for lens in &mut db.lenses {
            lens.distortion.sort_by(|a, b| a.0.total_cmp(&b.0));
            lens.tca.sort_by(|a, b| a.0.total_cmp(&b.0));
            lens.vignetting
                .sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        }
        db
    }

    /// The camera, by the maker and model its files name.
    pub fn camera(&self, make: &str, model: &str) -> Option<&Camera> {
        let (make, model) = (squash(make), squash(model));
        self.cameras.iter().find(|c| {
            let m = squash(&c.maker);
            (make.contains(&m) || m.contains(&make)) && squash(&c.model) == model
        })
    }

    /// The lens a file's `LensModel` names, taken on `camera` if it is known: the same
    /// focal lengths and apertures exactly, and most of the rest of the name. Among
    /// calibrations of one lens, the one made on the sensor size nearest this one.
    pub fn lens(&self, name: &str, camera: Option<&Camera>) -> Option<&Lens> {
        let want = Tokens::new(name);
        if want.numbers.is_empty() {
            return None;
        }
        let crop = camera.map_or(1.0, |c| c.crop);
        self.lenses
            .iter()
            .filter_map(|lens| {
                let have = Tokens::new(&lens.model);
                if have.numbers != want.numbers {
                    return None;
                }
                // The maker's name is often in one string and not the other.
                let maker = Tokens::new(&lens.maker);
                let words = |t: &Tokens| -> Vec<String> {
                    t.words
                        .iter()
                        .filter(|w| !maker.words.contains(w))
                        .cloned()
                        .collect()
                };
                let (a, b) = (words(&want), words(&have));
                let common = a.iter().filter(|w| b.contains(w)).count();
                let union = a.len() + b.len() - common;
                let likeness = if union == 0 {
                    1.0
                } else {
                    common as f32 / union as f32
                };
                if likeness < 0.5 {
                    return None;
                }
                let mount = camera.is_some_and(|c| lens.mounts.contains(&c.mount));
                let score = likeness + if mount { 0.25 } else { 0.0 }
                    - (lens.crop / crop).ln().abs() * 0.01;
                Some((score, lens))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, lens)| lens)
    }
}

/// Lowercase letters and digits only, for comparing camera names ("ILCE-7M4" and
/// "ilce7m4" are the same camera).
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A lens name as the numbers that must match (focal lengths, apertures: "24-70",
/// "2.8") and the words that mostly should. "f", "mm" and punctuation carry nothing.
struct Tokens {
    numbers: Vec<String>,
    words: Vec<String>,
}

impl Tokens {
    fn new(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        let mut numbers = Vec::new();
        let mut words = Vec::new();
        let mut chars = lower.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                // A number, or a range of them: "24.0-70.0" reads as "24-70".
                let mut run = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' || d == '-' {
                        run.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let parts: Vec<String> = run
                    .trim_matches(|c| c == '-' || c == '.')
                    .split('-')
                    .filter_map(|p| p.parse::<f32>().ok())
                    .map(|v| format!("{v}"))
                    .collect();
                if !parts.is_empty() {
                    numbers.push(parts.join("-"));
                }
            } else if c.is_ascii_alphabetic() {
                let mut run = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_alphabetic() {
                        run.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if run != "f" && run != "mm" {
                    words.push(run);
                }
            } else {
                chars.next();
            }
        }
        numbers.sort();
        Self { numbers, words }
    }
}

/// Linear between the two calibrations either side of `x`, held beyond them.
fn between<T: Copy>(
    points: &[(f32, T)],
    x: f32,
    lerp: impl Fn(T, T, f32) -> Option<T>,
) -> Option<T> {
    let first = points.first()?;
    if x <= first.0 {
        return Some(first.1);
    }
    let last = points.last()?;
    if x >= last.0 {
        return Some(last.1);
    }
    let i = points.iter().position(|p| p.0 > x)?;
    let (a, b) = (points[i - 1], points[i]);
    let t = (x - a.0) / (b.0 - a.0).max(1e-6);
    // Two models either side cannot be blended; the nearer one stands.
    lerp(a.1, b.1, t).or(Some(if t < 0.5 { a.1 } else { b.1 }))
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// What a lens does at one focal length and aperture.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// "Sony FE 24-70mm f/2.8 GM", for the Lens card to say what it found.
    pub name: String,
    /// Distortion as r' = r·Σ cᵢ·rⁱ, in lensfun's distortion units.
    distortion: Option<[f32; 8]>,
    /// v, c, b for red and for blue, in the same units.
    tca: Option<[[f32; 3]; 2]>,
    vignetting: Option<[f32; 3]>,
    /// The calibration's crop factor and aspect ratio, which fix those units.
    crop: f32,
    aspect: f32,
}

impl Lens {
    /// The corrections at `focal` mm and `aperture` (f-number), if either is known.
    pub fn profile(&self, focal: f32, aperture: Option<f32>) -> Profile {
        let distortion = between(&self.distortion, focal, |a, b, t| match (a, b) {
            (Distortion::PtLens(a), Distortion::PtLens(b)) => Some(Distortion::PtLens([
                mix(a[0], b[0], t),
                mix(a[1], b[1], t),
                mix(a[2], b[2], t),
            ])),
            (Distortion::Poly3(a), Distortion::Poly3(b)) => Some(Distortion::Poly3(mix(a, b, t))),
            (Distortion::Poly5(a), Distortion::Poly5(b)) => {
                Some(Distortion::Poly5([mix(a[0], b[0], t), mix(a[1], b[1], t)]))
            }
            _ => None,
        })
        .map(|d| match d {
            Distortion::PtLens([a, b, c]) => [1.0 - a - b - c, c, b, a, 0.0, 0.0, 0.0, 0.0],
            Distortion::Poly3(k1) => [1.0 - k1, 0.0, k1, 0.0, 0.0, 0.0, 0.0, 0.0],
            Distortion::Poly5([k1, k2]) => [1.0, 0.0, k1, 0.0, k2, 0.0, 0.0, 0.0],
        });
        let as_poly = |t: Tca| match t {
            Tca::Linear { red, blue } => [[red, 0.0, 0.0], [blue, 0.0, 0.0]],
            Tca::Poly3 { red, blue } => [red, blue],
        };
        let tca: Vec<(f32, [[f32; 3]; 2])> =
            self.tca.iter().map(|(f, t)| (*f, as_poly(*t))).collect();
        let tca = between(&tca, focal, |a, b, t| {
            let m = |x: [f32; 3], y: [f32; 3]| {
                [mix(x[0], y[0], t), mix(x[1], y[1], t), mix(x[2], y[2], t)]
            };
            Some([m(a[0], b[0]), m(a[1], b[1])])
        });
        let vignetting = aperture.and_then(|n| self.vignetting_at(focal, n));
        Profile {
            name: if self
                .model
                .to_ascii_lowercase()
                .contains(&self.maker.to_ascii_lowercase())
            {
                self.model.clone()
            } else {
                format!("{} {}", self.maker, self.model)
            },
            distortion,
            tca,
            vignetting,
            crop: self.crop,
            aspect: self.aspect,
        }
    }

    /// Vignetting at each calibrated focal length for this aperture (linear in stops),
    /// then between focal lengths.
    fn vignetting_at(&self, focal: f32, aperture: f32) -> Option<[f32; 3]> {
        let mut focals: Vec<f32> = self.vignetting.iter().map(|v| v.0).collect();
        focals.dedup();
        let per_focal: Vec<(f32, [f32; 3])> = focals
            .iter()
            .filter_map(|&f| {
                let stops: Vec<(f32, [f32; 3])> = self
                    .vignetting
                    .iter()
                    .filter(|v| v.0 == f)
                    .map(|v| (v.1.max(0.5).log2(), v.2))
                    .collect();
                let k = between(&stops, aperture.max(0.5).log2(), |a, b, t| {
                    Some([mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)])
                })?;
                Some((f, k))
            })
            .collect();
        between(&per_focal, focal, |a, b, t| {
            Some([mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)])
        })
    }
}

impl Profile {
    pub fn is_empty(&self) -> bool {
        self.distortion.is_none() && self.tca.is_none() && self.vignetting.is_none()
    }

    /// The warp for a `sensor`-sized image from a camera of crop factor `crop`.
    pub fn warp(&self, sensor: (u32, u32), crop: f32) -> Option<FileWarp> {
        if self.is_empty() {
            return None;
        }
        let (w, h) = (sensor.0 as f32, sensor.1 as f32);
        // Pixels per millimetre on this camera's sensor.
        let per_mm = (w.hypot(h) * 0.5) / (FULL_FRAME_HALF_DIAGONAL / crop.max(0.1));
        let cal_half_diagonal = FULL_FRAME_HALF_DIAGONAL / self.crop.max(0.1);
        let unit = cal_half_diagonal / self.aspect.hypot(1.0) * per_mm;
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let d = self.distortion.unwrap_or(identity);
        Some(FileWarp {
            centre: [w * 0.5, h * 0.5],
            m: unit,
            planes: [d, d, d],
            tca: self.tca,
            vignetting: self.vignetting.map(|k| (k, cal_half_diagonal * per_mm)),
        })
    }
}

/// The profile for a photo, from what its EXIF says: the camera for the sensor size, the
/// lens by name, the focal length and aperture it was shot at. `sensor` is the decoded
/// mosaic's size. The name is the lens as the database knows it.
pub fn for_photo(info: &crate::PhotoInfo, sensor: (u32, u32)) -> Option<(String, FileWarp)> {
    let db = database();
    let camera = db.camera(&info.make, &info.model);
    let lens = db.lens(info.lens.as_deref()?, camera)?;
    let profile = lens.profile(info.focal_length?, info.f_number);
    let warp = profile.warp(sensor, camera.map_or(lens.crop, |c| c.crop))?;
    Some((profile.name, warp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_database_reads() {
        let db = database();
        assert!(db.cameras.len() > 500, "{} cameras", db.cameras.len());
        assert!(db.lenses.len() > 1000, "{} lenses", db.lenses.len());
    }

    #[test]
    fn a_camera_is_found_by_its_files_own_names() {
        let db = database();
        let a7 = db.camera("SONY", "ILCE-7M4").expect("the A7 IV");
        assert_eq!(a7.crop, 1.0);
        assert_eq!(a7.mount, "Sony E");
    }

    #[test]
    fn a_lens_is_found_by_the_name_its_exif_gives() {
        let db = database();
        let a7 = db.camera("SONY", "ILCE-7M4");
        // Sony writes "F2.8"; lensfun "f/2.8".
        let gm = db.lens("FE 24-70mm F2.8 GM", a7).expect("found");
        assert_eq!(gm.model, "FE 24-70mm f/2.8 GM");
        // Nikon writes its focal lengths with decimals.
        let nikon = db.camera("NIKON CORPORATION", "NIKON D850");
        let z = db.lens("AF-S Nikkor 24.0-70.0 mm f/2.8G ED", nikon);
        assert!(
            z.is_some_and(|l| l.model.contains("24-70mm f/2.8G")),
            "{z:?}"
        );
        // Numbers are not negotiable: no 24-105 for a 24-70.
        assert!(db
            .lens("FE 24-105mm F2.8 GM", a7)
            .is_none_or(|l| l.model.contains("24-105")));
        assert!(db.lens("", a7).is_none());
    }

    #[test]
    fn a_profile_interpolates_between_focal_lengths() {
        let db = Database::parse(
            "L\tAcme\t10-20mm f/4\tX\t1.5\t1.5\n\
             D\t10\tpoly3\t-0.02\n\
             D\t20\tpoly3\t0.02\n\
             V\t10\t4\t-0.5\t0\t0\n\
             V\t10\t8\t-0.1\t0\t0\n",
        );
        let lens = &db.lenses[0];
        let p = lens.profile(15.0, Some(5.6));
        assert_eq!(p.distortion.unwrap()[2], 0.0);
        let p = lens.profile(10.0, Some(4.0));
        assert!((p.distortion.unwrap()[2] + 0.02).abs() < 1e-6);
        assert!((p.vignetting.unwrap()[0] + 0.5).abs() < 1e-6);
        // Half way in stops between f/4 and f/8.
        let p = lens.profile(10.0, Some(5.657));
        assert!((p.vignetting.unwrap()[0] + 0.3).abs() < 1e-3);
        // Beyond the calibrations, held at the last.
        assert!((lens.profile(40.0, None).distortion.unwrap()[2] - 0.02).abs() < 1e-6);
        assert!(lens.profile(10.0, None).vignetting.is_none());
    }

    #[test]
    fn units_follow_the_calibration_sensor() {
        // Calibrated on APS-C 3:2, used on full frame: the same lens covers more of
        // the full-frame sensor's pixels per millimetre... and lensfun's r = 1 (half
        // the APS-C frame's short side, 7.9 mm) is a smaller share of this frame.
        let db = Database::parse("L\tAcme\t35mm f/2\tX\t1.5\t1.5\nD\t35\tpoly3\t0.01\n");
        let p = db.lenses[0].profile(35.0, None);
        let (w, h) = (6000u32, 4000u32);
        let on_ff = p.warp((w, h), 1.0).unwrap();
        let on_apsc = p.warp((w, h), 1.5).unwrap();
        // On APS-C, r = 1 is half the image height exactly.
        assert!((on_apsc.m - 2000.0).abs() < 1.0, "{}", on_apsc.m);
        assert!((on_ff.m - 2000.0 / 1.5).abs() < 1.0, "{}", on_ff.m);
    }
}

//! The sRGB colour profile every export carries, so a colour-managed viewer shows the
//! file as it looked in Duckfoot instead of guessing (and a wide-gamut screen guessing
//! wrong oversaturates everything).
//!
//! Built here rather than bundled: an ICC v4 display profile for sRGB is a header, a tag
//! table and nine small tags, all of it public constants — the IEC 61966-2-1 primaries
//! adapted to the D50 connection space with Bradford, and the sRGB curve as ICC's
//! parametric curve type 3. Some 550 bytes, no licence to track.

use std::sync::OnceLock;

/// The profile's bytes, built once.
pub fn srgb_profile() -> &'static [u8] {
    static PROFILE: OnceLock<Vec<u8>> = OnceLock::new();
    PROFILE.get_or_init(build)
}

/// ICC's s15Fixed16Number.
fn fixed(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for v in [x, y, z] {
        t.extend_from_slice(&fixed(v));
    }
    t
}

/// multiLocalizedUnicodeType with one en-US record.
fn mluc(text: &str) -> Vec<u8> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut t = b"mluc\0\0\0\0".to_vec();
    t.extend_from_slice(&1u32.to_be_bytes()); // records
    t.extend_from_slice(&12u32.to_be_bytes()); // record size
    t.extend_from_slice(b"enUS");
    t.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    t.extend_from_slice(&28u32.to_be_bytes()); // offset of the string in the tag
    t.extend_from_slice(&utf16);
    t
}

fn build() -> Vec<u8> {
    // D50, the profile connection space's white.
    const D50: (f64, f64, f64) = (0.9642, 1.0, 0.8249);
    // The sRGB curve: Y = (aX + b)^g for X >= d, else cX.
    let mut para = b"para\0\0\0\0".to_vec();
    para.extend_from_slice(&3u16.to_be_bytes());
    para.extend_from_slice(&[0, 0]);
    for v in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045] {
        para.extend_from_slice(&fixed(v));
    }
    // Bradford D65 → D50: what the viewer undoes to get back to the display's white.
    let mut chad = b"sf32\0\0\0\0".to_vec();
    for v in [
        1.047882, 0.022918, -0.050217, 0.029586, 0.990478, -0.017075, -0.009247, 0.015075, 0.751678,
    ] {
        chad.extend_from_slice(&fixed(v));
    }
    // (signature, data); the three curves share one block.
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", mluc("sRGB (Duckfoot)")),
        (b"cprt", mluc("No copyright, use freely")),
        (b"wtpt", xyz(D50.0, D50.1, D50.2)),
        (b"chad", chad),
        (b"rXYZ", xyz(0.436066, 0.222488, 0.013916)),
        (b"gXYZ", xyz(0.385147, 0.716873, 0.097076)),
        (b"bXYZ", xyz(0.143066, 0.060608, 0.714096)),
        (b"rTRC", para.clone()),
        (b"gTRC", para.clone()),
        (b"bTRC", para),
    ];

    let table_len = 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut entries = Vec::new();
    let mut curve_at: Option<(u32, u32)> = None;
    for (sig, body) in &tags {
        let is_curve = sig.ends_with(b"TRC");
        let (offset, size) = match (is_curve, curve_at) {
            (true, Some(shared)) => shared,
            _ => {
                let offset = (128 + table_len + data.len()) as u32;
                data.extend_from_slice(body);
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                (offset, body.len() as u32)
            }
        };
        if is_curve {
            curve_at = Some((offset, size));
        }
        entries.push((*sig, offset, size));
    }
    let total = 128 + table_len + data.len();

    let mut p = Vec::with_capacity(total);
    p.extend_from_slice(&(total as u32).to_be_bytes());
    p.extend_from_slice(&[0; 4]); // preferred CMM: none
    p.extend_from_slice(&0x0430_0000u32.to_be_bytes()); // version 4.3
    p.extend_from_slice(b"mntrRGB XYZ ");
    for v in [2026u16, 1, 1, 0, 0, 0] {
        p.extend_from_slice(&v.to_be_bytes());
    }
    p.extend_from_slice(b"acsp");
    p.extend_from_slice(&[0; 4]); // platform
    p.extend_from_slice(&[0; 4]); // flags
    p.extend_from_slice(&[0; 8]); // manufacturer, model
    p.extend_from_slice(&[0; 8]); // attributes
    p.extend_from_slice(&0u32.to_be_bytes()); // perceptual
    for v in [D50.0, D50.1, D50.2] {
        p.extend_from_slice(&fixed(v));
    }
    p.extend_from_slice(&[0; 4]); // creator
    p.extend_from_slice(&[0; 16]); // profile ID: not computed, which v4 allows
    p.extend_from_slice(&[0; 28]);
    debug_assert_eq!(p.len(), 128);
    p.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (sig, offset, size) in entries {
        p.extend_from_slice(sig);
        p.extend_from_slice(&offset.to_be_bytes());
        p.extend_from_slice(&size.to_be_bytes());
    }
    p.extend_from_slice(&data);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(b: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn the_profile_is_well_formed() {
        let p = srgb_profile();
        assert_eq!(be32(p, 0) as usize, p.len(), "size field");
        assert_eq!(&p[36..40], b"acsp");
        assert_eq!(&p[12..24], b"mntrRGB XYZ ");
        let count = be32(p, 128) as usize;
        assert_eq!(count, 10);
        for i in 0..count {
            let at = 132 + 12 * i;
            let (offset, size) = (be32(p, at + 4) as usize, be32(p, at + 8) as usize);
            assert_eq!(offset % 4, 0, "tag {i} is aligned");
            assert!(offset + size <= p.len(), "tag {i} fits");
        }
        // The colorants add up to the D50 white, as a matrix/TRC profile's must.
        let colorant = |sig: &[u8]| {
            let i = (0..count)
                .find(|i| &p[132 + 12 * i..136 + 12 * i] == sig)
                .unwrap();
            let offset = be32(p, 136 + 12 * i) as usize;
            [0, 1, 2].map(|k| be32(p, offset + 8 + 4 * k) as i32 as f64 / 65536.0)
        };
        let sum: Vec<f64> = (0..3)
            .map(|k| colorant(b"rXYZ")[k] + colorant(b"gXYZ")[k] + colorant(b"bXYZ")[k])
            .collect();
        for (got, want) in sum.iter().zip([0.9642, 1.0, 0.8249]) {
            assert!((got - want).abs() < 2e-4, "{sum:?}");
        }
    }
}

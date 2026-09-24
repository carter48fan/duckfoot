//! Non-destructive sidecar file persistence for Barrel 1.
//!
//! When an image is edited, Duckfoot stores its adjustment parameters in a companion
//! sidecar file (`<filename>.duckfoot` or `<stem>.duckfoot`) right beside the source RAW.
//! The original RAW is never modified (DESIGN.md invariant 5).

use crate::params::PhotoStack;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SIDECAR_EXT: &str = "duckfoot";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Flag {
    #[default]
    Unflagged,
    Pick,
    Reject,
}

/// Asset metadata stored in the sidecar (ratings, flags, color tags).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AssetMetadata {
    /// Rating from 0 to 5 stars (0 = unrated).
    #[serde(default)]
    pub rating: u8,
    /// Culling flag status (Unflagged, Pick, Reject).
    #[serde(default)]
    pub flag: Flag,
    /// Color label (e.g. "red", "yellow", "green", "blue", "purple").
    #[serde(default)]
    pub color_label: Option<String>,
}

/// Complete sidecar container containing both edit parameters and asset metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarData {
    pub stack: PhotoStack,
    #[serde(default)]
    pub metadata: AssetMetadata,
}

/// Returns the primary sidecar path for a RAW file: `<raw_path>.duckfoot`.
/// E.g. `DSC08450.ARW` -> `DSC08450.ARW.duckfoot`.
pub fn sidecar_path(raw_path: &Path) -> PathBuf {
    let mut p = raw_path.as_os_str().to_os_string();
    p.push(".");
    p.push(SIDECAR_EXT);
    PathBuf::from(p)
}

/// Returns the legacy/stem sidecar path: `<stem>.duckfoot`.
/// E.g. `DSC08450.ARW` -> `DSC08450.duckfoot`.
pub fn sidecar_stem_path(raw_path: &Path) -> PathBuf {
    raw_path.with_extension(SIDECAR_EXT)
}

/// Locates an existing sidecar file beside the RAW file, checking the primary
/// (`<raw_path>.duckfoot`) first, then falling back to `<stem>.duckfoot`.
pub fn find_sidecar(raw_path: &Path) -> Option<PathBuf> {
    let primary = sidecar_path(raw_path);
    if primary.is_file() {
        return Some(primary);
    }
    let stem = sidecar_stem_path(raw_path);
    if stem.is_file() {
        return Some(stem);
    }
    None
}

/// Serializes and writes a `PhotoStack` and optional `AssetMetadata` to the primary sidecar file.
pub fn save_sidecar_full(
    raw_path: &Path,
    stack: &PhotoStack,
    metadata: &AssetMetadata,
) -> Result<PathBuf> {
    let target = sidecar_path(raw_path);
    let data = SidecarData {
        stack: stack.clone(),
        metadata: metadata.clone(),
    };
    let json =
        serde_json::to_string_pretty(&data).context("failed to serialize sidecar data to JSON")?;

    // Write via temp file beside the target to ensure atomic update.
    let tmp = target.with_extension("tmp");
    fs::write(&tmp, json)
        .with_context(|| format!("failed to write temporary sidecar {}", tmp.display()))?;
    fs::rename(&tmp, &target)
        .with_context(|| format!("failed to rename sidecar to {}", target.display()))?;
    Ok(target)
}

/// Serializes and writes a `PhotoStack` to the primary sidecar file (preserving existing metadata if present).
pub fn save_sidecar(raw_path: &Path, stack: &PhotoStack) -> Result<PathBuf> {
    let existing_metadata = load_sidecar_full(raw_path)
        .ok()
        .flatten()
        .map(|d| d.metadata)
        .unwrap_or_default();
    save_sidecar_full(raw_path, stack, &existing_metadata)
}

/// Reads full `SidecarData` (stack + metadata) from an existing sidecar if present.
pub fn load_sidecar_full(raw_path: &Path) -> Result<Option<SidecarData>> {
    let Some(path) = find_sidecar(raw_path) else {
        return Ok(None);
    };
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read sidecar file {}", path.display()))?;

    // First attempt decoding as modern SidecarData container:
    if let Ok(data) = serde_json::from_str::<SidecarData>(&content) {
        return Ok(Some(data));
    }

    // Backward compatibility: attempt decoding as raw legacy PhotoStack:
    let stack: PhotoStack = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse sidecar file {}", path.display()))?;
    Ok(Some(SidecarData {
        stack,
        metadata: AssetMetadata::default(),
    }))
}

/// Reads a `PhotoStack` from an existing sidecar if present.
/// Returns `Ok(None)` if no sidecar file exists.
pub fn load_sidecar(raw_path: &Path) -> Result<Option<PhotoStack>> {
    Ok(load_sidecar_full(raw_path)?.map(|d| d.stack))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::CurveNode;
    use crate::params::{Crop, Geometry, Module, WhiteBalance};

    #[test]
    fn sidecar_roundtrips_through_json() {
        let mut stack = PhotoStack {
            geometry: Geometry {
                quarter_turns: 1,
                mirror_h: true,
                mirror_v: false,
                straighten_deg: 2.5,
                crop: Crop {
                    x: 0.1,
                    y: 0.15,
                    width: 0.7,
                    height: 0.8,
                },
            },
            white_balance: Module::new(
                true,
                WhiteBalance {
                    tint_r: 1.45,
                    tint_b: 0.82,
                },
            ),
            ..Default::default()
        };
        stack.tone_curve.params.push(CurveNode::new(0.5, 0.65));

        let json = serde_json::to_string_pretty(&stack).expect("serialization failed");
        let decoded: PhotoStack = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(stack, decoded);
    }

    #[test]
    fn sidecar_file_save_and_load() {
        let dir = std::env::temp_dir().join(format!("duckfoot_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let raw_file = dir.join("TEST0001.ARW");
        let _ = fs::write(&raw_file, b"fake-raw");

        assert!(find_sidecar(&raw_file).is_none());
        assert_eq!(load_sidecar(&raw_file).unwrap(), None);

        let mut stack = PhotoStack::default();
        stack.exposure.params.ev = 1.25;
        let saved_path = save_sidecar(&raw_file, &stack).expect("save failed");
        assert!(saved_path.is_file());
        assert_eq!(find_sidecar(&raw_file), Some(saved_path.clone()));

        let loaded = load_sidecar(&raw_file)
            .expect("load failed")
            .expect("missing stack");
        assert_eq!(loaded.exposure.params.ev, 1.25);

        let _ = fs::remove_file(&saved_path);
        let _ = fs::remove_file(&raw_file);
        let _ = fs::remove_dir(&dir);
    }
}

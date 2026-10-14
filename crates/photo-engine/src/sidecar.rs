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
    /// Words to find the photo by, in the order given, none twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
}

/// The longest a keyword may be, in characters.
pub const KEYWORD_MAX: usize = 64;

impl AssetMetadata {
    /// Add the keywords in `text`, split at commas and semicolons: trimmed, cut to
    /// [`KEYWORD_MAX`], and skipped when already there in any case. True if any were new.
    pub fn add_keywords(&mut self, text: &str) -> bool {
        let before = self.keywords.len();
        for word in text.split([',', ';']) {
            let word: String = word.trim().chars().take(KEYWORD_MAX).collect();
            let word = word.trim().to_string();
            if !word.is_empty() && !self.has_keyword(&word) {
                self.keywords.push(word);
            }
        }
        self.keywords.len() > before
    }

    pub fn has_keyword(&self, word: &str) -> bool {
        let word = word.to_lowercase();
        self.keywords.iter().any(|k| k.to_lowercase() == word)
    }

    pub fn remove_keyword(&mut self, word: &str) {
        self.keywords.retain(|k| k != word);
    }
}

/// The sidecar format this build writes. A file from a newer build is read for display
/// but never written back: its fields beyond this version's would be lost in the save.
pub const SIDECAR_VERSION: u32 = 2;

fn legacy_version() -> u32 {
    1
}

/// A named state of the stack the user can return to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    pub stack: PhotoStack,
}

/// Everything Duckfoot keeps about one photo, beside it on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarData {
    #[serde(default = "legacy_version")]
    pub version: u32,
    /// `None` when only the rating or flag has been set. Writing a default stack instead
    /// would record "no rotation" for a photo whose camera orientation was never read,
    /// and a portrait photo would open sideways from then on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<PhotoStack>,
    #[serde(default)]
    pub metadata: AssetMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<Snapshot>,
    /// The undo steps behind `stack`, oldest first, so undo reaches back past a relaunch
    /// (W-06). A recent few, not all: see `HISTORY_BUDGET`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<PhotoStack>,
    /// Fields this build does not know, kept so a save does not drop them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for SidecarData {
    fn default() -> Self {
        Self {
            version: SIDECAR_VERSION,
            stack: None,
            metadata: AssetMetadata::default(),
            snapshots: Vec::new(),
            history: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

impl SidecarData {
    /// Written by a newer Duckfoot: safe to show, not safe to overwrite.
    pub fn is_from_newer_version(&self) -> bool {
        self.version > SIDECAR_VERSION
    }
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

/// Locates an existing sidecar file beside the photo, checking the primary
/// (`<raw_path>.duckfoot`) first, then the legacy `<stem>.duckfoot`.
///
/// The legacy name is only ever a RAW's: a camera writing RAW+JPEG gives both files the
/// same stem, and the JPEG must not pick up the RAW's edits.
pub fn find_sidecar(raw_path: &Path) -> Option<PathBuf> {
    let primary = sidecar_path(raw_path);
    if primary.is_file() {
        return Some(primary);
    }
    if crate::is_raster_path(raw_path) {
        return None;
    }
    let stem = sidecar_stem_path(raw_path);
    stem.is_file().then_some(stem)
}

/// Reads the sidecar beside `raw_path`: `Ok(None)` if there is none, `Err` if there is
/// one that cannot be read. The difference matters — a file that fails to parse holds
/// the user's edits in a form this build does not understand, and must not be treated
/// as absent and then overwritten.
pub fn load_sidecar_full(raw_path: &Path) -> Result<Option<SidecarData>> {
    let Some(path) = find_sidecar(raw_path) else {
        return Ok(None);
    };
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read sidecar file {}", path.display()))?;
    parse_sidecar(&content)
        .map(Some)
        .with_context(|| format!("failed to parse sidecar file {}", path.display()))
}

pub(crate) fn parse_sidecar(content: &str) -> Result<SidecarData> {
    let mut value: serde_json::Value = serde_json::from_str(content)?;
    clamp_numbers(&mut value);
    let is_container = value.as_object().is_some_and(|o| {
        o.contains_key("stack") || o.contains_key("metadata") || o.contains_key("version")
    });
    let mut data = if is_container {
        serde_json::from_value::<SidecarData>(value)?
    } else {
        // The first format: a bare stack.
        SidecarData {
            version: 1,
            stack: Some(serde_json::from_value::<PhotoStack>(value)?),
            ..Default::default()
        }
    };
    if let Some(stack) = &mut data.stack {
        *stack = stack.sanitised();
    }
    for snapshot in &mut data.snapshots {
        snapshot.stack = snapshot.stack.sanitised();
    }
    for stack in &mut data.history {
        *stack = stack.sanitised();
    }
    // A hand-edited file's keywords, held to what the app itself would keep.
    let keywords = std::mem::take(&mut data.metadata.keywords);
    data.metadata.add_keywords(&keywords.join(","));
    data.metadata.rating = data.metadata.rating.min(5);
    Ok(data)
}

/// JSON cannot hold NaN or infinity, but a number too large for an `f32` becomes infinity
/// on the way in, and from there NaN in the shader. No parameter is anywhere near this.
fn clamp_numbers(value: &mut serde_json::Value) {
    const LIMIT: f64 = 1.0e6;
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.abs() > LIMIT && !n.is_u64() && !n.is_i64() {
                    if let Some(c) = serde_json::Number::from_f64(f.clamp(-LIMIT, LIMIT)) {
                        *n = c;
                    }
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(clamp_numbers),
        serde_json::Value::Object(map) => map.values_mut().for_each(clamp_numbers),
        _ => {}
    }
}

/// Write `data` beside `raw_path`, atomically, stamped with this build's version.
pub fn write_sidecar(raw_path: &Path, data: &SidecarData) -> Result<PathBuf> {
    let target = sidecar_path(raw_path);
    let data = SidecarData {
        version: SIDECAR_VERSION,
        ..data.clone()
    };
    let json =
        serde_json::to_string_pretty(&data).context("failed to serialize sidecar data to JSON")?;
    crate::fsio::atomic_write_bytes(&target, json.as_bytes())
        .with_context(|| format!("failed to write sidecar {}", target.display()))?;
    Ok(target)
}

/// Read, change and write back the sidecar, keeping whatever `change` does not touch —
/// snapshots, fields from other versions. Refuses a sidecar it cannot parse, or one from
/// a newer build, rather than replace it.
pub fn update_sidecar(raw_path: &Path, change: impl FnOnce(&mut SidecarData)) -> Result<PathBuf> {
    let mut data = load_sidecar_full(raw_path)?.unwrap_or_default();
    if data.is_from_newer_version() {
        anyhow::bail!(
            "{} was written by a newer Duckfoot (format {}); leaving it untouched",
            sidecar_path(raw_path).display(),
            data.version
        );
    }
    change(&mut data);
    write_sidecar(raw_path, &data)
}

/// Save a stack (or, with `None`, only metadata) and metadata together.
pub fn save_sidecar_full(
    raw_path: &Path,
    stack: Option<&PhotoStack>,
    metadata: &AssetMetadata,
) -> Result<PathBuf> {
    update_sidecar(raw_path, |data| {
        data.stack = stack.cloned();
        data.metadata = metadata.clone();
    })
}

/// The most undo steps a sidecar keeps.
pub const HISTORY_STEPS: usize = 50;
/// And the most bytes they take: brush strokes make a stack large, and a sidecar is read on
/// every open.
pub const HISTORY_BUDGET: usize = 256 * 1024;

/// The newest of `past` (oldest first) that fit [`HISTORY_STEPS`] and [`HISTORY_BUDGET`],
/// still oldest first.
pub fn recent_history<'a, I>(past: I) -> Vec<PhotoStack>
where
    I: IntoIterator<Item = &'a PhotoStack>,
    I::IntoIter: DoubleEndedIterator,
{
    let mut kept = Vec::new();
    let mut bytes = 0;
    for stack in past.into_iter().rev().take(HISTORY_STEPS) {
        bytes += serde_json::to_string(stack).map_or(usize::MAX, |s| s.len());
        if bytes > HISTORY_BUDGET {
            break;
        }
        kept.push(stack.clone());
    }
    kept.reverse();
    kept
}

/// [`save_sidecar_full`], with the undo steps behind the stack.
pub fn save_sidecar_with_history<'a, I>(
    raw_path: &Path,
    stack: &PhotoStack,
    metadata: &AssetMetadata,
    past: I,
) -> Result<PathBuf>
where
    I: IntoIterator<Item = &'a PhotoStack>,
    I::IntoIter: DoubleEndedIterator,
{
    let history = recent_history(past);
    update_sidecar(raw_path, |data| {
        data.stack = Some(stack.clone());
        data.metadata = metadata.clone();
        data.history = history;
    })
}

/// Save a stack, keeping the metadata already in the sidecar.
pub fn save_sidecar(raw_path: &Path, stack: &PhotoStack) -> Result<PathBuf> {
    update_sidecar(raw_path, |data| data.stack = Some(stack.clone()))
}

/// Reads the stack from an existing sidecar. `Ok(None)` if there is no sidecar or it
/// holds only metadata.
pub fn load_sidecar(raw_path: &Path) -> Result<Option<PhotoStack>> {
    Ok(load_sidecar_full(raw_path)?.and_then(|d| d.stack))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::CurveNode;
    use crate::params::{Crop, Geometry, Module, WhiteBalance};

    #[test]
    fn a_mangled_sidecar_is_refused_not_crashed_on() {
        let mut stack = PhotoStack::default();
        stack.local.enabled = true;
        stack
            .local
            .params
            .masks
            .push(crate::local::Mask::radial([0.5, 0.5], 0.2));
        let good = serde_json::to_vec(&SidecarData {
            stack: Some(stack.clone()),
            history: vec![stack],
            ..SidecarData::default()
        })
        .unwrap();
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..2000 {
            let mut b = good.clone();
            for _ in 0..1 + next() % 4 {
                let at = (next() % b.len() as u64) as usize;
                b[at] = b"0123456789-.e{}[]\",:tfn\\"[(next() % 20) as usize];
            }
            if let Ok(text) = String::from_utf8(b) {
                if let Ok(data) = parse_sidecar(&text) {
                    // Whatever parsed is within range.
                    let s = data.stack.unwrap_or_default();
                    assert_eq!(s, s.sanitised());
                }
            }
        }
    }

    #[test]
    fn keywords_are_trimmed_split_and_kept_once() {
        let mut m = AssetMetadata::default();
        assert!(m.add_keywords(" Iceland, waterfall;; iceland ,"));
        assert_eq!(m.keywords, ["Iceland", "waterfall"]);
        assert!(!m.add_keywords("WATERFALL"));
        m.remove_keyword("Iceland");
        assert_eq!(m.keywords, ["waterfall"]);
        assert!(m.add_keywords(&"x".repeat(100)));
        assert_eq!(m.keywords[1].len(), KEYWORD_MAX);
    }

    #[test]
    fn history_keeps_the_newest_steps_within_budget() {
        let past: Vec<PhotoStack> = (0..80)
            .map(|i| {
                let mut s = PhotoStack::default();
                s.exposure.params.ev = i as f32 / 100.0;
                s
            })
            .collect();
        let kept = recent_history(&past);
        assert_eq!(kept.len(), HISTORY_STEPS);
        assert_eq!(kept.first(), past.get(80 - HISTORY_STEPS));
        assert_eq!(kept.last(), past.last());
        // One step bigger than the budget keeps nothing rather than a gap.
        let json = serde_json::to_string(&past[0]).unwrap().len();
        let many = vec![past[0].clone(); HISTORY_BUDGET / json + 5];
        assert!(recent_history(&many).len() <= HISTORY_BUDGET / json);
    }

    #[test]
    fn history_round_trips_through_a_sidecar() {
        let dir = std::env::temp_dir().join(format!("df-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("a.dng");
        let mut before = PhotoStack::default();
        before.exposure.params.ev = 0.25;
        let now = PhotoStack::default();
        save_sidecar_with_history(&raw, &now, &AssetMetadata::default(), &[before.clone()])
            .unwrap();
        let data = load_sidecar_full(&raw).unwrap().unwrap();
        assert_eq!(data.history, vec![before.sanitised()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
                perspective_v: 12.0,
                perspective_h: 0.0,
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

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("duckfoot-sidecar-{name}-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn a_rating_alone_writes_no_stack() {
        let dir = scratch("meta");
        let raw = dir.join("P1.ARW");
        let metadata = AssetMetadata {
            rating: 3,
            ..Default::default()
        };
        save_sidecar_full(&raw, None, &metadata).unwrap();
        let data = load_sidecar_full(&raw).unwrap().unwrap();
        assert_eq!(
            data.stack, None,
            "no stack means the camera orientation still rules"
        );
        assert_eq!(data.metadata.rating, 3);
        assert_eq!(data.version, SIDECAR_VERSION);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_does_not_parse_is_never_overwritten() {
        let dir = scratch("corrupt");
        let raw = dir.join("P2.ARW");
        fs::write(sidecar_path(&raw), b"{ \"stack\": { not json").unwrap();
        assert!(load_sidecar_full(&raw).is_err());
        assert!(save_sidecar(&raw, &PhotoStack::default()).is_err());
        assert_eq!(
            fs::read(sidecar_path(&raw)).unwrap(),
            b"{ \"stack\": { not json"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_sidecar_is_read_but_not_written() {
        let dir = scratch("newer");
        let raw = dir.join("P3.ARW");
        let json = r#"{"version": 99, "metadata": {"rating": 4}, "masks": [1, 2]}"#;
        fs::write(sidecar_path(&raw), json).unwrap();
        let data = load_sidecar_full(&raw).unwrap().unwrap();
        assert!(data.is_from_newer_version());
        assert_eq!(data.metadata.rating, 4);
        assert!(save_sidecar(&raw, &PhotoStack::default()).is_err());
        assert_eq!(fs::read_to_string(sidecar_path(&raw)).unwrap(), json);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fields_this_build_does_not_know_survive_a_save() {
        let dir = scratch("extra");
        let raw = dir.join("P4.ARW");
        fs::write(
            sidecar_path(&raw),
            r#"{"version": 2, "metadata": {"rating": 1}, "keywords": ["dog"]}"#,
        )
        .unwrap();
        save_sidecar(&raw, &PhotoStack::default()).unwrap();
        let data = load_sidecar_full(&raw).unwrap().unwrap();
        assert_eq!(data.extra["keywords"], serde_json::json!(["dog"]));
        assert_eq!(data.metadata.rating, 1, "saving a stack keeps the rating");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_values_are_brought_back_into_range() {
        let mut stack = serde_json::to_value(PhotoStack::default()).unwrap();
        stack["exposure"]["params"]["ev"] = serde_json::json!(1.0e39);
        stack["geometry"]["quarter_turns"] = serde_json::json!(255);
        stack["tone_curve"]["params"] = serde_json::json!([]);
        let json = serde_json::json!({ "version": 2, "stack": stack }).to_string();
        let data = parse_sidecar(&json).unwrap();
        let stack = data.stack.unwrap();
        assert!(stack.exposure.params.ev.is_finite());
        assert_eq!(stack.geometry.quarter_turns, 3);
        assert_eq!(stack.tone_curve.params.len(), 2);
    }

    #[test]
    fn the_first_format_a_bare_stack_still_loads() {
        let mut stack = PhotoStack::default();
        stack.exposure.params.ev = 0.75;
        let json = serde_json::to_string(&stack).unwrap();
        let data = parse_sidecar(&json).unwrap();
        assert_eq!(data.version, 1);
        assert_eq!(data.stack.unwrap().exposure.params.ev, 0.75);
    }

    #[test]
    fn a_jpeg_does_not_inherit_its_raw_twins_legacy_sidecar() {
        let dir = scratch("twins");
        let raw = dir.join("DSC1.ARW");
        let jpeg = dir.join("DSC1.JPG");
        fs::write(sidecar_stem_path(&raw), "{}").unwrap();
        assert!(find_sidecar(&raw).is_some());
        assert!(find_sidecar(&jpeg).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}

//! The little the app remembers between runs: whether the phone tour has been seen, and
//! which modules the phone's Edit strip shows, in what order. One JSON file in the
//! platform's settings directory; with no directory (tests, the screenshot tool) nothing
//! is remembered and nothing is written.

use std::path::{Path, PathBuf};

use crate::{InspectorSectionConfig, InspectorSectionId};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The phone tour has been finished or skipped.
    pub tour_done: bool,
    /// The phone Edit strip's modules. `None` until the user changes them.
    pub compact_sections: Option<Vec<InspectorSectionConfig>>,
}

impl Settings {
    pub fn load(dir: Option<&Path>) -> Self {
        dir.map(file)
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: Option<&Path>) {
        let Some(dir) = dir else { return };
        // Through a temporary file: a crash mid-write must not reset every setting.
        let result = std::fs::create_dir_all(dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| {
                let json = serde_json::to_vec_pretty(self)?;
                photo_engine::fsio::atomic_write_bytes(&file(dir), &json)
            });
        if let Err(e) = result {
            log::warn!("could not save settings in {}: {e:#}", dir.display());
        }
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// The phone's modules: every module, in the desktop's order, with the scope and filmic
/// off. Both are 3D views at heart, which a thumb on a small screen cannot orbit; they
/// stay a switch away in the modules list, drawn flat when turned on.
pub fn default_compact_sections() -> Vec<InspectorSectionConfig> {
    let mut sections = InspectorSectionConfig::default_sections();
    for s in &mut sections {
        s.visible = !matches!(
            s.id,
            InspectorSectionId::Scope | InspectorSectionId::Filmic3D
        );
    }
    sections
}

/// `saved`, with any module it does not know about (one added since it was saved)
/// appended in its default state, and any it names twice or that no longer exists
/// dropped.
pub fn merge_sections(saved: Vec<InspectorSectionConfig>) -> Vec<InspectorSectionConfig> {
    let mut out: Vec<InspectorSectionConfig> = Vec::new();
    for s in saved {
        if !out.iter().any(|o| o.id == s.id) {
            out.push(s);
        }
    }
    for d in default_compact_sections() {
        if !out.iter().any(|o| o.id == d.id) {
            out.push(d);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_fill_in_new_modules() {
        let dir = std::env::temp_dir().join(format!("duckfoot-settings-{}", std::process::id()));
        let mut saved = default_compact_sections();
        saved.retain(|s| s.id != InspectorSectionId::Vignette);
        saved.swap(1, 2);
        Settings {
            tour_done: true,
            compact_sections: Some(saved.clone()),
        }
        .save(Some(&dir));
        let loaded = Settings::load(Some(&dir));
        assert!(loaded.tour_done);
        let merged = merge_sections(loaded.compact_sections.unwrap());
        assert_eq!(merged[1].id, saved[1].id);
        assert_eq!(merged.last().unwrap().id, InspectorSectionId::Vignette);
        assert_eq!(merged.len(), InspectorSectionId::all().len());
        assert!(!Settings::load(None).tour_done);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

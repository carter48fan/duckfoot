//! What the app remembers between runs: the shelf and the photo left open, how the
//! inspector and the phone's Edit strip are arranged, the text size, the export recipes
//! and the rename pattern, and whether the phone tour has been seen. One JSON file in the
//! platform's settings directory; with no directory (tests, the screenshot tool) nothing
//! is remembered and nothing is written.

use std::path::{Path, PathBuf};

use crate::{InspectorSectionConfig, InspectorSectionId};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The phone tour has been finished or skipped.
    pub tour_done: bool,
    /// The phone Edit strip's modules. `None` until the user changes them.
    pub compact_sections: Option<Vec<InspectorSectionConfig>>,
    /// The desktop inspector's modules, in order, and which are shown.
    pub inspector_sections: Option<Vec<InspectorSectionConfig>>,
    pub single_open: Option<bool>,
    pub inspector_width: Option<f32>,
    pub shelf_open: Option<bool>,
    /// Size of the whole interface, 0.85..=1.5: text, controls, spacing.
    pub ui_scale: f32,
    /// The shelf as it was left, and the photo that was open.
    pub shelf: Vec<PathBuf>,
    pub last_open: Option<PathBuf>,
    pub export_recipes: Option<Vec<crate::batch::ExportRecipe>>,
    pub rename_pattern: Option<String>,
    /// Where the photo pickers start: the folder the last photos came from.
    pub import_dir: Option<PathBuf>,
    /// Where, and as what, the last single export was written.
    pub export_dir: Option<PathBuf>,
    /// The single export's settings, as last used.
    pub export: Option<photo_engine::ExportSettings>,
    pub batch_output_dir: Option<PathBuf>,
    /// The shelf's filter and sort (the search is not kept).
    pub filter: Option<crate::dam::FilterState>,
    /// Keep a straightened photo's crop on photographed pixels (no empty corners).
    pub constrain_crop: bool,
    /// What paste carries (W-03).
    pub paste: Option<crate::paste::PasteSet>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tour_done: false,
            compact_sections: None,
            inspector_sections: None,
            single_open: None,
            inspector_width: None,
            shelf_open: None,
            ui_scale: 1.0,
            shelf: Vec::new(),
            last_open: None,
            export_recipes: None,
            rename_pattern: None,
            import_dir: None,
            export_dir: None,
            export: None,
            batch_output_dir: None,
            filter: None,
            constrain_crop: true,
            paste: None,
        }
    }
}

/// The interface sizes the text-size setting offers.
pub const UI_SCALES: [(f32, &str); 5] = [
    (0.85, "S"),
    (1.0, "M"),
    (1.15, "L"),
    (1.3, "XL"),
    (1.5, "XXL"),
];

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
/// placed in its default state after the module it follows by default, and any it names twice or that no longer exists
/// dropped.
pub fn merge_sections(saved: Vec<InspectorSectionConfig>) -> Vec<InspectorSectionConfig> {
    merge_sections_with(saved, default_compact_sections())
}

/// `saved`, with anything it does not know about — modules added since it was written —
/// taken from `defaults` and put after the module it follows there (lens corrections
/// belong beside crop, not under vignette), and duplicates dropped.
pub fn merge_sections_with(
    saved: Vec<InspectorSectionConfig>,
    defaults: Vec<InspectorSectionConfig>,
) -> Vec<InspectorSectionConfig> {
    let mut out: Vec<InspectorSectionConfig> = Vec::new();
    for s in saved {
        if !out.iter().any(|o| o.id == s.id) {
            out.push(s);
        }
    }
    for (i, d) in defaults.iter().enumerate() {
        if out.iter().any(|o| o.id == d.id) {
            continue;
        }
        let at = defaults[..i]
            .iter()
            .rev()
            .find_map(|before| out.iter().position(|o| o.id == before.id))
            .map_or(0, |p| p + 1);
        out.insert(at, d.clone());
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
            ui_scale: 1.3,
            shelf: vec![PathBuf::from("/photos/a.ARW")],
            ..Default::default()
        }
        .save(Some(&dir));
        let loaded = Settings::load(Some(&dir));
        assert!(loaded.tour_done);
        assert_eq!(loaded.ui_scale, 1.3);
        assert_eq!(loaded.shelf, vec![PathBuf::from("/photos/a.ARW")]);
        let merged = merge_sections(loaded.compact_sections.unwrap());
        assert_eq!(merged[1].id, saved[1].id);
        let vignette = merged
            .iter()
            .position(|s| s.id == InspectorSectionId::Vignette)
            .unwrap();
        assert_eq!(merged[vignette - 1].id, InspectorSectionId::Sharpen);
        assert_eq!(merged.len(), InspectorSectionId::all().len());
        // A module new since the save lands where it belongs, not at the bottom.
        let mut older = default_compact_sections();
        older.retain(|s| s.id != InspectorSectionId::Lens);
        let merged = merge_sections(older);
        let lens = merged
            .iter()
            .position(|s| s.id == InspectorSectionId::Lens)
            .unwrap();
        assert_eq!(merged[lens - 1].id, InspectorSectionId::Geometry);
        assert!(!Settings::load(None).tour_done);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_from_an_older_build_load_with_defaults_for_the_rest() {
        let old: Settings = serde_json::from_str(r#"{"tour_done": true}"#).unwrap();
        assert!(old.tour_done);
        assert_eq!(old.ui_scale, 1.0);
        assert!(old.shelf.is_empty());
    }
}

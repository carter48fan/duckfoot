//! User presets (W-04): a look saved to reuse — the stack it was saved from, and which of
//! its modules it carries (a [`PasteSet`], chosen as for paste). Kept one file each in
//! the settings folder's `presets/`, as `.duckfoot-preset` JSON that can be exported and
//! imported between machines. Anything read from a file goes through the stack's own
//! sanitising, as a sidecar does.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use photo_engine::PhotoStack;
use serde::{Deserialize, Serialize};

use crate::paste::PasteSet;

pub const EXTENSION: &str = "duckfoot-preset";
/// The newest file version this build writes and reads.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreset {
    pub version: u32,
    pub name: String,
    /// Which modules applying it sets; the rest of the photo is left alone.
    #[serde(default)]
    pub carries: PasteSet,
    pub stack: PhotoStack,
}

impl UserPreset {
    pub fn new(name: &str, carries: PasteSet, stack: &PhotoStack) -> Self {
        Self {
            version: VERSION,
            name: name.trim().to_string(),
            carries,
            stack: stack.clone(),
        }
    }

    /// A preset from a file's bytes: refused if it is from a newer version or not one at
    /// all, and made safe otherwise.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let mut p: UserPreset = serde_json::from_slice(bytes).context("not a Duckfoot preset")?;
        if p.version > VERSION {
            bail!(
                "made by a newer Duckfoot (preset version {}, this reads {VERSION})",
                p.version
            );
        }
        p.name = p.name.trim().chars().take(80).collect();
        if p.name.is_empty() {
            p.name = "Preset".into();
        }
        p.stack = p.stack.sanitised();
        p.version = VERSION;
        Ok(p)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// `target` with this preset's modules.
    pub fn apply(&self, target: &PhotoStack) -> PhotoStack {
        self.carries.apply(&self.stack, target)
    }

    /// A file name for it: its name, with anything a file system might object to
    /// replaced.
    pub fn file_name(&self) -> String {
        let safe: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}.{EXTENSION}", safe.trim())
    }
}

/// Where the presets live, under the settings folder.
pub fn folder(settings_dir: &Path) -> PathBuf {
    settings_dir.join("presets")
}

/// Every preset in `dir`, by name. Files that cannot be read are skipped and logged.
pub fn load_all(dir: &Path) -> Vec<(PathBuf, UserPreset)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, UserPreset)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == EXTENSION))
        .filter_map(|path| {
            let read = std::fs::read(&path)
                .map_err(anyhow::Error::from)
                .and_then(|b| UserPreset::from_json(&b));
            match read {
                Ok(p) => Some((path, p)),
                Err(e) => {
                    log::warn!("{}: {e:#}", path.display());
                    None
                }
            }
        })
        .collect();
    out.sort_by_key(|(_, p)| p.name.to_lowercase());
    out
}

/// Write `preset` into `dir`, replacing one of the same name.
pub fn save(dir: &Path, preset: &UserPreset) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(preset.file_name());
    photo_engine::fsio::atomic_write_bytes(&path, preset.to_json().as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

impl crate::PhotoApp {
    fn presets_dir(&self) -> Option<PathBuf> {
        self.settings_dir.as_deref().map(folder)
    }

    pub(crate) fn reload_user_presets(&mut self) {
        self.user_presets = self.presets_dir().map(|d| load_all(&d)).unwrap_or_default();
    }

    /// Save the open photo's look, as far as the paste set carries it, under `name`.
    pub fn save_user_preset(&mut self, name: &str) {
        let Some(dir) = self.presets_dir() else {
            self.set_status("Presets need a settings folder, which this device has not given");
            return;
        };
        if name.trim().is_empty() {
            return;
        }
        let preset = UserPreset::new(name, self.paste_set.clone(), &self.stack);
        match save(&dir, &preset) {
            Ok(_) => {
                self.reload_user_presets();
                self.set_status(format!("Saved preset {}", preset.name));
            }
            Err(e) => self.error = Some(format!("Could not save the preset: {e:#}")),
        }
    }

    /// Apply user preset `i` to the open photo, as one undo step.
    pub fn apply_user_preset(&mut self, i: usize) {
        let Some((_, preset)) = self.user_presets.get(i) else {
            return;
        };
        if !self.can_edit() {
            return;
        }
        let applied = preset.apply(&self.stack);
        let name = preset.name.clone();
        if applied != self.stack {
            self.history.checkpoint(&self.stack);
            self.stack = applied;
            self.needs_render = true;
        }
        self.set_status(format!("Applied {name}"));
    }

    pub(crate) fn delete_user_preset(&mut self, i: usize) {
        if let Some((path, preset)) = self.user_presets.get(i).cloned() {
            match std::fs::remove_file(&path) {
                Ok(()) => self.set_status(format!("Deleted preset {}", preset.name)),
                Err(e) => self.error = Some(format!("{}: {e}", path.display())),
            }
            self.reload_user_presets();
        }
    }

    pub(crate) fn export_user_preset(&mut self, ctx: &egui::Context, i: usize) {
        if let Some((_, preset)) = self.user_presets.get(i).cloned() {
            let name = preset.file_name();
            self.exporting_preset = Some(preset);
            self.request(ctx, crate::platform::Request::ExportPreset { name });
        }
    }

    /// Add a preset file to the user's presets, under its own name.
    pub fn import_preset(&mut self, path: &Path) {
        let read = std::fs::read(path)
            .map_err(anyhow::Error::from)
            .and_then(|b| UserPreset::from_json(&b));
        let (Some(dir), Ok(preset)) = (self.presets_dir(), read.as_ref()) else {
            if let Err(e) = read {
                self.error = Some(format!("{}: {e:#}", path.display()));
            }
            return;
        };
        match save(&dir, preset) {
            Ok(_) => {
                self.reload_user_presets();
                self.set_status(format!("Imported preset {}", preset.name));
            }
            Err(e) => self.error = Some(format!("Could not import the preset: {e:#}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InspectorSectionId;

    /// Writes the preset fuzz target's seed (see photo-engine's `write_fuzz_seeds`).
    #[test]
    #[ignore]
    fn write_fuzz_seeds() {
        let Some(root) = std::env::var_os("DUCKFOOT_FUZZ_SEEDS").map(std::path::PathBuf::from)
        else {
            return;
        };
        let dir = root.join("preset");
        std::fs::create_dir_all(&dir).unwrap();
        let preset = UserPreset::new("Evening", PasteSet::default(), &look());
        std::fs::write(
            dir.join("evening.duckfoot-preset"),
            serde_json::to_vec_pretty(&preset).unwrap(),
        )
        .unwrap();
    }

    fn look() -> PhotoStack {
        let mut s = PhotoStack::default();
        s.vibrance.enabled = true;
        s.vibrance.params.vibrance = 0.4;
        s.exposure.params.ev = 1.5;
        s
    }

    #[test]
    fn a_preset_round_trips_and_applies_only_what_it_carries() {
        let mut carries = PasteSet::default();
        carries.set(InspectorSectionId::Exposure, false);
        let p = UserPreset::new("  Punchy  ", carries, &look());
        assert_eq!(p.name, "Punchy");
        let back = UserPreset::from_json(p.to_json().as_bytes()).unwrap();
        assert_eq!(back, p);
        let out = back.apply(&PhotoStack::default());
        assert!(out.vibrance.enabled);
        assert_eq!(out.exposure.params.ev, 0.0, "exposure was not carried");
    }

    #[test]
    fn a_bad_or_newer_file_is_refused_and_a_hostile_one_made_safe() {
        assert!(UserPreset::from_json(b"{}").is_err());
        let mut p = UserPreset::new("x", PasteSet::default(), &look());
        p.version = VERSION + 1;
        let err = UserPreset::from_json(p.to_json().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("newer"), "{err}");
        let mut p = UserPreset::new("", PasteSet::default(), &look());
        p.stack.geometry.quarter_turns = 9;
        let safe = UserPreset::from_json(p.to_json().as_bytes()).unwrap();
        assert_eq!(safe.stack.geometry.quarter_turns, 1);
        assert_eq!(safe.name, "Preset");
    }

    #[test]
    fn presets_are_saved_one_file_each_and_listed_by_name() {
        let dir = std::env::temp_dir().join(format!("duckfoot-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save(
            &dir,
            &UserPreset::new("b/w: soft", PasteSet::default(), &look()),
        )
        .unwrap();
        save(
            &dir,
            &UserPreset::new("Alpha", PasteSet::default(), &look()),
        )
        .unwrap();
        // Same name again replaces it.
        save(
            &dir,
            &UserPreset::new("Alpha", PasteSet::default(), &PhotoStack::default()),
        )
        .unwrap();
        std::fs::write(dir.join("junk.duckfoot-preset"), b"nope").unwrap();
        let all = load_all(&dir);
        let names: Vec<&str> = all.iter().map(|(_, p)| p.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "b/w: soft"]);
        assert_eq!(all[0].1.stack, PhotoStack::default());
        assert!(all[1].0.ends_with("b_w_ soft.duckfoot-preset"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

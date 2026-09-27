//! Folder tree browser and directory scanning for session-first asset management.

use photo_engine::is_supported_image;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct FolderBookmark {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FolderTree {
    pub current_folder: Option<PathBuf>,
    pub bookmarks: Vec<FolderBookmark>,
}

impl Default for FolderTree {
    fn default() -> Self {
        let mut bookmarks = Vec::new();

        if let Ok(home) = std::env::var("HOME") {
            let home_path = PathBuf::from(&home);
            bookmarks.push(FolderBookmark {
                name: "Home".into(),
                path: home_path.clone(),
            });

            let pictures = home_path.join("Pictures");
            if pictures.is_dir() {
                bookmarks.push(FolderBookmark {
                    name: "Pictures".into(),
                    path: pictures,
                });
            }
        }

        Self {
            current_folder: None,
            bookmarks,
        }
    }
}

impl FolderTree {
    /// List subfolders inside the given directory for tree exploration.
    pub fn list_subfolders(dir: &Path) -> Vec<PathBuf> {
        let mut subfolders = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Ignore hidden directories like .git or .duckfoot_cache
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !name.starts_with('.') {
                            subfolders.push(path);
                        }
                    }
                }
            }
        }
        subfolders.sort();
        subfolders
    }

    /// `dir` and the folders inside it, `depth` levels down, skipping hidden and system
    /// folders (WP5.8): each folder that holds photos, with them, the top one first.
    pub fn scan_tree(dir: &Path, depth: u32) -> Vec<(PathBuf, Vec<PathBuf>)> {
        let mut out = Vec::new();
        let mut queue = vec![(dir.to_path_buf(), 0u32)];
        // A tree of thousands of folders is not a roll of photos: stop well before.
        while let Some((d, level)) = queue.pop() {
            if out.len() >= 256 {
                break;
            }
            let images = Self::scan_supported_images(&d);
            if level < depth {
                let mut subs = Self::list_subfolders(&d);
                subs.retain(|s| {
                    let name = s.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    !matches!(
                        name.to_ascii_lowercase().as_str(),
                        "system volume information"
                            | "$recycle.bin"
                            | "lost+found"
                            | "node_modules"
                    ) && !name.ends_with(".app")
                });
                // Popped from the end: pushed reversed so they come in order.
                queue.extend(subs.into_iter().rev().map(|s| (s, level + 1)));
            }
            if !images.is_empty() {
                out.push((d, images));
            }
        }
        out
    }

    /// Scan directory for all supported photo files (RAW + Raster), and the virtual
    /// copies their sidecars record, each copy after its photo.
    pub fn scan_supported_images(dir: &Path) -> Vec<PathBuf> {
        let mut images = Vec::new();
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_supported_image(&path) {
                    images.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        images.extend(
            photo_engine::copies::copies_among(&files)
                .into_iter()
                .filter(|p| is_supported_image(p)),
        );
        photo_engine::copies::sort(&mut images);
        images
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_tree_default_has_bookmarks() {
        let tree = FolderTree::default();
        assert!(!tree.bookmarks.is_empty());
    }
}

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

        // HOME, or on Windows USERPROFILE (HOME is usually unset there).
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));
        if let Ok(home) = home {
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

/// A folder the shelf's photos are in: its depth below the shelf's common folder, the
/// name to show for it, and how many photos are in it and the folders inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShelfFolder {
    pub path: PathBuf,
    pub depth: usize,
    pub name: String,
    pub photos: usize,
}

impl FolderTree {
    /// The folders `photos` are in, and every folder between them and the one they all
    /// share, as a tree in path order. Read from the paths alone: nothing touches the disk.
    pub fn from_paths<'a>(photos: impl IntoIterator<Item = &'a Path>) -> Vec<ShelfFolder> {
        use std::collections::BTreeMap;
        let mut direct: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for p in photos {
            if let Some(parent) = p.parent() {
                *direct.entry(parent.to_path_buf()).or_default() += 1;
            }
        }
        let Some(first) = direct.keys().next().cloned() else {
            return Vec::new();
        };
        // The deepest folder every one is inside.
        let mut root = first;
        for dir in direct.keys() {
            while !dir.starts_with(&root) {
                match root.parent() {
                    Some(up) => root = up.to_path_buf(),
                    None => break,
                }
            }
        }
        // Each folder from the root down to every one that holds photos, counted with
        // what is inside it.
        let mut all: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for (dir, n) in &direct {
            let mut at = Some(dir.as_path());
            while let Some(d) = at {
                if !d.starts_with(&root) {
                    break;
                }
                *all.entry(d.to_path_buf()).or_default() += n;
                at = d.parent();
            }
        }
        let root_depth = root.components().count();
        all.into_iter()
            .map(|(path, photos)| ShelfFolder {
                depth: path.components().count() - root_depth,
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string()),
                path,
                photos,
            })
            .collect()
    }

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
        Self::scan_tree_with(dir, depth, |_, _| true).unwrap_or_default()
    }

    /// [`FolderTree::scan_tree`], told after each folder how many folders it has read and
    /// photos it has found; `None` if `keep_going` said to stop.
    pub fn scan_tree_with(
        dir: &Path,
        depth: u32,
        keep_going: impl Fn(usize, usize) -> bool,
    ) -> Option<Vec<(PathBuf, Vec<PathBuf>)>> {
        let mut out = Vec::new();
        let mut queue = vec![(dir.to_path_buf(), 0u32)];
        let (mut read, mut photos) = (0, 0);
        // A tree of thousands of folders is not a roll of photos: stop well before.
        while let Some((d, level)) = queue.pop() {
            if out.len() >= 256 {
                break;
            }
            if !keep_going(read, photos) {
                return None;
            }
            let images = Self::scan_supported_images(&d);
            read += 1;
            photos += images.len();
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
        keep_going(read, photos).then_some(out)
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
    fn the_shelfs_folders_are_a_tree_counted_from_the_paths() {
        let paths = [
            "/cards/2026/june/a.ARW",
            "/cards/2026/june/b.ARW",
            "/cards/2026/july/c.ARW",
            "/cards/2026/july/raw/d.ARW",
        ]
        .map(PathBuf::from);
        let tree = FolderTree::from_paths(paths.iter().map(PathBuf::as_path));
        let rows: Vec<(String, usize, usize)> = tree
            .iter()
            .map(|f| (f.name.clone(), f.depth, f.photos))
            .collect();
        assert_eq!(
            rows,
            [
                ("2026".to_string(), 0, 4),
                ("july".to_string(), 1, 2),
                ("raw".to_string(), 2, 1),
                ("june".to_string(), 1, 2),
            ]
        );
        assert!(FolderTree::from_paths(std::iter::empty()).is_empty());
    }

    #[test]
    fn folder_tree_default_has_bookmarks() {
        let tree = FolderTree::default();
        assert!(!tree.bookmarks.is_empty());
    }
}

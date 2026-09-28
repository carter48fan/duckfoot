//! The phone's library of copies (H11): what Duckfoot copied in from the picker, what
//! each copy costs, and deleting them. A desktop opens photos where they are and has no
//! such folder.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use photo_engine::sidecar::sidecar_stem_path;
use photo_engine::{is_supported_image, sidecar_path, SIDECAR_EXT};

/// One photo in the library, with everything that goes with it.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryEntry {
    pub path: PathBuf,
    /// The photo and every sidecar beside it: its own and its virtual copies'.
    pub bytes: u64,
    /// The sidecars, deleted with the photo.
    pub sidecars: Vec<PathBuf>,
}

/// The Library screen's state while it is open.
#[derive(Debug, Default)]
pub struct StorageView {
    /// `None` while the library is being read.
    pub entries: Option<Vec<LibraryEntry>>,
    pub selected: HashSet<PathBuf>,
    /// A delete button has been pressed once and asks to be pressed again.
    pub confirming: Option<Confirm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Selected,
    Unused,
}

/// Folders read below the library. Picked folders are copied in one level deep, so this
/// is generous.
const DEPTH: u32 = 4;

/// Every photo under `dir`, largest first.
pub fn list(dir: &Path) -> Vec<LibraryEntry> {
    let mut out = Vec::new();
    let mut queue = vec![(dir.to_path_buf(), 0u32)];
    while let Some((d, level)) = queue.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if level < DEPTH {
                    queue.push((path, level + 1));
                }
            } else {
                files.push((path, meta.len()));
            }
        }
        let suffix = format!(".{SIDECAR_EXT}");
        for (photo, size) in files.iter().filter(|(p, _)| is_supported_image(p)) {
            let own = [sidecar_path(photo), sidecar_stem_path(photo)];
            let stem = photo.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let copy_prefix = format!("{stem} (v");
            let sidecars: Vec<(PathBuf, u64)> = files
                .iter()
                .filter(|(f, _)| {
                    own.contains(f)
                        || f.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                            n.ends_with(&suffix)
                                && n.starts_with(&copy_prefix)
                                && photo_engine::copies::parse(
                                    &f.with_file_name(n.strip_suffix(&suffix).unwrap_or(n)),
                                )
                                .is_some_and(|(source, _)| &source == photo)
                        })
                })
                .cloned()
                .collect();
            out.push(LibraryEntry {
                path: photo.clone(),
                bytes: size + sidecars.iter().map(|(_, s)| s).sum::<u64>(),
                sidecars: sidecars.into_iter().map(|(p, _)| p).collect(),
            });
        }
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    out
}

/// Deletes the photo and its sidecars. A sidecar already gone is not an error; the photo
/// already gone is not either, since nothing is left to delete.
pub fn delete(entry: &LibraryEntry) -> io::Result<()> {
    let gone = |r: io::Result<()>| match r {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    gone(fs::remove_file(&entry.path))?;
    for sidecar in &entry.sidecars {
        gone(fs::remove_file(sidecar))?;
    }
    Ok(())
}

/// Whether `path`, or the photo it is a virtual copy of, lies in `library`.
pub fn is_in(library: &Path, path: &Path) -> bool {
    photo_engine::copies::source_file(path).starts_with(library)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("duckfoot-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_photo_is_counted_with_its_sidecars_and_deleted_with_them() {
        let dir = scratch("list");
        let sub = dir.join("Camera");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("a.jpg"), vec![0u8; 100]).unwrap();
        fs::write(dir.join("a.jpg.duckfoot"), vec![0u8; 10]).unwrap();
        fs::write(dir.join("a (v2).jpg.duckfoot"), vec![0u8; 5]).unwrap();
        // Another photo's copy, and a stray file: neither is a.jpg's.
        fs::write(dir.join("ab (v2).jpg.duckfoot"), vec![0u8; 7]).unwrap();
        fs::write(dir.join("notes.txt"), vec![0u8; 1000]).unwrap();
        fs::write(sub.join("b.png"), vec![0u8; 300]).unwrap();

        let found = list(&dir);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].path, sub.join("b.png"));
        assert_eq!(found[0].bytes, 300);
        let a = &found[1];
        assert_eq!(a.path, dir.join("a.jpg"));
        assert_eq!(a.bytes, 115);
        assert_eq!(a.sidecars.len(), 2);

        delete(a).unwrap();
        assert!(!dir.join("a.jpg").exists());
        assert!(!dir.join("a.jpg.duckfoot").exists());
        assert!(!dir.join("a (v2).jpg.duckfoot").exists());
        assert!(dir.join("ab (v2).jpg.duckfoot").exists());
        // Deleting what is already gone succeeds.
        delete(a).unwrap();
        assert!(is_in(&dir, &dir.join("x (v3).jpg")));
        assert!(!is_in(&dir, Path::new("/elsewhere/x.jpg")));
        let _ = fs::remove_dir_all(&dir);
    }
}

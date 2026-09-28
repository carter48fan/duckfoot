//! Virtual copies (W-05): more than one edit of the same file. A copy is addressed as the
//! file with ` (vN)` before its extension (`IMG_0001 (v2).dng`), a path that is not on
//! disk but has a sidecar of its own beside the photo. Each copy so keeps its own edits,
//! marks, history and snapshots, and a folder read again finds its copies from their
//! sidecars. Whatever reads the photo's bytes asks [`source_file`] first.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The copy a path names, and the file it is a copy of, by its name alone.
pub fn parse(path: &Path) -> Option<(PathBuf, u32)> {
    let stem = path.file_stem()?.to_str()?;
    let open = stem.rfind(" (v")?;
    let n: u32 = stem[open + 3..].strip_suffix(')')?.parse().ok()?;
    if n < 2 || stem[open + 3..].starts_with('0') {
        return None;
    }
    let mut name = stem[..open].to_string();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        name.push('.');
        name.push_str(ext);
    }
    Some((path.with_file_name(name), n))
}

/// The copy's number, 1 for the photo itself.
pub fn number(path: &Path) -> u32 {
    parse(path).map_or(1, |(_, n)| n)
}

/// The file whose bytes `path` shows: the photo a copy is of, or `path` itself. A real
/// file that happens to be named like a copy is itself.
pub fn source_file(path: &Path) -> Cow<'_, Path> {
    match parse(path) {
        Some((source, _)) if !path.exists() => Cow::Owned(source),
        _ => Cow::Borrowed(path),
    }
}

/// Copy `n` of `source`.
pub fn copy_path(source: &Path, n: u32) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("photo");
    let name = match source.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem} (v{n}).{ext}"),
        None => format!("{stem} (v{n})"),
    };
    source.with_file_name(name)
}

/// The next copy of `source` (a copy's own source, if it is one) that nothing uses: not
/// a file, no sidecar, and not `taken`.
pub fn next_copy(source: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let source = source_file(source);
    (2..)
        .map(|n| copy_path(&source, n))
        .find(|p| !p.exists() && crate::find_sidecar(p).is_none() && !taken(p))
        .expect("an unbounded range always finds a free copy")
}

/// The copies a folder's files record: each sidecar named for a copy whose photo is
/// there.
pub fn copies_among(files: &[PathBuf]) -> Vec<PathBuf> {
    let suffix = format!(".{}", crate::SIDECAR_EXT);
    files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_str()?;
            let copy = f.with_file_name(name.strip_suffix(&suffix)?);
            let (source, _) = parse(&copy)?;
            (source.is_file() && !copy.exists()).then_some(copy)
        })
        .collect()
}

/// Photos in the order a roll reads them: each followed by its copies.
pub fn sort(paths: &mut [PathBuf]) {
    paths.sort_by_cached_key(|p| match parse(p) {
        Some((source, n)) => (source, n),
        None => (p.clone(), 1),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_names_its_photo() {
        let p = Path::new("/roll/IMG_0001 (v2).dng");
        assert_eq!(parse(p), Some((PathBuf::from("/roll/IMG_0001.dng"), 2)));
        assert_eq!(copy_path(Path::new("/roll/IMG_0001.dng"), 2), p);
        assert_eq!(number(Path::new("/roll/IMG_0001.dng")), 1);
        for plain in [
            "/roll/a (v1).dng",
            "/roll/a (v02).dng",
            "/roll/a (vx).dng",
            "/roll/a.dng",
        ] {
            assert_eq!(parse(Path::new(plain)), None, "{plain}");
        }
        // A missing copy reads from its photo.
        assert_eq!(source_file(p), Path::new("/roll/IMG_0001.dng"));
    }

    #[test]
    fn copies_follow_their_photo() {
        let mut paths: Vec<PathBuf> = ["b.dng", "a (v3).dng", "a.dng", "a (v2).dng", "a b.dng"]
            .iter()
            .map(PathBuf::from)
            .collect();
        sort(&mut paths);
        let names: Vec<_> = paths.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(
            names,
            ["a b.dng", "a.dng", "a (v2).dng", "a (v3).dng", "b.dng"]
        );
    }

    #[test]
    fn a_folder_finds_its_copies_from_their_sidecars() {
        let dir = std::env::temp_dir().join(format!("df-copies-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng");
        std::fs::write(&photo, b"").unwrap();
        assert_eq!(next_copy(&photo, |_| false), dir.join("a (v2).dng"));
        let copy = dir.join("a (v2).dng");
        crate::save_sidecar(&copy, &crate::PhotoStack::default()).unwrap();
        assert_eq!(next_copy(&photo, |_| false), dir.join("a (v3).dng"));
        assert_eq!(
            next_copy(&copy, |p| p.ends_with("a (v3).dng")),
            dir.join("a (v4).dng")
        );
        // An orphan (its photo gone) is not offered.
        crate::save_sidecar(&dir.join("gone (v2).dng"), &crate::PhotoStack::default()).unwrap();
        let files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(copies_among(&files), vec![copy]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

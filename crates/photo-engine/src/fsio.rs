//! Writing files so that a crash, a full disk or a pulled card never leaves half of one.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

/// Write `path` by way of a temporary file beside it: the old contents stay intact until
/// the new ones are complete and on disk, then the rename swaps them in one step.
///
/// The temporary name is unique per process and call, so two writers racing for the same
/// file (an autosave and a rating, say) cannot interleave their bytes.
pub fn atomic_write(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let tmp = temp_path(path);
    let result = (|| {
        let mut file =
            File::create(&tmp).with_context(|| format!("could not create {}", tmp.display()))?;
        write(&mut file)?;
        file.flush()?;
        file.sync_all()
            .with_context(|| format!("could not flush {} to disk", tmp.display()))?;
        drop(file);
        fs::rename(&tmp, path).with_context(|| format!("could not replace {}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// [`atomic_write`] for contents already in memory.
pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_write(path, |f| {
        f.write_all(bytes)
            .with_context(|| format!("could not write {}", path.display()))
    })
}

fn temp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Hidden, so a file manager or the shelf's folder scan does not show it mid-write.
    path.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("duckfoot-fsio-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temporary() {
        let dir = scratch("fail");
        let target = dir.join("a.json");
        fs::write(&target, b"old").unwrap();
        let result = atomic_write(&target, |f| {
            f.write_all(b"half of the new")?;
            anyhow::bail!("disk full")
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "the temporary file was cleaned up");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_replaces_the_contents() {
        let dir = scratch("ok");
        let target = dir.join("a.json");
        fs::write(&target, b"old").unwrap();
        atomic_write_bytes(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }
}

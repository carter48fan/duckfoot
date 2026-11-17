//! Watching the folders opened whole (W-11): a file that appears in one joins the shelf,
//! and a shelved photo whose file goes is marked missing until it is back. Polled, not
//! notified, so a network share or a card reader is watched the same as a local disk; the
//! folders are read off the UI thread.
//!
//! Polled only while the window has focus (the app does not wake itself in the
//! background for this), and cheaply: a folder whose modified time has not moved is not
//! listed again. FAT cards and some network shares do not keep folder times reliably, so
//! every [`FULL_EVERY`]th read lists every folder anyway.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

/// How often the folders are read.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// Every this many reads, every folder is listed whatever its modified time says.
pub const FULL_EVERY: u32 = 15;

/// Each folder read: its modified time, and the photos in it, unless it was skipped as
/// unchanged.
type Listings = Vec<(PathBuf, Option<SystemTime>, Option<Vec<PathBuf>>)>;

/// What one read of the folders found.
pub struct Scan {
    /// Photos not in their folder at the previous read.
    pub new: Vec<PathBuf>,
    /// Every photo in the watched folders now.
    pub present: HashSet<PathBuf>,
    /// The folders read.
    pub folders: Vec<PathBuf>,
}

/// A folder watched.
struct Watched {
    dir: PathBuf,
    /// Every photo ever seen in it, so one that goes and comes back is not new.
    seen: HashSet<PathBuf>,
    /// What it held at the last listing.
    last: Vec<PathBuf>,
    /// Its modified time at the last listing, if it could be read.
    mtime: Option<SystemTime>,
}

#[derive(Default)]
pub struct FolderWatch {
    folders: Vec<Watched>,
    next: Option<Instant>,
    pending: Option<mpsc::Receiver<Listings>>,
    /// Reads since every folder was last listed.
    reads: u32,
}

fn modified(dir: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(dir).and_then(|m| m.modified()).ok()
}

/// Lists each folder whose modified time moved since `mtime`, or every one if `full`.
fn read(folders: &[(PathBuf, Option<SystemTime>)], full: bool) -> Listings {
    folders
        .iter()
        .map(|(dir, before)| {
            let now = modified(dir);
            let unchanged = !full && now.is_some() && now == *before;
            let listing = (!unchanged).then(|| super::FolderTree::scan_supported_images(dir));
            (dir.clone(), now, listing)
        })
        .collect()
}

impl FolderWatch {
    /// Watch `dir`, which holds `listing` now; a folder already watched is left as it is.
    pub fn watch(&mut self, dir: PathBuf, listing: &[PathBuf]) {
        if !self.folders.iter().any(|w| w.dir == dir) {
            self.folders.push(Watched {
                mtime: modified(&dir),
                seen: listing.iter().cloned().collect(),
                last: listing.to_vec(),
                dir,
            });
        }
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        self.folders.iter().map(|w| w.dir.clone()).collect()
    }

    pub fn forget(&mut self, dir: &std::path::Path) {
        self.folders.retain(|w| w.dir != dir);
    }

    /// The next poll reads at once (the window came back into focus, say).
    pub fn read_soon(&mut self) {
        self.next = None;
    }

    fn to_read(&self) -> Vec<(PathBuf, Option<SystemTime>)> {
        self.folders
            .iter()
            .map(|w| (w.dir.clone(), w.mtime))
            .collect()
    }

    /// A finished read, if one is; starts the next when it is due.
    pub fn poll(&mut self, now: Instant) -> Option<Scan> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(listings) => {
                    self.pending = None;
                    self.next = Some(now + INTERVAL);
                    return Some(self.take(listings));
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            }
        }
        if self.folders.is_empty() || self.next.is_some_and(|t| now < t) {
            self.next.get_or_insert(now + INTERVAL);
            return None;
        }
        let folders = self.to_read();
        let full = self.reads.is_multiple_of(FULL_EVERY);
        self.reads = self.reads.wrapping_add(1);
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("duckfoot-watch".into())
            .spawn(move || {
                let _ = tx.send(read(&folders, full));
            });
        if started.is_ok() {
            self.pending = Some(rx);
        }
        self.next = Some(now + INTERVAL);
        None
    }

    /// Read the folders now, on this thread.
    pub fn scan_now(&mut self) -> Scan {
        let listings = read(&self.to_read(), true);
        self.take(listings)
    }

    fn take(&mut self, listings: Listings) -> Scan {
        let mut scan = Scan {
            new: Vec::new(),
            present: HashSet::new(),
            folders: Vec::new(),
        };
        for (dir, mtime, listing) in listings {
            // A folder that is gone (a card pulled) reads empty; its photos go missing
            // but are not forgotten as seen, so they do not come back as new.
            let Some(watched) = self.folders.iter_mut().find(|w| w.dir == dir) else {
                continue;
            };
            if let Some(listing) = listing {
                for p in &listing {
                    if watched.seen.insert(p.clone()) {
                        scan.new.push(p.clone());
                    }
                }
                watched.last = listing;
                watched.mtime = mtime;
            }
            scan.present.extend(watched.last.iter().cloned());
            scan.folders.push(dir);
        }
        scan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder whose modified time has not moved is not listed again, unless the read
    /// is a full one; one that gained a photo is.
    #[test]
    fn an_unchanged_folder_is_not_listed_again() {
        let dir = std::env::temp_dir().join(format!("duckfoot-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::new(4, 4).save(dir.join("a.png")).unwrap();
        let mtime = modified(&dir);
        assert!(mtime.is_some());
        let quiet = read(&[(dir.clone(), mtime)], false);
        assert!(quiet[0].2.is_none(), "an unchanged folder was listed");
        let full = read(&[(dir.clone(), mtime)], true);
        assert_eq!(full[0].2.as_ref().map(Vec::len), Some(1));

        let mut watch = FolderWatch::default();
        watch.watch(dir.clone(), &[dir.join("a.png")]);
        // Folder times are kept to the second on some filesystems.
        std::thread::sleep(Duration::from_millis(1100));
        image::RgbImage::new(4, 4).save(dir.join("b.png")).unwrap();
        let listings = read(&watch.to_read(), false);
        assert!(
            listings[0].2.is_some(),
            "a folder that changed was not listed"
        );
        let scan = watch.take(listings);
        assert_eq!(scan.new, vec![dir.join("b.png")]);
        assert_eq!(scan.present.len(), 2);
        // Skipped next time, and what it holds is still known.
        let scan = watch.take(read(&watch.to_read(), false));
        assert!(scan.new.is_empty());
        assert_eq!(scan.present.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

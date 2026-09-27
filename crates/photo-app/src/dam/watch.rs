//! Watching the folders opened whole (W-11): a file that appears in one joins the shelf,
//! and a shelved photo whose file goes is marked missing until it is back. Polled, not
//! notified, so a network share or a card reader is watched the same as a local disk; the
//! folders are read off the UI thread.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How often the folders are read.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// Each folder read, with the photos in it.
type Listings = Vec<(PathBuf, Vec<PathBuf>)>;

/// What one read of the folders found.
pub struct Scan {
    /// Photos not in their folder at the previous read.
    pub new: Vec<PathBuf>,
    /// Every photo in the watched folders now.
    pub present: HashSet<PathBuf>,
    /// The folders read.
    pub folders: Vec<PathBuf>,
}

#[derive(Default)]
pub struct FolderWatch {
    /// Each folder with what it held at the last read.
    folders: Vec<(PathBuf, HashSet<PathBuf>)>,
    next: Option<Instant>,
    pending: Option<mpsc::Receiver<Listings>>,
}

fn read(folders: &[PathBuf]) -> Listings {
    folders
        .iter()
        .map(|f| (f.clone(), super::FolderTree::scan_supported_images(f)))
        .collect()
}

impl FolderWatch {
    /// Watch `dir`, which holds `listing` now; a folder already watched is left as it is.
    pub fn watch(&mut self, dir: PathBuf, listing: &[PathBuf]) {
        if !self.folders.iter().any(|(f, _)| *f == dir) {
            self.folders.push((dir, listing.iter().cloned().collect()));
        }
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        self.folders.iter().map(|(f, _)| f.clone()).collect()
    }

    pub fn forget(&mut self, dir: &std::path::Path) {
        self.folders.retain(|(f, _)| f != dir);
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
        let folders = self.folders();
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("duckfoot-watch".into())
            .spawn(move || {
                let _ = tx.send(read(&folders));
            });
        if started.is_ok() {
            self.pending = Some(rx);
        }
        self.next = Some(now + INTERVAL);
        None
    }

    /// Read the folders now, on this thread.
    pub fn scan_now(&mut self) -> Scan {
        let listings = read(&self.folders());
        self.take(listings)
    }

    fn take(&mut self, listings: Listings) -> Scan {
        let mut scan = Scan {
            new: Vec::new(),
            present: HashSet::new(),
            folders: Vec::new(),
        };
        for (dir, listing) in listings {
            // A folder that is gone (a card pulled) reads empty; its photos go missing
            // but are not forgotten as seen, so they do not come back as new.
            let Some((_, seen)) = self.folders.iter_mut().find(|(f, _)| *f == dir) else {
                continue;
            };
            for p in &listing {
                if seen.insert(p.clone()) {
                    scan.new.push(p.clone());
                }
            }
            scan.present.extend(listing);
            scan.folders.push(dir);
        }
        scan
    }
}

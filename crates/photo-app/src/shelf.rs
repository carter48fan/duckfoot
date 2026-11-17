//! The Shelf's photos, in their order, with an index by path.
//!
//! Almost everything the app does to a photo it does by path (a thumbnail arrives, a
//! sidecar is read, a rating is set), and a shelf of a whole card is tens of thousands of
//! photos. A linear search per lookup made opening a big folder quadratic on the UI
//! thread. The index makes a lookup a hash probe.
//!
//! Reads go through `Deref` to the slice, so `len`, `iter`, indexing and the rest work
//! as they did on the `Vec`. Writes that move items go through the methods here, which
//! keep the index in step. `DerefMut` hands out `&mut ShelfItem`, which is safe for the
//! index only because nothing ever changes an item's `path` after it is shelved: a photo
//! that moves is a different item.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use crate::ShelfItem;

#[derive(Default)]
pub struct Shelf {
    items: Vec<ShelfItem>,
    index: HashMap<PathBuf, usize>,
}

impl Shelf {
    /// Where `path` stands on the shelf.
    pub fn position(&self, path: &Path) -> Option<usize> {
        let i = *self.index.get(path)?;
        debug_assert_eq!(self.items[i].path, path, "the shelf's index is stale");
        Some(i)
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.index.contains_key(path)
    }

    pub fn get(&self, path: &Path) -> Option<&ShelfItem> {
        self.position(path).map(|i| &self.items[i])
    }

    pub fn get_mut(&mut self, path: &Path) -> Option<&mut ShelfItem> {
        self.position(path).map(|i| &mut self.items[i])
    }

    /// Adds `item` at the end. A path already shelved is not shelved twice.
    pub fn push(&mut self, item: ShelfItem) {
        if self.contains(&item.path) {
            return;
        }
        self.index.insert(item.path.clone(), self.items.len());
        self.items.push(item);
    }

    /// Adds `item` at `at` (or the end, if past it).
    pub fn insert(&mut self, at: usize, item: ShelfItem) {
        if self.contains(&item.path) {
            return;
        }
        let at = at.min(self.items.len());
        self.items.insert(at, item);
        self.reindex();
    }

    /// Takes off every item `gone` picks, each with the index it stood at, in one pass.
    pub fn take_where(
        &mut self,
        mut gone: impl FnMut(&ShelfItem) -> bool,
    ) -> Vec<(usize, ShelfItem)> {
        let mut taken = Vec::new();
        let mut kept = Vec::with_capacity(self.items.len());
        for (i, item) in std::mem::take(&mut self.items).into_iter().enumerate() {
            if gone(&item) {
                taken.push((i, item));
            } else {
                kept.push(item);
            }
        }
        self.items = kept;
        if !taken.is_empty() {
            self.reindex();
        }
        taken
    }

    /// Puts back what [`Self::take_where`] took, at the indices it reported. Taken in
    /// order, each index is where the item stood with the earlier ones already back, so
    /// inserting in order restores the shelf exactly.
    pub fn put_back(&mut self, taken: Vec<(usize, ShelfItem)>) {
        for (at, item) in taken {
            if self.index.contains_key(&item.path) {
                continue;
            }
            // Holds the path's place in `contains` until the rebuild below.
            self.index.insert(item.path.clone(), usize::MAX);
            let at = at.min(self.items.len());
            self.items.insert(at, item);
        }
        self.reindex();
    }

    fn reindex(&mut self) {
        self.index.clear();
        self.index.reserve(self.items.len());
        for (i, item) in self.items.iter().enumerate() {
            self.index.insert(item.path.clone(), i);
        }
    }
}

impl Deref for Shelf {
    type Target = [ShelfItem];
    fn deref(&self) -> &[ShelfItem] {
        &self.items
    }
}

impl DerefMut for Shelf {
    fn deref_mut(&mut self) -> &mut [ShelfItem] {
        &mut self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf(names: &[&str]) -> Shelf {
        let mut s = Shelf::default();
        for n in names {
            s.push(ShelfItem::new(PathBuf::from(n)));
        }
        s
    }

    fn names(s: &Shelf) -> Vec<String> {
        s.iter().map(|it| it.path.display().to_string()).collect()
    }

    fn consistent(s: &Shelf) {
        assert_eq!(s.index.len(), s.len());
        for (i, it) in s.iter().enumerate() {
            assert_eq!(s.position(&it.path), Some(i));
        }
    }

    #[test]
    fn the_index_follows_every_change() {
        let mut s = shelf(&["a", "b", "c", "d", "e"]);
        consistent(&s);
        s.push(ShelfItem::new(PathBuf::from("c")));
        assert_eq!(s.len(), 5, "a path is shelved once");

        s.insert(1, ShelfItem::new(PathBuf::from("a copy")));
        assert_eq!(names(&s), ["a", "a copy", "b", "c", "d", "e"]);
        consistent(&s);

        let taken = s.take_where(|it| ["b", "d"].contains(&it.path.to_str().unwrap()));
        assert_eq!(taken.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [2, 4]);
        assert_eq!(names(&s), ["a", "a copy", "c", "e"]);
        consistent(&s);
        assert!(!s.contains(Path::new("b")));

        s.put_back(taken);
        assert_eq!(names(&s), ["a", "a copy", "b", "c", "d", "e"]);
        consistent(&s);
    }

    #[test]
    fn an_item_is_found_and_changed_by_its_path() {
        let mut s = shelf(&["x", "y"]);
        s.get_mut(Path::new("y")).unwrap().pending = false;
        assert!(!s.get(Path::new("y")).unwrap().pending);
        assert!(s.get(Path::new("z")).is_none());
    }
}

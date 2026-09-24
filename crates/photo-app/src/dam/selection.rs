//! Multi-selection model for Shelf and Filmstrip.
//!
//! Supports single selection, Ctrl+Click disjoint toggling,
//! Shift+Click contiguous range selection, and Ctrl+A select all.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionModel {
    /// Currently displayed photo in the main stage.
    pub active: Option<PathBuf>,
    /// Set of currently selected photos (can contain multiple).
    pub selected: HashSet<PathBuf>,
    /// Anchor point for Shift-Click range selection.
    pub anchor: Option<PathBuf>,
}

impl SelectionModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_selected(&self, path: &Path) -> bool {
        self.selected.contains(path)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Single click: set active and make it the only selected item.
    pub fn select_single(&mut self, path: PathBuf) {
        self.active = Some(path.clone());
        self.anchor = Some(path.clone());
        self.selected.clear();
        self.selected.insert(path);
    }

    /// Ctrl+Click: toggle item in selection without changing anchor unless none existed.
    pub fn toggle_select(&mut self, path: PathBuf) {
        if self.selected.contains(&path) {
            self.selected.remove(&path);
            if self.active.as_ref() == Some(&path) {
                self.active = self.selected.iter().next().cloned();
            }
        } else {
            self.selected.insert(path.clone());
            self.active = Some(path.clone());
            if self.anchor.is_none() {
                self.anchor = Some(path);
            }
        }
    }

    /// Shift+Click: select all items between anchor and the target path.
    pub fn select_range(&mut self, target: PathBuf, all_items: &[PathBuf]) {
        let anchor = self.anchor.clone().unwrap_or_else(|| target.clone());
        self.active = Some(target.clone());

        let idx_a = all_items.iter().position(|p| p == &anchor);
        let idx_b = all_items.iter().position(|p| p == &target);

        if let (Some(a), Some(b)) = (idx_a, idx_b) {
            let start = a.min(b);
            let end = a.max(b);
            for item in &all_items[start..=end] {
                self.selected.insert(item.clone());
            }
        } else {
            self.selected.insert(target);
        }
    }

    /// Ctrl+A: select all items in the current view.
    pub fn select_all(&mut self, all_items: &[PathBuf]) {
        for item in all_items {
            self.selected.insert(item.clone());
        }
        if self.active.is_none() && !all_items.is_empty() {
            self.active = Some(all_items[0].clone());
            self.anchor = Some(all_items[0].clone());
        }
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_range_selects_inclusive_span() {
        let items: Vec<PathBuf> = (0..5)
            .map(|i| PathBuf::from(format!("/p/{i}.jpg")))
            .collect();
        let mut sel = SelectionModel::new();

        // Click item 1
        sel.select_single(items[1].clone());
        assert_eq!(sel.count(), 1);
        assert!(sel.is_selected(&items[1]));

        // Shift-click item 3
        sel.select_range(items[3].clone(), &items);
        assert_eq!(sel.count(), 3); // items 1, 2, 3
        assert!(sel.is_selected(&items[1]));
        assert!(sel.is_selected(&items[2]));
        assert!(sel.is_selected(&items[3]));
        assert!(!sel.is_selected(&items[0]));
        assert!(!sel.is_selected(&items[4]));
    }
}

//! Digital Asset Management filtering and sorting engine.

use photo_engine::{AssetMetadata, Flag};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RatingFilter {
    #[default]
    All,
    MinStars(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FlagFilter {
    #[default]
    All,
    PicksOnly,
    HideRejects,
    RejectsOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum EditFilter {
    #[default]
    All,
    EditedOnly,
    PristineOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SortBy {
    #[default]
    FileName,
    Rating,
    FileSize,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FilterState {
    pub rating: RatingFilter,
    pub flag: FlagFilter,
    pub edit: EditFilter,
    pub color_label: Option<String>,
    #[serde(skip)]
    pub search_query: String,
    pub sort_by: SortBy,
    pub sort_descending: bool,
}

impl FilterState {
    /// Determines whether an item matches all active filter criteria.
    pub fn matches(
        &self,
        path: &Path,
        metadata: &AssetMetadata,
        is_edited: bool,
        has_sidecar: bool,
    ) -> bool {
        // 1. Rating filter
        if let RatingFilter::MinStars(min) = self.rating {
            if metadata.rating < min {
                return false;
            }
        }

        // 2. Flag filter
        match self.flag {
            FlagFilter::All => {}
            FlagFilter::PicksOnly => {
                if metadata.flag != Flag::Pick {
                    return false;
                }
            }
            FlagFilter::HideRejects => {
                if metadata.flag == Flag::Reject {
                    return false;
                }
            }
            FlagFilter::RejectsOnly => {
                if metadata.flag != Flag::Reject {
                    return false;
                }
            }
        }

        // 3. Edit filter
        match self.edit {
            EditFilter::All => {}
            EditFilter::EditedOnly => {
                if !is_edited && !has_sidecar {
                    return false;
                }
            }
            EditFilter::PristineOnly => {
                if is_edited || has_sidecar {
                    return false;
                }
            }
        }

        // 4. Color label filter
        if let Some(target_color) = &self.color_label {
            if metadata.color_label.as_deref() != Some(target_color.as_str()) {
                return false;
            }
        }

        // 5. Text search query
        if !self.search_query.is_empty() {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            let query = self.search_query.to_lowercase();
            if !file_name.contains(&query) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_state_matches_rating_and_flag() {
        let mut filter = FilterState::default();
        let path = Path::new("/photos/DSC08450.ARW");

        let meta_good = AssetMetadata {
            rating: 4,
            flag: Flag::Pick,
            color_label: Some("green".into()),
        };

        let meta_rejected = AssetMetadata {
            rating: 5,
            flag: Flag::Reject,
            color_label: None,
        };

        assert!(filter.matches(path, &meta_good, false, false));
        assert!(filter.matches(path, &meta_rejected, false, false));

        // Filter: Hide rejects
        filter.flag = FlagFilter::HideRejects;
        assert!(filter.matches(path, &meta_good, false, false));
        assert!(!filter.matches(path, &meta_rejected, false, false));

        // Filter: Min 5 stars
        filter.rating = RatingFilter::MinStars(5);
        assert!(!filter.matches(path, &meta_good, false, false));
    }
}

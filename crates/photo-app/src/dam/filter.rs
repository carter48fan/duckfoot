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

/// A filter kept under a name (W-11's collections-lite): what it shows, how it is sorted
/// and what it searches for.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SavedFilter {
    pub name: String,
    pub filter: FilterState,
    pub search: String,
}

impl SavedFilter {
    pub fn new(name: &str, filter: &FilterState) -> Self {
        let mut filter = filter.clone();
        let search = std::mem::take(&mut filter.search_query);
        Self {
            name: name.trim().to_string(),
            filter,
            search,
        }
    }

    /// The filter it keeps, search included.
    pub fn filter(&self) -> FilterState {
        FilterState {
            search_query: self.search.clone(),
            ..self.filter.clone()
        }
    }
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
            let keyword = || {
                metadata
                    .keywords
                    .iter()
                    .any(|k| k.to_lowercase().contains(&query))
            };
            if !file_name.contains(&query) && !keyword() {
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
            keywords: vec!["Iceland".into()],
        };

        let meta_rejected = AssetMetadata {
            rating: 5,
            flag: Flag::Reject,
            color_label: None,
            keywords: Vec::new(),
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

    #[test]
    fn a_saved_filter_keeps_its_search() {
        let filter = FilterState {
            rating: RatingFilter::MinStars(3),
            search_query: "iceland".into(),
            ..Default::default()
        };
        let saved = SavedFilter::new(" Best of Iceland ", &filter);
        assert_eq!(saved.name, "Best of Iceland");
        let json = serde_json::to_string(&saved).unwrap();
        let back: SavedFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(back.filter(), filter);
    }

    #[test]
    fn a_label_or_a_keyword_finds_a_photo() {
        let path = Path::new("/photos/DSC08450.ARW");
        let mut tagged = AssetMetadata {
            color_label: Some("red".into()),
            ..Default::default()
        };
        tagged.add_keywords("Iceland, waterfall");
        let plain = AssetMetadata::default();
        let mut filter = FilterState {
            color_label: Some("red".into()),
            ..Default::default()
        };
        assert!(filter.matches(path, &tagged, false, false));
        assert!(!filter.matches(path, &plain, false, false));
        filter.color_label = None;
        filter.search_query = "WATERF".into();
        assert!(filter.matches(path, &tagged, false, false));
        assert!(!filter.matches(path, &plain, false, false));
        filter.search_query = "dsc084".into();
        assert!(
            filter.matches(path, &plain, false, false),
            "the name still finds it"
        );
    }
}

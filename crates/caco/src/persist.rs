use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct GuiState {
    #[serde(default)]
    pub view_layout: String,
    /// Which sort field, by its key: `state::SORT_FIELDS`. None is the default.
    #[serde(default)]
    pub sort_field: Option<String>,
    /// Legacy position in a list that has since changed: read for `sort_field`, and not
    /// written back.
    #[serde(default, skip_serializing)]
    pub sort_field_index: Option<usize>,
    #[serde(default = "default_true")]
    pub sort_desc: bool,
    /// Multi-select status filters (empty = all).
    #[serde(default)]
    pub status_filters: Vec<String>,
    /// Legacy single status filter — migrated to `status_filters` on load.
    #[serde(default)]
    pub status_filter: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for GuiState {
    fn default() -> Self {
        Self {
            view_layout: "grid".to_string(),
            sort_field: None,
            sort_field_index: None,
            sort_desc: true,
            status_filters: Vec::new(),
            status_filter: None,
        }
    }
}

fn state_path() -> PathBuf {
    caco_core::config::default_data_dir().join("gui-state.json")
}

pub fn load() -> GuiState {
    let path = state_path();
    if !path.exists() {
        return GuiState::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(data) => from_json(&data),
        Err(_) => GuiState::default(),
    }
}

/// What a saved state says, brought up to date.
fn from_json(data: &str) -> GuiState {
    let mut state: GuiState = serde_json::from_str(data).unwrap_or_default();
    // Migrate legacy single status_filter → status_filters
    if let Some(filter) = state.status_filter.take() {
        let migrated = match filter.as_str() {
            "inbox" | "queued" => Some("unplayed".to_string()),
            "playing" => Some("in-progress".to_string()),
            "shelved" => Some("completed".to_string()),
            "dropped" => Some("abandoned".to_string()),
            "unplayed" | "in-progress" | "completed" | "abandoned" => Some(filter),
            _ => None,
        };
        if let Some(s) = migrated
            && state.status_filters.is_empty()
        {
            state.status_filters.push(s);
        }
    }
    // What the sort used to be, by where it was in the list. The first was the default and
    // an internal row id, which nobody picks, so that one is as good as never having chosen.
    if state.sort_field.is_none() {
        state.sort_field = state
            .sort_field_index
            .take()
            .and_then(|index| LEGACY_SORT_FIELDS.get(index).copied().flatten())
            .map(str::to_string);
    }
    state
}

/// What `sort_field_index` meant.
const LEGACY_SORT_FIELDS: [Option<&str>; 8] = [
    None,
    Some("title"),
    Some("author"),
    Some("playtime"),
    Some("last_played"),
    Some("year"),
    Some("rating"),
    Some("random"),
];

pub fn save(state: &GuiState) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sort_saved_by_position_keeps_meaning_what_it_meant() {
        let sort = |json: &str| from_json(json).sort_field;
        assert_eq!(
            sort(r#"{"sort_field_index": 3}"#).as_deref(),
            Some("playtime")
        );
        assert_eq!(
            sort(r#"{"sort_field_index": 7}"#).as_deref(),
            Some("random")
        );
        // The old default, an internal row id, is no choice anybody made.
        assert_eq!(sort(r#"{"sort_field_index": 0}"#), None);
        assert_eq!(sort(r#"{"sort_field_index": 99}"#), None);
        assert_eq!(sort("{}"), None);
        // What is saved now wins over what was.
        assert_eq!(
            sort(r#"{"sort_field": "year", "sort_field_index": 3}"#).as_deref(),
            Some("year")
        );
    }

    #[test]
    fn the_old_position_is_not_written_back() {
        let saved = serde_json::to_string(&from_json(r#"{"sort_field_index": 3}"#)).unwrap();
        assert!(saved.contains("\"sort_field\":\"playtime\""), "{saved}");
        assert!(!saved.contains("sort_field_index"), "{saved}");
    }
}

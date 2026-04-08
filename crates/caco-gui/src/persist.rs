use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct GuiState {
    #[serde(default)]
    pub view_layout: String,
    #[serde(default = "default_true")]
    pub show_detail_panel: bool,
    #[serde(default)]
    pub sort_field_index: usize,
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
            show_detail_panel: false,
            sort_field_index: 0,
            sort_desc: true,
            status_filters: Vec::new(),
            status_filter: None,
        }
    }
}

fn state_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("caco");
    path.push("gui-state.json");
    path
}

pub fn load() -> GuiState {
    let path = state_path();
    if !path.exists() {
        return GuiState::default();
    }
    let mut state: GuiState = match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => return GuiState::default(),
    };
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
    state
}

pub fn save(state: &GuiState) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, data);
    }
}

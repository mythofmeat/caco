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
    // Migrate old unified status filter values to new status values
    if let Some(ref filter) = state.status_filter {
        state.status_filter = match filter.as_str() {
            "inbox" | "queued" => Some("unplayed".to_string()),
            "playing" => Some("in-progress".to_string()),
            "shelved" => Some("completed".to_string()),
            "dropped" => Some("abandoned".to_string()),
            "unplayed" | "in-progress" | "completed" | "abandoned" => state.status_filter,
            _ => None,
        };
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

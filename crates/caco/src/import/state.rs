// ---------------------------------------------------------------------------
// Import state types
// ---------------------------------------------------------------------------

pub const IMPORT_SOURCES: &[&str] = &["idgames", "doomwiki", "doomworld", "URL", "local"];

// ---------------------------------------------------------------------------
// Top-level import state
// ---------------------------------------------------------------------------

pub struct ImportState {
    pub active_source: usize,
    pub idgames: SearchState,
    pub doomwiki: SearchState,
    pub doomworld: FormState,
    pub url_form: FormState,
    pub local_form: FormState,
}

impl Default for ImportState {
    fn default() -> Self {
        Self {
            active_source: 0,
            idgames: SearchState::default(),
            doomwiki: SearchState::default(),
            doomworld: FormState::new(FormKind::Doomworld),
            url_form: FormState::new(FormKind::Url),
            local_form: FormState::new(FormKind::Local),
        }
    }
}

impl ImportState {
    /// Get a mutable reference to the search state for the given source.
    pub fn search_state_mut(&mut self, source: SearchSource) -> &mut SearchState {
        match source {
            SearchSource::Idgames => &mut self.idgames,
            SearchSource::Doomwiki => &mut self.doomwiki,
        }
    }

    /// Get a mutable reference to the form state for the given kind.
    pub fn form_state_mut(&mut self, kind: FormKind) -> &mut FormState {
        match kind {
            FormKind::Doomworld => &mut self.doomworld,
            FormKind::Url => &mut self.url_form,
            FormKind::Local => &mut self.local_form,
        }
    }
}

// ---------------------------------------------------------------------------
// Search state (idgames / doomwiki)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResultEntry>,
    pub selected_row: Option<usize>,
    pub is_searching: bool,
    pub status_text: String,
}

impl SearchState {
    /// Update state after a search completes.
    pub fn set_results(&mut self, results: Vec<SearchResultEntry>) {
        let count = results.len();
        self.is_searching = false;
        self.status_text = format!("{count} result{}", if count == 1 { "" } else { "s" });
        self.selected_row = if results.is_empty() { None } else { Some(0) };
        self.results = results;
    }
}

// ---------------------------------------------------------------------------
// Form state (doomworld / URL / local)
// ---------------------------------------------------------------------------

pub struct FormState {
    pub kind: FormKind,
    pub fields: Vec<FormField>,
    pub is_submitting: bool,
    pub status_text: String,
    /// Async file picker for the Local-import "Browse…" button. Polled each
    /// frame; result is written into the `path` field.
    pub pending_browse: Option<crate::workers::FileDialogReceiver>,
}

impl FormState {
    pub fn new(kind: FormKind) -> Self {
        let fields = match kind {
            FormKind::Doomworld => vec![
                FormField::new("url", "Forum URL", true),
                FormField::new("title", "Title", false),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
            ],
            FormKind::Url => vec![
                FormField::new("title", "Title", true),
                FormField::new("url", "Download URL", true),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
                FormField::new("notes", "Notes", false),
            ],
            FormKind::Local => vec![
                FormField::new("path", "File Path", true),
                FormField::new("title", "Title", true),
                FormField::new("author", "Author", false),
                FormField::new("year", "Year", false),
                FormField::new("tags", "Tags (comma-separated)", false),
            ],
        };
        Self {
            kind,
            fields,
            is_submitting: false,
            status_text: String::new(),
            pending_browse: None,
        }
    }

    /// Validate required fields. Returns Ok or Err with the missing field label.
    pub fn validate(&self) -> Result<(), String> {
        for f in &self.fields {
            if f.required && f.value.trim().is_empty() {
                return Err(format!("{} is required", f.label));
            }
        }
        Ok(())
    }

    /// Collect field name/value pairs.
    pub fn collect_values(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .map(|f| (f.name.to_string(), f.value.clone()))
            .collect()
    }

    /// Reset all fields and status.
    pub fn reset(&mut self) {
        for f in &mut self.fields {
            f.value.clear();
        }
        self.status_text.clear();
        self.is_submitting = false;
        self.pending_browse = None;
    }
}

pub struct FormField {
    pub name: &'static str,
    pub label: &'static str,
    pub display_label: String,
    pub value: String,
    pub required: bool,
}

impl FormField {
    fn new(name: &'static str, label: &'static str, required: bool) -> Self {
        let display_label = if required {
            format!("{label}*:")
        } else {
            format!("{label}:")
        };
        Self {
            name,
            label,
            display_label,
            value: String::new(),
            required,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    Doomworld,
    Url,
    Local,
}

// ---------------------------------------------------------------------------
// Search result types (shared with workers + messages)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SearchResultEntry {
    pub title: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub source_data: SearchSourceData,
}

impl SearchResultEntry {
    /// Display string for the table's extra column (derived from source_data).
    pub fn extra_display(&self) -> String {
        match &self.source_data {
            SearchSourceData::Idgames { rating, date, .. } => {
                let r = rating.map(|v| format!("{v:.1}")).unwrap_or_default();
                let d = date.as_deref().unwrap_or("");
                format!("{r}  {d}")
            }
            SearchSourceData::Doomwiki { year, port, .. } => {
                let y = year.map(|v| v.to_string()).unwrap_or_default();
                let p = port.as_deref().unwrap_or("");
                format!("{y}  {p}")
            }
        }
    }

    /// Identifier used when importing (derived from source_data).
    pub fn source_id(&self) -> String {
        match &self.source_data {
            SearchSourceData::Idgames { id, .. } => id.to_string(),
            SearchSourceData::Doomwiki { .. } => self.title.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SearchSourceData {
    Idgames {
        id: i64,
        rating: Option<f64>,
        date: Option<String>,
        filename: Option<String>,
    },
    Doomwiki {
        year: Option<i32>,
        iwad: Option<String>,
        port: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchSource {
    Idgames,
    Doomwiki,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idgames_entry(id: i64, title: &str) -> SearchResultEntry {
        SearchResultEntry {
            title: title.to_string(),
            author: None,
            description: None,
            source_data: SearchSourceData::Idgames {
                id,
                rating: None,
                date: None,
                filename: None,
            },
        }
    }

    #[test]
    fn set_results_selects_first_when_non_empty() {
        let mut s = SearchState::default();
        s.set_results(vec![idgames_entry(1, "a"), idgames_entry(2, "b")]);
        assert_eq!(s.selected_row, Some(0));
        assert!(!s.is_searching);
        assert_eq!(s.status_text, "2 results");
        assert_eq!(s.results.len(), 2);
    }

    #[test]
    fn set_results_clears_selection_when_empty() {
        let mut s = SearchState {
            selected_row: Some(3),
            is_searching: true,
            ..SearchState::default()
        };
        s.set_results(vec![]);
        assert_eq!(s.selected_row, None);
        assert!(!s.is_searching);
        assert_eq!(s.status_text, "0 results");
    }

    #[test]
    fn set_results_singular_status() {
        let mut s = SearchState::default();
        s.set_results(vec![idgames_entry(1, "solo")]);
        assert_eq!(s.status_text, "1 result");
    }

    #[test]
    fn form_validate_rejects_missing_required() {
        let form = FormState::new(FormKind::Doomworld);
        let err = form.validate().unwrap_err();
        assert!(err.contains("Forum URL"));
    }

    #[test]
    fn form_validate_accepts_required_filled() {
        let mut form = FormState::new(FormKind::Doomworld);
        for f in &mut form.fields {
            if f.required {
                f.value = "x".into();
            }
        }
        assert!(form.validate().is_ok());
    }

    #[test]
    fn form_reset_clears_fields_and_status() {
        let mut form = FormState::new(FormKind::Url);
        for f in &mut form.fields {
            f.value = "populated".into();
        }
        form.status_text = "boom".into();
        form.is_submitting = true;
        form.reset();
        assert!(form.fields.iter().all(|f| f.value.is_empty()));
        assert!(form.status_text.is_empty());
        assert!(!form.is_submitting);
    }

    #[test]
    fn form_collect_values_returns_all_fields() {
        let mut form = FormState::new(FormKind::Local);
        form.fields[0].value = "/tmp/a.wad".into();
        form.fields[1].value = "Title".into();
        let vals = form.collect_values();
        assert_eq!(vals[0], ("path".to_string(), "/tmp/a.wad".to_string()));
        assert_eq!(vals[1], ("title".to_string(), "Title".to_string()));
    }

    #[test]
    fn result_source_id_uses_idgames_id() {
        let e = idgames_entry(42, "wad");
        assert_eq!(e.source_id(), "42");
    }

    #[test]
    fn result_source_id_uses_doomwiki_title() {
        let e = SearchResultEntry {
            title: "Scythe".into(),
            author: None,
            description: None,
            source_data: SearchSourceData::Doomwiki {
                year: Some(2003),
                iwad: Some("doom2".into()),
                port: None,
            },
        };
        assert_eq!(e.source_id(), "Scythe");
    }

    #[test]
    fn active_source_state_mut_routes_correctly() {
        let mut state = ImportState::default();
        state.search_state_mut(SearchSource::Idgames).query = "ig".into();
        state.search_state_mut(SearchSource::Doomwiki).query = "dw".into();
        assert_eq!(state.idgames.query, "ig");
        assert_eq!(state.doomwiki.query, "dw");
    }
}

use std::path::Path;

use rusqlite::Connection;

use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

/// State for the WAD Unavailable / Link dialog.
pub struct LinkDialogState {
    pub wad_id: i64,
    pub wad_title: String,
    pub source_url: Option<String>,
    error_message: Option<String>,
    /// Active file picker, if the user clicked "Link Local File". Polled
    /// each frame; resolves to `Some(path)` or `None` (cancelled).
    pending_picker: Option<FileDialogReceiver>,
}

/// Result of showing the link dialog.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkResult {
    /// User linked a local file — WAD is now playable.
    Linked(i64),
    /// User cancelled.
    Cancelled,
    /// Dialog still open.
    Open,
}

impl LinkDialogState {
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::wads::get_wad(conn, wad_id, false).ok()??;
        Some(Self {
            wad_id,
            wad_title: wad.title,
            source_url: wad.source_url,
            error_message: None,
            pending_picker: None,
        })
    }

    fn handle_picked_file(&mut self, conn: &Connection, path: &Path) -> LinkResult {
        match link_picked_file(conn, self.wad_id, path) {
            Ok(()) => LinkResult::Linked(self.wad_id),
            Err(e) => {
                self.error_message = Some(e);
                LinkResult::Open
            }
        }
    }

    #[cfg(test)]
    fn handle_picked_file_to_cache(
        &mut self,
        conn: &Connection,
        path: &Path,
        cache_dir: &Path,
    ) -> LinkResult {
        match link_picked_file_to_cache(conn, self.wad_id, path, cache_dir) {
            Ok(()) => LinkResult::Linked(self.wad_id),
            Err(e) => {
                self.error_message = Some(e);
                LinkResult::Open
            }
        }
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> LinkResult {
        // First: check if an async file picker has returned since last frame.
        if let Some(rx) = &self.pending_picker
            && let Ok(picked) = rx.try_recv()
        {
            self.pending_picker = None;
            if let Some(path) = picked {
                let result = self.handle_picked_file(conn, &path);
                if result != LinkResult::Open {
                    return result;
                }
            }
            // Cancelled picker or failed link: fall through to keep dialog open.
        }

        let mut result = LinkResult::Open;

        egui::Window::new("WAD Unavailable")
            .collapsible(false)
            .resizable(false)
            .fixed_size([440.0, 240.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                // Title
                ui.colored_label(
                    theme::TEXT_PRIMARY,
                    egui::RichText::new(&self.wad_title).heading(),
                );

                ui.add_space(4.0);

                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "The WAD file for this entry is not available locally.\n\
                     You can open the source URL to download it, or link a local file.",
                );

                ui.add_space(8.0);
                if let Some(error) = &self.error_message {
                    ui.colored_label(theme::COLOR_ERROR, error);
                    ui.add_space(4.0);
                }
                ui.separator();
                ui.add_space(4.0);

                // Buttons
                ui.horizontal(|ui| {
                    // Open Source URL button
                    let has_url = self.source_url.is_some();
                    if ui
                        .add_enabled(has_url, egui::Button::new("Open Source URL"))
                        .clicked()
                        && let Some(url) = &self.source_url
                    {
                        let _ = open::that(url);
                    }

                    // Link Local File button — spawn an async picker. Disabled
                    // while one is already in flight.
                    let picker_busy = self.pending_picker.is_some();
                    if ui
                        .add_enabled(!picker_busy, egui::Button::new("Link Local File"))
                        .clicked()
                    {
                        self.error_message = None;
                        let req = FileDialogRequest::open()
                            .add_filter("WAD/ZIP files", &["wad", "zip", "WAD", "ZIP"])
                            .set_directory(dirs::home_dir().unwrap_or_default());
                        self.pending_picker = Some(spawn_file_dialog(Some(ctx.clone()), req));
                    }

                    if ui.button("Cancel").clicked() {
                        result = LinkResult::Cancelled;
                    }
                });
            });

        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return LinkResult::Cancelled;
        }

        result
    }
}

/// Copy the selected file into the cache dir and update the WAD record.
fn link_picked_file(conn: &Connection, wad_id: i64, path: &Path) -> Result<(), String> {
    link_picked_file_to_cache(conn, wad_id, path, &caco_core::config::get_cache_dir())
}

fn link_picked_file_to_cache(
    conn: &Connection,
    wad_id: i64,
    path: &Path,
    cache_dir: &Path,
) -> Result<(), String> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.wad".to_string());

    let dest = cache_dir.join(&filename);

    std::fs::create_dir_all(cache_dir).map_err(|e| format!("Failed to create cache dir: {e}"))?;

    // Only copy if source != destination
    if path != dest {
        std::fs::copy(path, &dest).map_err(|e| format!("Failed to copy WAD to cache: {e}"))?;
    }

    let dest_str = dest.to_string_lossy().to_string();
    let update = caco_core::db::wads::WadUpdate::new()
        .set_text("cached_path", Some(dest_str))
        .set_text("filename", Some(filename));

    caco_core::db::wads::update_wad(conn, wad_id, &update)
        .map_err(|e| format!("Failed to update WAD: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, SourceType};

    fn setup_conn() -> (rusqlite::Connection, i64) {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let wad_id = db::add_wad(&conn, &db::NewWad::new("Linked WAD", SourceType::Local)).unwrap();
        (conn, wad_id)
    }

    #[test]
    fn test_link_picked_file_updates_cached_path_and_filename() {
        let (conn, wad_id) = setup_conn();
        let source_dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("example.wad");
        std::fs::write(&source, b"PWAD").unwrap();

        link_picked_file_to_cache(&conn, wad_id, &source, cache_dir.path()).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        let expected_cached_path = cache_dir
            .path()
            .join("example.wad")
            .to_string_lossy()
            .to_string();
        assert_eq!(wad.filename.as_deref(), Some("example.wad"));
        assert_eq!(
            wad.cached_path.as_deref(),
            Some(expected_cached_path.as_str())
        );
        assert_eq!(
            std::fs::read(cache_dir.path().join("example.wad")).unwrap(),
            b"PWAD"
        );
    }

    #[test]
    fn test_failed_link_keeps_dialog_open_with_error_message() {
        let (conn, wad_id) = setup_conn();
        let mut state = LinkDialogState {
            wad_id,
            wad_title: "Linked WAD".to_string(),
            source_url: None,
            error_message: None,
            pending_picker: None,
        };

        let cache_dir = tempfile::tempdir().unwrap();
        let missing_dir = tempfile::tempdir().unwrap();
        let missing = missing_dir.path().join("missing.wad");
        let result = state.handle_picked_file_to_cache(&conn, &missing, cache_dir.path());

        assert_eq!(result, LinkResult::Open);
        assert!(
            state
                .error_message
                .as_deref()
                .is_some_and(|msg| msg.contains("Failed to copy WAD to cache"))
        );
    }
}

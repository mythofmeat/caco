use std::path::Path;

use rusqlite::Connection;

use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

/// State for the WAD Unavailable / Link dialog.
pub struct LinkDialogState {
    pub wad_id: i64,
    pub wad_title: String,
    pub source_url: Option<String>,
    /// Active file picker, if the user clicked "Link Local File". Polled
    /// each frame; resolves to `Some(path)` or `None` (cancelled).
    pending_picker: Option<FileDialogReceiver>,
}

/// Result of showing the link dialog.
pub enum LinkResult {
    /// User linked a local file — WAD is now playable.
    Linked,
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
            pending_picker: None,
        })
    }

    pub fn render(&mut self, ctx: &egui::Context, conn: &Connection) -> LinkResult {
        // First: check if an async file picker has returned since last frame.
        if let Some(rx) = &self.pending_picker
            && let Ok(picked) = rx.try_recv()
        {
            self.pending_picker = None;
            if let Some(path) = picked {
                match link_picked_file(conn, self.wad_id, &path) {
                    Ok(()) => return LinkResult::Linked,
                    Err(e) => eprintln!("Failed to link file: {e}"),
                }
            }
            // Cancelled picker or failed link: fall through to keep dialog open.
        }

        let mut result = LinkResult::Open;

        egui::Window::new("WAD Unavailable")
            .collapsible(false)
            .resizable(false)
            .fixed_size([400.0, 200.0])
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
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.wad".to_string());

    let cache_dir = caco_core::config::get_cache_dir();
    let dest = cache_dir.join(&filename);

    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("Failed to create cache dir: {e}"))?;

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

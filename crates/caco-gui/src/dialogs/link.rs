use rusqlite::Connection;

use crate::theme;

/// State for the WAD Unavailable / Link dialog.
pub struct LinkDialogState {
    pub wad_id: i64,
    pub wad_title: String,
    pub source_url: Option<String>,
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
        })
    }

    pub fn render(&self, ctx: &egui::Context, conn: &Connection) -> LinkResult {
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

                    // Link Local File button
                    if ui.button("Link Local File").clicked()
                        && let Some(true) = pick_and_link_file(conn, self.wad_id)
                    {
                        result = LinkResult::Linked;
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

/// Open a file picker, copy the selected file to the cache dir, and update the WAD's
/// cached_path and filename. Returns `Some(true)` on success, `Some(false)` on error,
/// `None` if the user cancelled the picker.
fn pick_and_link_file(conn: &Connection, wad_id: i64) -> Option<bool> {
    let start_dir = dirs::home_dir().unwrap_or_default();
    let path = rfd::FileDialog::new()
        .add_filter("WAD/ZIP files", &["wad", "zip", "WAD", "ZIP"])
        .set_directory(&start_dir)
        .pick_file()?;

    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.wad".to_string());

    // Copy file to cache directory
    let cache_dir = caco_core::config::get_cache_dir();
    let dest = cache_dir.join(&filename);

    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        eprintln!("Failed to create cache dir: {e}");
        return Some(false);
    }

    // Only copy if source != destination
    if path != dest
        && let Err(e) = std::fs::copy(&path, &dest)
    {
        eprintln!("Failed to copy WAD to cache: {e}");
        return Some(false);
    }

    // Update DB: cached_path and filename
    let dest_str = dest.to_string_lossy().to_string();
    let update = caco_core::db::wads::WadUpdate::new()
        .set_text("cached_path", Some(dest_str))
        .and_then(|u| u.set_text("filename", Some(filename)));

    match update {
        Ok(u) => {
            if let Err(e) = caco_core::db::wads::update_wad(conn, wad_id, &u) {
                eprintln!("Failed to update WAD: {e}");
                return Some(false);
            }
        }
        Err(e) => {
            eprintln!("Failed to build update: {e}");
            return Some(false);
        }
    }

    Some(true)
}

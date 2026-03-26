use rusqlite::Connection;

use crate::theme;

/// State for the delete confirmation dialog.
pub struct DeleteDialogState {
    pub wad_id: i64,
    pub wad_title: String,
    pub wad_author: Option<String>,
    pub session_count: i64,
    pub total_playtime: i64,
}

/// Result of showing the delete dialog.
pub enum DeleteResult {
    Confirmed,
    Cancelled,
    Open,
}

impl DeleteDialogState {
    /// Create a new delete dialog, fetching WAD info and stats from the DB.
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::wads::get_wad(conn, wad_id, false).ok()??;
        let (session_count, total_playtime) =
            caco_core::db::sessions::get_wad_stats(conn, wad_id).unwrap_or((0, 0));

        Some(Self {
            wad_id,
            wad_title: wad.title,
            wad_author: wad.author,
            session_count,
            total_playtime,
        })
    }

    /// Render the delete confirmation dialog. Returns the dialog result.
    pub fn render(&self, ctx: &egui::Context, conn: &Connection) -> DeleteResult {
        let mut result = DeleteResult::Open;

        egui::Window::new("Confirm Delete")
            .collapsible(false)
            .resizable(false)
            .fixed_size([350.0, 200.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                ui.colored_label(
                    theme::TEXT_PRIMARY,
                    egui::RichText::new("Delete this WAD?").heading(),
                );

                ui.add_space(4.0);

                // WAD info
                ui.colored_label(
                    theme::TEXT_ACCENT,
                    egui::RichText::new(&self.wad_title).strong(),
                );
                if let Some(author) = &self.wad_author {
                    ui.colored_label(theme::TEXT_SECONDARY, format!("by {author}"));
                }

                ui.add_space(4.0);

                // Stats
                if self.session_count > 0 {
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        format!(
                            "{} session{}, {} played",
                            self.session_count,
                            if self.session_count == 1 { "" } else { "s" },
                            caco_core::player::format_duration(self.total_playtime),
                        ),
                    );
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("Delete").color(crate::theme::COLOR_ERROR))
                        .clicked()
                    {
                        match caco_core::db::wads::delete_wad(conn, self.wad_id, false) {
                            Ok(_) => result = DeleteResult::Confirmed,
                            Err(e) => {
                                // Best-effort: log but still close
                                eprintln!("delete failed: {e}");
                                result = DeleteResult::Confirmed;
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        result = DeleteResult::Cancelled;
                    }
                });
            });

        // Also close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return DeleteResult::Cancelled;
        }

        result
    }
}

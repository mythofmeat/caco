use rusqlite::Connection;

use crate::dialogs::{Dialog, Primary, count};
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
    Error(String),
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

        let about = match &self.wad_author {
            Some(author) => format!("{} by {author}", self.wad_title),
            None => self.wad_title.clone(),
        };
        Dialog::new("Delete WAD", [420.0, 200.0])
            .about(about)
            .show(ctx, |dialog| {
                dialog
                    .scroll(|ui| {
                        if self.session_count > 0 {
                            ui.colored_label(
                                theme::TEXT_SECONDARY,
                                format!(
                                    "{}, {} played.",
                                    count(self.session_count as usize, "session", "sessions"),
                                    caco_core::player::format_duration(self.total_playtime),
                                ),
                            );
                        }
                        ui.colored_label(
                            theme::TEXT_SECONDARY,
                            "It goes in the Trash, with its play history, and can be restored \
                             from there.",
                        );
                    })
                    .buttons(|row| {
                        let ended = row.end(Some("Cancel"), Some(Primary::danger("Delete")));
                        if ended.dismissed {
                            result = DeleteResult::Cancelled;
                        }
                        if ended.primary {
                            result = match caco_core::db::wads::delete_wad(conn, self.wad_id, false)
                            {
                                Ok(_) => DeleteResult::Confirmed,
                                Err(e) => DeleteResult::Error(format!("Delete failed: {e}")),
                            };
                        }
                    })
            });

        // Also close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return DeleteResult::Cancelled;
        }

        result
    }
}

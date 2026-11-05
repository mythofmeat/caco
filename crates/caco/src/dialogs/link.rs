use std::path::{Path, PathBuf};

use caco_core::config::LinkMode;
use caco_core::downloads;
use caco_core::placement;
use caco_core::presence::StorePaths;
use rusqlite::Connection;

use crate::dialogs::{Dialog, Primary};
use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

/// State for the WAD Unavailable / Link dialog.
pub struct LinkDialogState {
    wad_id: i64,
    wad_title: String,
    /// Whether there is any page to send the user to.
    has_page: bool,
    /// Where the file was the last time caco found it, if it ever had one —
    /// what turns "not available" into "on another machine, or a drive that
    /// is not mounted".
    last_seen: Option<String>,
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
    /// User wants to get the file through their browser.
    Get(i64),
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
            has_page: !downloads::pages_for_wad(&wad).is_empty(),
            wad_title: wad.title,
            last_seen: wad.last_known_path.filter(|path| !path.is_empty()),
            error_message: None,
            pending_picker: None,
        })
    }

    /// Say why caco could not fetch the file, when it tried. Without this a mirror
    /// that has stopped answering looks the same as a WAD no mirror ever had.
    pub fn with_failure(mut self, failure: Option<String>) -> Self {
        self.error_message = failure;
        self
    }

    fn handle_picked_file(&mut self, conn: &Connection, path: &Path) -> LinkResult {
        self.handle_picked_file_in(conn, path, &StorePaths::from_config())
    }

    /// Copied whatever the link mode says. That setting is about what lands in the
    /// Downloads folder, and a file picked by hand can be anywhere, including
    /// somewhere it is meant to stay.
    fn handle_picked_file_in(
        &mut self,
        conn: &Connection,
        path: &Path,
        paths: &StorePaths,
    ) -> LinkResult {
        match link_picked_file_in(conn, self.wad_id, path, paths, LinkMode::Copy) {
            Ok(_) => LinkResult::Linked(self.wad_id),
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
        let picker_busy = self.pending_picker.is_some();
        let mut pick = false;

        Dialog::new("WAD Unavailable", [480.0, 280.0])
            .about(&self.wad_title)
            .show(ctx, |dialog| {
                dialog
                    .scroll(|ui| self.render_explanation(ui))
                    .above(|ui| {
                        if let Some(error) = &self.error_message {
                            ui.colored_label(theme::COLOR_ERROR, error);
                        }
                    })
                    .buttons(|row| {
                        let (get, link) = ("Get in Browser", "Link Local File");
                        if self.has_page {
                            row.left(|ui| {
                                pick = ui
                                    .add_enabled(!picker_busy, egui::Button::new(link))
                                    .clicked();
                            });
                            let ended = row.end(Some("Cancel"), Some(Primary::new(get)));
                            if ended.dismissed {
                                result = LinkResult::Cancelled;
                            }
                            if ended.primary {
                                result = LinkResult::Get(self.wad_id);
                            }
                        } else {
                            let primary = Primary::new(link)
                                .when(!picker_busy, "Waiting for you to pick a file");
                            let ended = row.end(Some("Cancel"), Some(primary));
                            if ended.dismissed {
                                result = LinkResult::Cancelled;
                            }
                            pick = ended.primary;
                        }
                    })
            });

        if pick {
            self.error_message = None;
            // Where a file someone downloaded a while ago will be. Both cases, because some
            // platforms' pickers go by exact match, and a WAD from the nineties is as likely
            // as not to be called DOOM.WAD.
            let extensions: Vec<String> = downloads::WAD_SHAPED
                .iter()
                .flat_map(|ext| [ext.to_string(), ext.to_uppercase()])
                .collect();
            let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
            let req = FileDialogRequest::open()
                .add_filter("WAD files", &extensions)
                .set_directory(caco_core::config::downloads_dir());
            self.pending_picker = Some(spawn_file_dialog(Some(ctx.clone()), req));
        }

        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return LinkResult::Cancelled;
        }

        result
    }

    fn render_explanation(&self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 6.0;
        match &self.last_seen {
            Some(path) => {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "This WAD's file is not on this machine. It was last seen at:",
                );
                ui.label(
                    egui::RichText::new(path)
                        .monospace()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "On another machine, or on a drive that isn't mounted? Once it's back \
                     there, Play will find it.",
                );
            }
            None => {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    "This WAD's file is not on this machine.",
                );
            }
        }
        ui.colored_label(
            theme::TEXT_SECONDARY,
            if self.has_page {
                "Get in Browser opens its page and watches your Downloads folder, so all you \
                 do is download it. Or link a copy you already have."
            } else {
                "caco doesn't know a page for it, so if you have a copy, link it."
            },
        );
    }
}

/// Put a file in the directory its WAD's retrievability calls for, update the WAD
/// record, and say where the file ended up.
///
/// A hand-picked file for a WAD caco cannot re-download is the exact case the keep dir
/// exists for: put it in the cache and the next Clean can delete the only copy. The WAD
/// is read back from the DB rather than trusting the caller, since retrievability is a
/// property of the record.
///
/// [`StorePaths`] is explicit for the reason [`caco_core::gc::GcPaths`] is: this writes
/// into those directories, and a test must not be able to reach the real ones.
pub(crate) fn link_picked_file_in(
    conn: &Connection,
    wad_id: i64,
    path: &Path,
    paths: &StorePaths,
    mode: LinkMode,
) -> Result<PathBuf, String> {
    let wad = caco_core::db::get_wad(conn, wad_id, false)
        .map_err(|e| format!("Failed to load WAD: {e}"))?
        .ok_or_else(|| "WAD not found".to_string())?;
    let dest_dir = paths.store_dir(wad.retrievability());
    let dest = placement::place_file(path, &dest_dir, mode).map_err(|e| {
        format!(
            "Could not put {} in {}: {e}",
            path.display(),
            dest_dir.display()
        )
    })?;

    let filename = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let update = caco_core::db::wads::WadUpdate::new()
        .set_text("cached_path", Some(dest.to_string_lossy().into_owned()))
        .set_text("filename", filename);
    caco_core::db::wads::update_wad(conn, wad_id, &update)
        .map_err(|e| format!("Failed to update WAD: {e}"))?;

    Ok(dest)
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
    fn test_dialog_says_where_the_file_was_last_seen() {
        let (conn, wad_id) = setup_conn();
        let last_seen =
            |conn: &rusqlite::Connection| LinkDialogState::new(conn, wad_id).unwrap().last_seen;
        assert_eq!(last_seen(&conn), None);

        for (stored, shown) in [
            ("", None),
            ("/mnt/nas/myhouse.pk3", Some("/mnt/nas/myhouse.pk3")),
        ] {
            conn.execute(
                "UPDATE wads SET last_known_path = ?1 WHERE id = ?2",
                rusqlite::params![stored, wad_id],
            )
            .unwrap();
            assert_eq!(last_seen(&conn).as_deref(), shown);
        }
    }

    /// The button that sends the user to a browser needs a page to send them to, and
    /// a local import's `source_url` is a path on this machine.
    #[test]
    fn test_get_in_browser_needs_a_page_to_open() {
        let (conn, wad_id) = setup_conn();
        let has_page =
            |conn: &rusqlite::Connection| LinkDialogState::new(conn, wad_id).unwrap().has_page;
        assert!(!has_page(&conn));

        for (stored, expected) in [
            ("/home/me/doom/example.wad", false),
            ("https://www.doomworld.com/forum/topic/1-example/", true),
        ] {
            conn.execute(
                "UPDATE wads SET source_url = ?1 WHERE id = ?2",
                rusqlite::params![stored, wad_id],
            )
            .unwrap();
            assert_eq!(has_page(&conn), expected, "{stored}");
        }
    }

    #[test]
    fn test_dialog_says_why_a_fetch_failed() {
        let (conn, wad_id) = setup_conn();
        let dialog = LinkDialogState::new(&conn, wad_id).unwrap();
        assert_eq!(dialog.error_message, None);

        let why = "Mirror home: could not connect".to_string();
        let dialog = dialog.with_failure(Some(why.clone()));
        assert_eq!(dialog.error_message, Some(why));
    }

    struct Store {
        paths: StorePaths,
        _root: tempfile::TempDir,
    }

    fn store() -> Store {
        let root = tempfile::tempdir().unwrap();
        Store {
            paths: StorePaths {
                cache_dir: root.path().join("cache"),
                keep_dir: root.path().join("keep"),
            },
            _root: root,
        }
    }

    #[test]
    fn test_link_picked_file_updates_cached_path_and_filename() {
        let (conn, wad_id) = setup_conn();
        // Linking a copy answers "where did it go", so the old answer goes.
        conn.execute(
            "UPDATE wads SET last_known_path = '/mnt/nas/example.wad' WHERE id = ?1",
            [wad_id],
        )
        .unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let store = store();
        let source = source_dir.path().join("example.wad");
        std::fs::write(&source, b"PWAD").unwrap();

        let dest =
            link_picked_file_in(&conn, wad_id, &source, &store.paths, LinkMode::Copy).unwrap();

        // Nothing could fetch this one again, so it goes where it stays.
        assert_eq!(dest, store.paths.keep_dir.join("example.wad"));
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.filename.as_deref(), Some("example.wad"));
        assert_eq!(wad.cached_path.as_deref(), dest.to_str());
        assert_eq!(wad.last_known_path, None);
        assert_eq!(std::fs::read(&dest).unwrap(), b"PWAD");
        assert!(source.exists(), "a copy leaves the original alone");
    }

    #[test]
    fn test_a_wad_caco_can_fetch_again_goes_in_the_cache() {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let wad_id = db::add_wad(
            &conn,
            &db::NewWad::new("Scythe", SourceType::Idgames).idgames_id("42"),
        )
        .unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let store = store();
        let source = source_dir.path().join("scythe.zip");
        std::fs::write(&source, b"PK").unwrap();

        let dest =
            link_picked_file_in(&conn, wad_id, &source, &store.paths, LinkMode::Copy).unwrap();

        assert_eq!(dest, store.paths.cache_dir.join("scythe.zip"));
    }

    /// `map01.wad` is a common name, and the second WAD to get one used to overwrite
    /// the first one's only copy.
    #[test]
    fn test_linking_a_file_does_not_overwrite_another_wads() {
        let (conn, wad_id) = setup_conn();
        let store = store();
        std::fs::create_dir_all(&store.paths.keep_dir).unwrap();
        let theirs = store.paths.keep_dir.join("map01.wad");
        std::fs::write(&theirs, b"another WAD's only copy").unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("map01.wad");
        std::fs::write(&source, b"this one").unwrap();

        let dest =
            link_picked_file_in(&conn, wad_id, &source, &store.paths, LinkMode::Copy).unwrap();

        assert_eq!(std::fs::read(&theirs).unwrap(), b"another WAD's only copy");
        assert_eq!(std::fs::read(&dest).unwrap(), b"this one");
        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert_eq!(wad.cached_path.as_deref(), dest.to_str());
        assert_eq!(
            wad.filename.as_deref(),
            dest.file_name().and_then(|name| name.to_str()),
            "the name it is actually stored under"
        );
    }

    /// A file picked by hand can be anywhere, including somewhere it is meant to stay.
    #[test]
    fn test_a_hand_picked_file_stays_where_it_was_whatever_the_link_mode() {
        let (conn, wad_id) = setup_conn();
        let store = store();
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("example.wad");
        std::fs::write(&source, b"PWAD").unwrap();
        let mut state = LinkDialogState::new(&conn, wad_id).unwrap();

        let result = state.handle_picked_file_in(&conn, &source, &store.paths);

        assert_eq!(result, LinkResult::Linked(wad_id));
        assert!(source.exists());
    }

    #[test]
    fn test_failed_link_keeps_dialog_open_with_error_message() {
        let (conn, wad_id) = setup_conn();
        let mut state = LinkDialogState {
            wad_id,
            wad_title: "Linked WAD".to_string(),
            has_page: false,
            last_seen: None,
            error_message: None,
            pending_picker: None,
        };

        let store = store();
        let missing_dir = tempfile::tempdir().unwrap();
        let missing = missing_dir.path().join("missing.wad");
        let result = state.handle_picked_file_in(&conn, &missing, &store.paths);

        assert_eq!(result, LinkResult::Open);
        let message = state.error_message.unwrap();
        assert!(message.contains("missing.wad"), "{message}");
        assert!(
            message.contains(&store.paths.keep_dir.display().to_string()),
            "{message}"
        );
        assert_eq!(
            db::get_wad(&conn, wad_id, false)
                .unwrap()
                .unwrap()
                .cached_path,
            None
        );
    }
}

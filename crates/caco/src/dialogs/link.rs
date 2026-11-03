use std::path::{Path, PathBuf};

use caco_core::config::LinkMode;
use caco_core::placement;
use caco_core::presence::StorePaths;
use rusqlite::Connection;

use crate::theme;
use crate::workers::{FileDialogReceiver, FileDialogRequest, spawn_file_dialog};

/// State for the WAD Unavailable / Link dialog.
pub struct LinkDialogState {
    wad_id: i64,
    wad_title: String,
    source_url: Option<String>,
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

        crate::dialogs::modal_window(ctx, "WAD Unavailable", [440.0, 240.0]).show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            // Title
            ui.colored_label(
                theme::TEXT_PRIMARY,
                egui::RichText::new(&self.wad_title).heading(),
            );

            ui.add_space(4.0);

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
                        "On another machine, or on a drive that isn't mounted? Once it's \
                         back there, Play will find it. Otherwise, open the source URL to \
                         download it again, or link a local copy.",
                    );
                }
                None => {
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        "The WAD file for this entry is not available locally.\n\
                         You can open the source URL to download it, or link a local file.",
                    );
                }
            }

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
            source_url: None,
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

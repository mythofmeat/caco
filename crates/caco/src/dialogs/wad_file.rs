//! The edit dialog's account of a WAD's file: where it is, what it is, and which
//! mirrors have it.
//!
//! Nothing here edits anything. The identity is caco's record of the file, kept by
//! `caco_core::identity`, not a field to edit. It is shown even when the file
//! has gone, because then it is the one thing that can still name the file,
//! to a mirror or to Wad Archive's index, which is keyed by the SHA-1 of the
//! WAD inside a zip. The one action is uploading it to a mirror, for a file that
//! was already here when the mirror was set up, which nothing else would ever offer.

use caco_core::config::MirrorConfig;
use caco_core::db::{WadRecord, mirrors};
use caco_core::identity::{self, FileIdentity, InnerFile};
use caco_core::utils::format_size;
use rusqlite::Connection;

use crate::theme;

pub struct WadFileSection {
    location: Location,
    identity: Option<FileIdentity>,
    inner: Vec<InnerFile>,
    /// Other WADs whose files play the same, as `(id, title)`.
    same_as: Vec<(i64, String)>,
    /// The mirrors holding this WAD's file, by name.
    on_mirrors: Vec<String>,
    /// The mirrors it could be uploaded to: writable, and without it.
    upload_targets: Vec<String>,
    /// The mirror an upload was last started to, this time the dialog was open.
    uploading_to: Option<String>,
}

enum Location {
    Here(String),
    LastSeen(String),
    Nowhere,
}

impl WadFileSection {
    pub fn load(conn: &Connection, wad: &WadRecord) -> Self {
        Self::load_with(conn, wad, &caco_core::config::load_config().mirrors)
    }

    fn load_with(conn: &Connection, wad: &WadRecord, configured: &[MirrorConfig]) -> Self {
        let nonempty = |path: &Option<String>| path.clone().filter(|path| !path.is_empty());
        let on_mirrors = mirror_names(conn, wad.id);
        let has_file = nonempty(&wad.cached_path).is_some();
        let upload_targets = configured
            .iter()
            .filter(|mirror| mirror.takes_uploads())
            .filter(|mirror| has_file && !on_mirrors.contains(&mirror.name))
            .map(|mirror| mirror.name.clone())
            .collect();
        let location = match (nonempty(&wad.cached_path), nonempty(&wad.last_known_path)) {
            (Some(path), _) => Location::Here(path),
            (None, Some(path)) => Location::LastSeen(path),
            (None, None) => Location::Nowhere,
        };
        Self {
            location,
            identity: wad.identity.clone(),
            inner: identity::inner_files(conn, wad.id).unwrap_or_default(),
            same_as: identity::same_content(conn, wad.id).unwrap_or_default(),
            on_mirrors,
            upload_targets,
            uploading_to: None,
        }
    }

    /// Show the section. Returns a mirror to upload the file to, if one was asked for.
    pub fn render(&mut self, ui: &mut egui::Ui) -> Option<String> {
        match &self.location {
            Location::Here(path) => mono(ui, path),
            Location::LastSeen(path) => {
                secondary(ui, "Not on this machine. Last seen at:");
                mono(ui, path);
            }
            Location::Nowhere => secondary(ui, "No file yet."),
        }
        let upload = self.render_mirrors(ui);
        self.render_identity(ui);
        upload
    }

    /// Before the hashes, because this is what can get the file back, and they run
    /// past the bottom of the tab.
    fn render_mirrors(&mut self, ui: &mut egui::Ui) -> Option<String> {
        if !self.on_mirrors.is_empty() {
            secondary(ui, &format!("On mirror: {}", self.on_mirrors.join(", ")));
        }
        if let Some(mirror) = &self.uploading_to {
            secondary(
                ui,
                &format!("Uploading to {mirror}. The status bar will say when it is done."),
            );
        }

        let mut clicked = None;
        ui.horizontal_wrapped(|ui| {
            for mirror in &self.upload_targets {
                if ui.button(format!("Upload to {mirror}")).clicked() {
                    clicked = Some(mirror.clone());
                }
            }
        });
        if let Some(mirror) = &clicked {
            // A second click would only upload it again.
            self.upload_targets.retain(|target| target != mirror);
            self.uploading_to = Some(mirror.clone());
        }
        clicked
    }

    fn render_identity(&self, ui: &mut egui::Ui) {
        // First, because it is the one line here that asks something of the
        // user, and the hashes below run past the bottom of the tab.
        if !self.same_as.is_empty() {
            let others: Vec<String> = self
                .same_as
                .iter()
                .map(|(_, title)| title.clone())
                .collect();
            ui.colored_label(
                theme::COLOR_WARNING,
                format!("Same WAD as {}", others.join(", ")),
            );
        }

        let Some(identity) = &self.identity else {
            if matches!(self.location, Location::Here(_)) {
                secondary(
                    ui,
                    "caco reads a new file when it starts, and before playing it, so there is \
                     nothing to show about this one yet.",
                );
            }
            return;
        };

        egui::Grid::new("wad_file_identity")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                secondary(ui, "Size");
                ui.label(egui::RichText::new(size_with_bytes(identity.file.size)).size(12.0));
                ui.end_row();
                secondary(ui, "SHA-256");
                mono(ui, &identity.file.sha256);
                ui.end_row();
                secondary(ui, "SHA-1");
                mono(ui, &identity.file.sha1);
                ui.end_row();
            });

        if !self.inner.is_empty() {
            ui.add_space(4.0);
            // In a zip that only wraps loadable files, they are what plays. In
            // anything else they are part of it, and so is everything around them.
            secondary(
                ui,
                if identity.is_container() {
                    "Plays"
                } else {
                    "Inside"
                },
            );
            for file in &self.inner {
                ui.horizontal_wrapped(|ui| {
                    mono(ui, &file.path);
                    secondary(ui, &format_size(file.hashes.size));
                });
                mono(ui, &format!("SHA-1 {}", file.hashes.sha1));
            }
        }
    }
}

/// The names of the mirrors holding a WAD's file, each once.
fn mirror_names(conn: &Connection, wad_id: i64) -> Vec<String> {
    let mut names: Vec<String> = mirrors::files_for_wad(conn, wad_id)
        .unwrap_or_default()
        .into_iter()
        .map(|file| file.mirror)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// "12.3 MB (12,912,310 bytes)": the rounded figure to read, the exact one to
/// compare against another copy.
fn size_with_bytes(size: u64) -> String {
    let digits = size.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{} ({grouped} bytes)", format_size(size))
}

fn mono(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .monospace()
                .size(11.0)
                .color(theme::TEXT_PRIMARY),
        )
        .wrap(),
    );
}

fn secondary(ui: &mut egui::Ui, text: &str) {
    ui.colored_label(theme::TEXT_SECONDARY, egui::RichText::new(text).size(12.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, NewWad, SourceType};
    use caco_core::mirrors::MirrorEntry;

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn writable(name: &str) -> MirrorConfig {
        MirrorConfig {
            name: name.to_string(),
            path: Some("/mnt/nas".to_string()),
            write: true,
            ..Default::default()
        }
    }

    /// A manual-only WAD on a thread two mirrors could list.
    fn setup(new: NewWad) -> (Connection, WadRecord) {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        let id = db::add_wad(
            &conn,
            &new.source_url("https://www.doomworld.com/vb/thread/1"),
        )
        .unwrap();
        let wad = db::get_wad(&conn, id, false).unwrap().unwrap();
        (conn, wad)
    }

    fn hold_it(conn: &Connection, mirror: &str) {
        let entry = MirrorEntry {
            sha256: HASH.to_string(),
            size: 4,
            filename: "road.zip".to_string(),
            doomworld: Some(1),
            ..Default::default()
        };
        mirrors::sync_files(conn, mirror, &[entry]).unwrap();
    }

    #[test]
    fn test_section_lists_the_mirrors_holding_the_file() {
        let (conn, wad) = setup(NewWad::new("Road", SourceType::Doomworld));
        assert!(
            WadFileSection::load_with(&conn, &wad, &[])
                .on_mirrors
                .is_empty()
        );

        hold_it(&conn, "nas");
        hold_it(&conn, "home");

        assert_eq!(
            WadFileSection::load_with(&conn, &wad, &[]).on_mirrors,
            ["home", "nas"]
        );
    }

    #[test]
    fn test_a_file_here_can_go_to_any_writable_mirror_without_it() {
        let (conn, wad) =
            setup(NewWad::new("Road", SourceType::Doomworld).cached_path("/keep/road.zip"));
        hold_it(&conn, "home");
        let read_only = MirrorConfig {
            write: false,
            ..writable("read-only")
        };

        let section =
            WadFileSection::load_with(&conn, &wad, &[writable("home"), read_only, writable("nas")]);

        assert_eq!(section.upload_targets, ["nas"]);
    }

    #[test]
    fn test_no_file_here_means_nothing_to_upload() {
        let (conn, wad) = setup(NewWad::new("Road", SourceType::Doomworld));
        let section = WadFileSection::load_with(&conn, &wad, &[writable("home")]);
        assert!(section.upload_targets.is_empty());
    }

    #[test]
    fn test_size_with_bytes_groups_thousands() {
        assert_eq!(size_with_bytes(512), "512 B (512 bytes)");
        assert_eq!(size_with_bytes(1024), "1.0 KB (1,024 bytes)");
        assert_eq!(size_with_bytes(12_912_310), "12.3 MB (12,912,310 bytes)");
    }
}

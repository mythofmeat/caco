//! The edit dialog's account of a WAD's file: where it is, and what it is.
//!
//! Read-only. The identity is caco's record of the file, kept by
//! `caco_core::identity`, not a field to edit. It is shown even when the file
//! has gone, because then it is the one thing that can still name the file,
//! to a mirror or to Wad Archive's index, which is keyed by the SHA-1 of the
//! WAD inside a zip.

use caco_core::db::WadRecord;
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
}

enum Location {
    Here(String),
    LastSeen(String),
    Nowhere,
}

impl WadFileSection {
    pub fn load(conn: &Connection, wad: &WadRecord) -> Self {
        let nonempty = |path: &Option<String>| path.clone().filter(|path| !path.is_empty());
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
        }
    }

    pub fn render(&self, ui: &mut egui::Ui) {
        match &self.location {
            Location::Here(path) => mono(ui, path),
            Location::LastSeen(path) => {
                secondary(ui, "Not on this machine. Last seen at:");
                mono(ui, path);
            }
            Location::Nowhere => secondary(ui, "No file yet."),
        }

        // First, because it is the one line here that asks something of the
        // user, and the hashes below run past the bottom of the tab.
        if !self.same_as.is_empty() {
            let others: Vec<String> = self
                .same_as
                .iter()
                .map(|(id, title)| format!("{title} (#{id})"))
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
                    "Not hashed yet. caco hashes a new file at startup and before it is played.",
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

    #[test]
    fn test_size_with_bytes_groups_thousands() {
        assert_eq!(size_with_bytes(512), "512 B (512 bytes)");
        assert_eq!(size_with_bytes(1024), "1.0 KB (1,024 bytes)");
        assert_eq!(size_with_bytes(12_912_310), "12.3 MB (12,912,310 bytes)");
    }
}

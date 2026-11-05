//! The question put when a file lands in the Downloads folder while a Get is waiting.
//!
//! Not a modal, because the user is not necessarily here to answer it: they are in
//! their browser, or playing something else, and nothing they might be doing in caco
//! should be interrupted by a file arriving. So it sits in the corner over whatever
//! else is open, and it asks rather than assumes because the folder is theirs and the
//! file may be nothing to do with caco.

use std::path::Path;

use caco_core::config::LinkMode;
use caco_core::utils::format_size;
use egui::text::{LayoutJob, TextFormat};

use crate::dialogs::{Dialog, Primary};
use crate::get::Asking;
use crate::theme;

const WIDTH: f32 = 360.0;

/// What the user said about the file.
#[derive(Debug, PartialEq, Eq)]
pub enum GetAnswer {
    Link,
    NotThisOne,
}

/// Ask about the file. In a foreground layer, because the point of asking this way is
/// that a dialog being open somewhere does not stop it being seen.
pub fn render(ctx: &egui::Context, asking: &Asking<'_>, mode: LinkMode) -> Option<GetAnswer> {
    let mut answer = None;
    Dialog::new("Download Finished", [WIDTH, 240.0])
        .in_the_corner()
        .show(ctx, |dialog| {
            dialog
                .scroll(|ui| {
                    ui.label(question(ui, asking));
                    ui.colored_label(
                        theme::TEXT_SECONDARY,
                        egui::RichText::new(details(asking.path, asking.size, mode)).size(11.0),
                    );
                })
                .buttons(|row| {
                    let ended = row.end(Some("Not This One"), Some(Primary::new("Link")));
                    if ended.dismissed {
                        answer = Some(GetAnswer::NotThisOne);
                    }
                    if ended.primary {
                        answer = Some(GetAnswer::Link);
                    }
                })
        });
    answer
}

/// "Link `AftercareSessions.wad` to **Aftercare Sessions**?"
fn question(ui: &egui::Ui, asking: &Asking<'_>) -> LayoutJob {
    let plain = TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        color: theme::TEXT_PRIMARY,
        ..Default::default()
    };
    let file = TextFormat {
        font_id: egui::TextStyle::Monospace.resolve(ui.style()),
        color: theme::TEXT_ACCENT,
        ..Default::default()
    };
    let title = TextFormat {
        color: theme::TEXT_ACCENT,
        ..plain.clone()
    };

    let name = asking
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut job = LayoutJob::default();
    job.append("Link ", 0.0, plain.clone());
    job.append(&name, 0.0, file);
    job.append(" to ", 0.0, plain.clone());
    job.append(asking.title, 0.0, title);
    job.append("?", 0.0, plain);
    job.wrap.max_width = ui.available_width();
    job
}

/// "24.3 MB in /home/me/Downloads. Linking moves it into your library."
fn details(path: &Path, size: u64, mode: LinkMode) -> String {
    let what_linking_does = match mode {
        LinkMode::Move => "moves it into your library",
        LinkMode::Copy => "copies it into your library",
    };
    let folder = path.parent().unwrap_or(path).display();
    format!(
        "{} in {folder}. Linking {what_linking_does}.",
        format_size(size)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moving a file out of someone's Downloads folder is worth saying before it happens.
    #[test]
    fn test_details_say_what_linking_will_do_to_the_file() {
        let path = Path::new("/home/me/Downloads/road.zip");
        assert_eq!(
            details(path, 1536, LinkMode::Move),
            "1.5 KB in /home/me/Downloads. Linking moves it into your library."
        );
        assert_eq!(
            details(path, 1536, LinkMode::Copy),
            "1.5 KB in /home/me/Downloads. Linking copies it into your library."
        );
    }
}

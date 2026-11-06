//! Modal dialogs. Each is built with [`Dialog`], which is the only thing that opens a window.

pub mod cache;
pub mod cacoward_link;
pub mod collections;
pub mod companions;
pub mod delete;
pub mod edit;
pub mod enrich;
pub mod file_changed;
pub mod gc;
pub mod get_prompt;
pub mod link;
pub mod mirror_offer;
pub mod mirror_setup;
pub mod mirrors;
pub mod newer_saves;
pub mod profiles;
pub mod resources;
pub mod sessions;
pub mod settings;
mod skeleton;
pub mod sourceports;
pub mod storage;
pub mod sync;
pub mod trash;
pub mod wad_data;
pub mod wad_file;
pub mod wad_stats;

pub use skeleton::{
    Dialog, DialogBody, DialogDone, DialogFooter, Ended, FooterRow, Primary, StatusLine, count,
    dialog_table, form_label, scroll_body, tab_bar,
};

/// A path as a person would write it: under their home folder, a `~`.
pub fn short_path(path: &str) -> String {
    match dirs::home_dir() {
        Some(home) => path_under_home(path, &home),
        None => path.to_string(),
    }
}

fn path_under_home(path: &str, home: &std::path::Path) -> String {
    match std::path::Path::new(path).strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string(),
    }
}

/// The opt-in that lets a bulk delete touch manually added files.
///
/// Cache and Clean both delete `wads.cached_path` files, and both know which
/// of them are irreplaceable — Manual retrievability with a file actually on
/// disk — but they used to disagree about what a bulk action does with those.
/// Cache excluded them outright, with no way to bulk-clear one at all. Clean
/// merely started them unticked, and its "All" button then swept them in with
/// one click, no relabel and no warning. Same file, same deletion, two rules.
///
/// One rule now: a bulk action skips them unless this is ticked. Deleting them
/// is possible, as it must be — a library can accumulate junk that is manual
/// only because it never came from idgames — but it is never what a button
/// press does by default.
///
/// Returns true if the toggle is on. Renders nothing when `manual_files` is 0,
/// since there is nothing to arm.
pub fn include_at_risk_toggle(ui: &mut egui::Ui, on: &mut bool, manual_files: usize) -> bool {
    if manual_files == 0 {
        *on = false;
        return false;
    }
    ui.checkbox(
        on,
        egui::RichText::new(format!(
            "Include {} caco can't download again",
            count(manual_files, "file", "files")
        ))
        .size(11.0),
    )
    .on_hover_text("These aren't on idgames or one of your mirrors, so deleting one loses it.");
    *on
}

/// Put something between the app and a modal: dimmed, so that it reads as "the window is
/// blocked" rather than as a stray floating panel, and taking the pointer and the keyboard, so
/// that it is.
///
/// A window is only in front of the panels where it actually is, so without this a WAD behind
/// Settings could be double-clicked and launched, and a card's tooltip could turn up on top of
/// the dialog. Being the modal layer is what does the same for the keyboard: Tab would go on
/// past a dialog's controls to the sidebar, where Enter presses what it lands on, and a field
/// that had focus when the dialog opened would go on taking what is typed.
///
/// In a layer of its own, after the panels' in the same `Order` and so in front of them, and an
/// `Order` below the windows. Disabling the panels would do it too, but that fades them, and
/// their backgrounds with them.
pub fn modal_backdrop(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let area = egui::Area::new(egui::Id::unique("modal_backdrop"))
        .order(egui::Order::Background)
        .fixed_pos(screen.min);
    ctx.memory_mut(|m| m.set_modal_layer(area.layer()));
    area.show(ctx, |ui| {
        ui.painter().rect_filled(screen, 0.0, crate::theme::SCRIM);
        // Not `click_and_drag()`, which can take focus: Tab would stop on the backdrop.
        ui.allocate_response(screen.size(), egui::Sense::CLICK | egui::Sense::DRAG);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_under_home_starts_with_a_tilde_and_nothing_else_changes() {
        let home = std::path::Path::new("/home/someone");
        assert_eq!(
            path_under_home("/home/someone/.cache/caco/wads/be2.zip", home),
            "~/.cache/caco/wads/be2.zip"
        );
        assert_eq!(
            path_under_home("/mnt/nas/be2.zip", home),
            "/mnt/nas/be2.zip"
        );
        // Not a path under it, just a name that starts the same.
        assert_eq!(
            path_under_home("/home/someone-else/be2.zip", home),
            "/home/someone-else/be2.zip"
        );
    }
}

//! Modal dialogs, plus the one place that decides how big a modal may be.

pub mod cache;
pub mod cacoward_link;
pub mod collections;
pub mod companions;
pub mod delete;
pub mod edit;
pub mod enrich;
pub mod gc;
pub mod link;
pub mod profiles;
pub mod resources;
pub mod sessions;
pub mod settings;
pub mod sourceports;
pub mod storage;
pub mod trash;
pub mod wad_data;
pub mod wad_stats;

/// Margin left between a dialog and the app window edge, per side.
const DIALOG_GUTTER: f32 = 24.0;

/// Build a modal window that cannot escape the app window.
///
/// Every dialog used to spell out its own `collapsible(false).resizable(..)
/// .default_size([W, H]).anchor(CENTER_CENTER)`, with `W`/`H` picked for a
/// comfortable window and never checked against a small one. Since egui only
/// treats `default_size` as a starting point and lets content push a window
/// past the viewport, dialogs like Sourceports (820x620) rendered larger than the
/// 800x400 minimum window — title bar above the top edge, buttons below the
/// bottom one, and no way to reach either.
///
/// So the size is a *request* here, clamped to what the window can actually
/// show. `max_size` keeps content growth from re-breaking it, and callers
/// scroll their bodies so clamping costs a scrollbar rather than a hidden
/// button row.
///
/// egui derives a window's identity from its title, so a dialog whose title
/// carries a WAD name gets a fresh identity per WAD and forgets any resize the
/// user made. Those callers chain `.id(..)` with a stable value.
pub fn modal_window(
    ctx: &egui::Context,
    title: impl Into<egui::WidgetText>,
    desired: [f32; 2],
) -> egui::Window<'static> {
    let avail = ctx.content_rect().size() - egui::vec2(DIALOG_GUTTER * 2.0, DIALOG_GUTTER * 2.0);
    let max = egui::vec2(avail.x.max(240.0), avail.y.max(160.0));
    let size = egui::vec2(desired[0].min(max.x), desired[1].min(max.y));

    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .max_size(max)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .constrain(true)
}

/// Scroll a modal's body, reserving `footer` points for the button row that
/// follows it.
///
/// [`modal_window`]'s `max_size` alone is not enough: egui resolves a window's
/// size as `natural.at_most(max).at_least(min)`, so a body whose *minimum*
/// height exceeds the window — a long list, an unwrappable URL — wins over the
/// cap and pushes the window past the viewport anyway. A scroll area has a
/// small minimum by construction, so this is what actually makes the cap bind.
///
/// Callers pass the height of their own footer because it stays outside the
/// scroll: the Close button must not be the thing that scrolls out of reach.
pub fn modal_body<R>(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    footer: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    scroll_body(ui, modal_body_height(ctx, footer), add)
}

/// Scroll a region whose height has already been decided by whoever owns the
/// window.
///
/// [`modal_body`] budgets against the screen, which is right for a dialog that
/// *is* the window and wrong for one rendered inside another — a Storage tab
/// asking the screen how tall it may be gets an answer that ignores the tab
/// strip and the Close button above and below it, and every tab then overflows
/// by the same amount. The tab is handed its budget instead.
pub fn scroll_body<R>(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(height.max(MIN_BODY_HEIGHT))
        .show(ui, add)
        .inner
}

/// How tall a modal's scrolling region may be, given `chrome` points of
/// furniture that does not scroll — title bar, header, button row.
///
/// Budgeted from the screen rather than from `ui.available_height()` on
/// purpose. Inside a window that is still settling, `available_height` reports
/// the size egui picked *last* frame, so a cap derived from it chases its own
/// tail and lands a few points too generous — enough to push a button row off
/// the bottom edge and never converge. The screen does not move.
pub fn modal_body_height(ctx: &egui::Context, chrome: f32) -> f32 {
    (ctx.content_rect().height() - DIALOG_GUTTER * 2.0 - chrome).max(MIN_BODY_HEIGHT)
}

/// Floor for any scrolling body. Below this a list is not worth showing, and
/// the window is going to spill whatever we do — better it spill by a little
/// with a usable scrollbar than collapse to nothing.
const MIN_BODY_HEIGHT: f32 = 80.0;

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
            "Include {manual_files} manually added file{}",
            if manual_files == 1 { "" } else { "s" }
        ))
        .size(11.0),
    )
    .on_hover_text(
        "These files did not come from idgames, so caco cannot download them again after deletion.",
    );
    *on
}

/// Dim the app behind a modal, so it reads as "the window is blocked" rather
/// than as a stray floating panel.
pub fn dim_backdrop(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("modal_backdrop"),
    ))
    .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(160));
}

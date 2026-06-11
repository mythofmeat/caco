//! Modal dialog dispatch — render whichever dialog is active, handle its result.

use rusqlite::Connection;

use crate::dialogs::cache::CacheResult;
use crate::dialogs::cacoward_link::CacowardLinkResult;
use crate::dialogs::collections::CollectionsResult;
use crate::dialogs::delete::DeleteResult;
use crate::dialogs::edit::EditResult;
use crate::dialogs::link::LinkResult;
use crate::dialogs::resources::ResourcesResult;
use crate::dialogs::sessions::SessionsResult;
use crate::dialogs::settings::SettingsResult;
use crate::dialogs::stats::StatsResult;
use crate::dialogs::wad_stats::WadStatsResult;
use crate::message::Notification;
use crate::state::{ActionRequest, ActiveDialog, AppState};

use super::help::{render_about_dialog, render_help_dialog};

/// Render the currently-active dialog (if any) and handle its result.
///
/// On dialog close: clears `state.active_dialog` and sets `needs_reload` when
/// the dialog reports modifications.
pub(super) fn render_active_dialog(
    state: &mut AppState,
    conn: &Connection,
    ctx: &egui::Context,
) -> Option<ActionRequest> {
    let mut close_dialog = false;
    let mut follow_up_action = None;
    if let Some(dialog) = &mut state.active_dialog {
        match dialog {
            ActiveDialog::Edit(edit_state) => match edit_state.render(ctx, conn) {
                EditResult::Saved => {
                    close_dialog = true;
                    state.needs_reload = true;
                    state.notification = Some(Notification::info("WAD updated".to_string()));
                }
                EditResult::Cancelled => {
                    close_dialog = true;
                }
                EditResult::Modified => {
                    close_dialog = true;
                    state.needs_reload = true;
                }
                EditResult::Open => {}
            },
            ActiveDialog::Delete(delete_state) => match delete_state.render(ctx, conn) {
                DeleteResult::Confirmed => {
                    close_dialog = true;
                    state.needs_reload = true;
                    state.notification = Some(Notification::info("WAD deleted".to_string()));
                }
                DeleteResult::Error(msg) => {
                    close_dialog = true;
                    state.notification = Some(Notification::error(msg));
                }
                DeleteResult::Cancelled => {
                    close_dialog = true;
                }
                DeleteResult::Open => {}
            },
            ActiveDialog::Sessions(sessions_state) => match sessions_state.render(ctx) {
                SessionsResult::Closed => {
                    close_dialog = true;
                }
                SessionsResult::Open => {}
            },
            ActiveDialog::Stats(stats_state) => match stats_state.render(ctx) {
                StatsResult::Closed => {
                    close_dialog = true;
                }
                StatsResult::Open => {}
            },
            ActiveDialog::Cache(cache_state) => match cache_state.render(ctx, conn) {
                CacheResult::Closed => {
                    close_dialog = true;
                }
                CacheResult::Open => {}
            },
            ActiveDialog::Settings(settings_state) => match settings_state.render(ctx) {
                SettingsResult::Saved => {
                    state.notification = Some(Notification::info("Settings saved".to_string()));
                    state.needs_reload = true;
                    close_dialog = true;
                }
                SettingsResult::Closed => {
                    close_dialog = true;
                }
                SettingsResult::Open => {}
            },
            ActiveDialog::Collections(collections_state) => {
                let modified = collections_state.modified;
                match collections_state.render(ctx, conn) {
                    CollectionsResult::Closed => {
                        if modified {
                            state.refresh_collections(conn);
                        }
                        close_dialog = true;
                    }
                    CollectionsResult::LoadQuery(query) => {
                        close_dialog = true;
                        state.refresh_collections(conn);
                        state.active_collection = None;
                        state.filter.input = query;
                        state.filter.mark_changed(std::time::Instant::now());
                    }
                    CollectionsResult::Open => {}
                }
            }
            ActiveDialog::Resources(resources_state) => match resources_state.render(ctx, conn) {
                ResourcesResult::Closed => {
                    close_dialog = true;
                }
                ResourcesResult::Open => {}
            },
            ActiveDialog::WadStats(wad_stats_state) => match wad_stats_state.render(ctx, conn) {
                WadStatsResult::Closed => {
                    close_dialog = true;
                }
                WadStatsResult::Modified => {
                    // Stay open so the user can keep managing entries; just
                    // ask the parent to refresh library data.
                    state.needs_reload = true;
                }
                WadStatsResult::Open => {}
            },
            ActiveDialog::Link(link_state) => match link_state.render(ctx, conn) {
                LinkResult::Linked(wad_id) => {
                    close_dialog = true;
                    state.needs_reload = true;
                    state.notification = Some(Notification::info(
                        "WAD file linked; launching...".to_string(),
                    ));
                    follow_up_action = Some(ActionRequest::Play(wad_id));
                }
                LinkResult::Cancelled => {
                    close_dialog = true;
                }
                LinkResult::Open => {}
            },
            ActiveDialog::CacowardLink(dialog) => match dialog.render(ctx) {
                CacowardLinkResult::Linked(pk, wad_id) => {
                    close_dialog = true;
                    if let Err(e) = caco_core::db::cacowards::link_wad(conn, pk, wad_id, true) {
                        state.notification = Some(Notification::error(format!("Link failed: {e}")));
                    } else {
                        state.cacowards.needs_reload = true;
                        state.notification =
                            Some(Notification::info("Linked to library WAD".to_string()));
                    }
                }
                CacowardLinkResult::Cancelled => {
                    close_dialog = true;
                }
                CacowardLinkResult::Open => {}
            },
            ActiveDialog::Help => {
                if render_help_dialog(ctx) {
                    close_dialog = true;
                }
            }
            ActiveDialog::About => {
                if render_about_dialog(ctx) {
                    close_dialog = true;
                }
            }
        }
    }
    if close_dialog {
        // Cache/Collections/Resources expose a `modified` flag that's only
        // meaningful when the dialog is still in scope — read it before we drop
        // the state.
        let was_modified = match &state.active_dialog {
            Some(ActiveDialog::Cache(s)) => s.modified,
            Some(ActiveDialog::Collections(s)) => s.modified,
            Some(ActiveDialog::Resources(s)) => s.modified,
            _ => false,
        };
        if was_modified {
            state.needs_reload = true;
        }
        state.active_dialog = None;
    }
    follow_up_action
}

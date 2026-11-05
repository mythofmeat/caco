//! The Cacowards view, for browsing a year's awards, how far through them you are, and
//! doing something about each one (import / get / play).
//!
//! Layout:
//! 1. A header with the year, how far through it you are, and a link to Doomworld's write-ups
//!    for it. The magazine look lives here, and it collapses to a line in a short window.
//! 2. A strip of years with how far through each you are.
//! 3. Category sections (winner / runner-up / honorable-mention / mordeth), each entry tagged
//!    with its linked WAD's status.
//! 4. Every other award the year gave, the same way, under a heading saying that they don't
//!    count towards its completion.
//!
//! Entries come as a compact list, because this is a checklist more than a gallery, and a
//! year's fifty or so nearly fit on a screen that way, where six posters did. Posters are one
//! click away, on the same toggle the library has, for looking at the pictures.
//!
//! Entries stay in the order of the page they came from, which is not a ranking. They used to
//! move around as they were completed, so that an entry was never quite where you left it.

use caco_core::db::cacowards::{CacowardRecord, EffectiveStatus};
use caco_core::db::{CORE_CATEGORIES, Status, is_core_category};
use egui::{CornerRadius, Rect, StrokeKind, Vec2};

use crate::get::GetTarget;
use crate::state::{ActionRequest, AppState, ViewLayout};
use crate::theme;

const HERO_HEIGHT: f32 = 100.0;
/// In a short window: the title and the bar, on a line.
const SHORT_HERO_HEIGHT: f32 = 44.0;
const SECTION_PAD_X: f32 = 20.0;
const SECTION_PAD_Y: f32 = 14.0;

const YEAR_CHIP_WIDTH: f32 = 66.0;
const YEAR_CHIP_HEIGHT: f32 = 36.0;
const YEAR_CHIP_GAP: f32 = 6.0;
const YEAR_STRIP_VPAD: f32 = 8.0;

/// A list column is at least this wide, so two across at the default window and one at the
/// minimum, and three on a big screen.
const LIST_COLUMN_MIN: f32 = 420.0;
const LIST_COLUMN_GAP: f32 = 16.0;
const LIST_ROW_HEIGHT: f32 = 28.0;
const LIST_ROW_PADDING: f32 = 8.0;

const CARD_MIN_WIDTH: f32 = 176.0;
const CARD_MAX_WIDTH: f32 = 260.0;
const CARD_GAP: f32 = 14.0;
const CARD_ROUNDING: u8 = 8;
/// Thumbnail height as a multiple of card width: the 5:3 poster, a touch shorter than the
/// library grid's 4:3.
const THUMB_ASPECT: f32 = 0.6;
/// Title, author and the row with the status and the action in it, and the gaps around them.
const CARD_BODY_HEIGHT: f32 = 76.0;

/// Render the Cacowards central panel.
pub fn render(
    ui: &mut egui::Ui,
    state: &mut AppState,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) -> Option<ActionRequest> {
    if state.cacowards.all_entries.is_empty() {
        return render_empty(ui);
    }
    let year = state.cacowards.selected_year?;

    let mut action: Option<ActionRequest> = None;

    render_hero(ui, state, year);

    if let Some(y) = render_year_strip(ui, state) {
        state.cacowards.choose_year(y);
    }

    // Snapshot for the chosen year so we can iterate without borrowing
    // `state.cacowards` during render.
    let year_entries: Vec<(CacowardRecord, EffectiveStatus)> = state
        .cacowards
        .all_entries
        .iter()
        .filter(|(r, _)| r.year == year)
        .cloned()
        .collect();

    let others = other_categories(&year_entries);
    let core = CORE_CATEGORIES.iter().copied();
    let categories: Vec<&str> = core.chain(others.iter().copied()).collect();

    if let Some(a) = entry_keys(ui, state, &year_entries, &categories) {
        action.get_or_insert(a);
    }

    for (index, category) in categories.iter().copied().enumerate() {
        if index == CORE_CATEGORIES.len() && !others.is_empty() {
            render_other_awards_heading(ui);
        }
        let in_section: Vec<&(CacowardRecord, EffectiveStatus)> = year_entries
            .iter()
            .filter(|(r, _)| r.category == category)
            .collect();
        if in_section.is_empty() {
            continue;
        }
        if let Some(a) = render_category_section(ui, state, category, &in_section, thumbnails) {
            action.get_or_insert(a);
        }
    }

    // Trailing space so the last section's cards aren't flush with the
    // status bar.
    ui.add_space(40.0);

    action
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// Going through a year from the keyboard, as the library goes: left and right between years,
/// up and down between entries, and Enter for whatever the entry's button would do.
fn entry_keys(
    ui: &egui::Ui,
    state: &mut AppState,
    entries: &[(CacowardRecord, EffectiveStatus)],
    categories: &[&str],
) -> Option<ActionRequest> {
    if state.has_dialog() || ui.ctx().egui_wants_keyboard_input() {
        return None;
    }
    let pressed = |key| ui.input(|i| i.key_pressed(key));

    if pressed(egui::Key::Escape) {
        state.cacowards.selected_entry_pk = None;
    }
    if pressed(egui::Key::ArrowRight) || pressed(egui::Key::L) {
        state.cacowards.step_year(1);
    }
    if pressed(egui::Key::ArrowLeft) || pressed(egui::Key::H) {
        state.cacowards.step_year(-1);
    }

    // In the order they are on the screen, leaving out what is being hidden.
    let hide_finished = state.cacowards.hide_finished;
    let in_order: Vec<&(CacowardRecord, EffectiveStatus)> = categories
        .iter()
        .flat_map(|category| entries.iter().filter(move |(r, _)| r.category == *category))
        .filter(|(record, status)| !hide_finished || !is_finished(record, *status))
        .collect();
    let place = state
        .cacowards
        .selected_entry_pk
        .and_then(|pk| in_order.iter().position(|(r, _)| r.id == pk));
    let last = in_order.len().saturating_sub(1);
    let moved = if pressed(egui::Key::ArrowDown) || pressed(egui::Key::J) {
        Some(place.map_or(0, |at| (at + 1).min(last)))
    } else if pressed(egui::Key::ArrowUp) || pressed(egui::Key::K) {
        Some(place.map_or(0, |at| at.saturating_sub(1)))
    } else {
        None
    };
    if let Some((record, _)) = moved.and_then(|at| in_order.get(at).copied()) {
        state.cacowards.selected_entry_pk = Some(record.id);
        state.cacowards.reveal_entry = true;
    }

    if !pressed(egui::Key::Enter) {
        return None;
    }
    let (record, status) = in_order.get(place?).copied()?;
    let linked = record
        .wad_id
        .and_then(|id| state.cacowards.linked_wads.get(&id));
    leading_action(record, *status, get_target(record, *status, linked))
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

fn render_empty(ui: &mut egui::Ui) -> Option<ActionRequest> {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.colored_label(
            theme::TEXT_PRIMARY,
            egui::RichText::new("No Cacoward data yet")
                .size(20.0)
                .strong(),
        );
        ui.add_space(8.0);
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "The Cacowards list could not be loaded when caco started.",
        );
    });
    None
}

// ---------------------------------------------------------------------------
// Hero banner
// ---------------------------------------------------------------------------

fn render_hero(ui: &mut egui::Ui, state: &AppState, year: i64) {
    let short = theme::is_short_window(ui.ctx());
    let height = if short {
        SHORT_HERO_HEIGHT
    } else {
        HERO_HEIGHT
    };
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, theme::BG_MEDIUM);

    // Bottom rule
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y - 0.5),
            egui::pos2(rect.max.x, rect.max.y - 0.5),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER_MED),
    );

    render_page_pill(
        ui,
        rect,
        state.cacowards.pages.get(&year).map(String::as_str),
    );

    let left = rect.min.x + SECTION_PAD_X;
    let (total, done) = state.cacowards.year_summary(year);
    let progress = if total > 0 {
        format!(
            "{done} of {total} completed ({:.0}%)",
            done as f32 * 100.0 / total as f32
        )
    } else {
        "Nothing here counts towards the year".to_string()
    };

    let title_font = egui::FontId::proportional(if short { 20.0 } else { 28.0 });
    let mut cursor_y = rect.min.y + 8.0;
    if !short {
        painter.text(
            egui::pos2(left, cursor_y),
            egui::Align2::LEFT_TOP,
            hero_kicker(&state.cacowards.all_entries, year),
            egui::FontId::proportional(11.0),
            theme::GOLD,
        );
        cursor_y += 15.0;
    }

    // "Cacowards YEAR"
    let prefix = painter.layout_no_wrap(
        "Cacowards ".to_string(),
        title_font.clone(),
        theme::TEXT_PRIMARY,
    );
    let prefix_width = prefix.size().x;
    painter.galley(egui::pos2(left, cursor_y), prefix, theme::TEXT_PRIMARY);
    let year_right = painter.text(
        egui::pos2(left + prefix_width, cursor_y),
        egui::Align2::LEFT_TOP,
        year.to_string(),
        title_font,
        theme::TEXT_ACCENT,
    );

    if short {
        painter.text(
            egui::pos2(year_right.max.x + 12.0, year_right.center().y),
            egui::Align2::LEFT_CENTER,
            progress,
            egui::FontId::proportional(12.0),
            theme::TEXT_SECONDARY,
        );
    } else {
        cursor_y += 35.0;
        painter.text(
            egui::pos2(left, cursor_y),
            egui::Align2::LEFT_TOP,
            format!("{progress} \u{00b7} Doomworld's annual selection of the year's best WADs"),
            egui::FontId::proportional(12.0),
            theme::TEXT_SECONDARY,
        );
    }

    // How far through the year, along the bottom. What fills it is completed entries, so it
    // is the colour of Completed.
    let bar = Rect::from_min_max(
        egui::pos2(left, rect.max.y - 7.0),
        egui::pos2(rect.max.x - SECTION_PAD_X, rect.max.y - 3.0),
    );
    painter.rect_filled(bar, CornerRadius::same(2), theme::BG_LIGHT);
    if total > 0 {
        let fill = Rect::from_min_size(
            bar.min,
            Vec2::new(bar.width() * (done as f32 / total as f32), bar.height()),
        );
        painter.rect_filled(
            fill,
            CornerRadius::same(2),
            theme::status_color(Status::Completed),
        );
    }
}

/// The year's Doomworld page, top-right of the hero. The write-ups are Doomworld's
/// own text, which is why caco links to them rather than carrying them.
fn render_page_pill(ui: &mut egui::Ui, hero: Rect, page: Option<&str>) {
    let painter = ui.painter_at(hero);
    let Some(page) = page else {
        return;
    };
    let text = "Write-ups on Doomworld \u{2197}";
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), theme::TEXT_SECONDARY);
    let pad = Vec2::new(10.0, 4.0);
    let size = galley.size() + pad * 2.0;
    // On the title's row, whose right end is free at any window size. The kicker
    // above it reaches the pill's edge at the minimum width.
    let short = theme::is_short_window(ui.ctx());
    let pill = Rect::from_min_size(
        egui::pos2(
            hero.max.x - SECTION_PAD_X - size.x,
            hero.min.y + if short { 10.0 } else { 26.0 },
        ),
        size,
    );

    let mut color = theme::TEXT_SECONDARY;
    let response = ui
        .interact(
            pill,
            ui.scope_id().with("cacoward-page"),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(page);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Link, true, "Write-ups on Doomworld")
    });
    if response.hovered() {
        color = theme::TEXT_ACCENT;
    }
    if response.clicked() {
        // Nothing to do about a machine with no browser to hand this to.
        let _ = open::that(page);
    }

    painter.rect(
        pill,
        CornerRadius::same(11),
        theme::BG_DARK,
        egui::Stroke::new(1.0_f32, theme::BORDER_MED),
        StrokeKind::Inside,
    );
    painter.text(pill.min + pad, egui::Align2::LEFT_TOP, text, font, color);
}

/// "12 winners · 2 runners-up", naming only what the year gave: most of them had no honorable
/// mentions at all.
fn hero_kicker(entries: &[(CacowardRecord, EffectiveStatus)], year: i64) -> String {
    let mut parts = Vec::new();
    for (category, one, many) in [
        ("winner", "winner", "winners"),
        ("runner-up", "runner-up", "runners-up"),
        (
            "honorable-mention",
            "honorable mention",
            "honorable mentions",
        ),
    ] {
        let count = entries
            .iter()
            .filter(|(r, _)| r.year == year && r.category == category)
            .count();
        match count {
            0 => {}
            1 => parts.push(format!("1 {one}")),
            n => parts.push(format!("{n} {many}")),
        }
    }
    parts.join(" · ")
}

// ---------------------------------------------------------------------------
// Year strip
// ---------------------------------------------------------------------------

fn render_year_strip(ui: &mut egui::Ui, state: &mut AppState) -> Option<i64> {
    let years = state.cacowards.years();
    let selected = state.cacowards.selected_year;
    let reveal = std::mem::take(&mut state.cacowards.reveal_year);
    let state = &*state;
    let strip_height = YEAR_CHIP_HEIGHT + YEAR_STRIP_VPAD * 2.0;

    // Background strip the full width of the panel.
    let (strip_rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), strip_height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(strip_rect, 0, theme::BG_MEDIUM);
    ui.painter().line_segment(
        [
            egui::pos2(strip_rect.min.x, strip_rect.max.y - 0.5),
            egui::pos2(strip_rect.max.x, strip_rect.max.y - 0.5),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );

    // The years scroll, because no window is wide enough to be sure of them all:
    // twenty-odd chips is 1800pt, against 620pt or so at the minimum window size.
    let mut strip_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip_rect.shrink2(Vec2::new(SECTION_PAD_X, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    // Solid rather than the floating bar that only shows on hover: the strip can
    // end exactly on a chip's edge, which looks like the end of the years.
    theme::solid_scrollbar(&mut strip_ui);
    let mut clicked: Option<i64> = None;
    egui::ScrollArea::horizontal()
        .id_salt("cacoward-years")
        .auto_shrink([false, false])
        .show(&mut strip_ui, |ui| {
            ui.spacing_mut().item_spacing.x = YEAR_CHIP_GAP;
            ui.horizontal_centered(|ui| {
                for year in years {
                    let (chip_rect, response) = ui.allocate_exact_size(
                        Vec2::new(YEAR_CHIP_WIDTH, YEAR_CHIP_HEIGHT),
                        egui::Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, year.to_string())
                    });
                    draw_year_chip(
                        ui,
                        chip_rect,
                        year,
                        state,
                        selected == Some(year),
                        response.hovered(),
                    );
                    if response.clicked() {
                        clicked = Some(year);
                        // A chip half out of view comes the rest of the way.
                        response.scroll_to_me(None);
                    }
                    if reveal && selected == Some(year) {
                        response.scroll_to_me(None);
                    }
                }
            });
        });
    clicked
}

fn draw_year_chip(
    ui: &egui::Ui,
    rect: Rect,
    year: i64,
    state: &AppState,
    active: bool,
    hovered: bool,
) {
    let painter = ui.painter_at(rect);

    let (bg, fg, sub) = if active {
        (
            theme::BG_SELECTED,
            theme::TEXT_ACCENT,
            theme::TEXT_SECONDARY,
        )
    } else if hovered {
        (theme::BG_LIGHT, theme::TEXT_PRIMARY, theme::TEXT_MUTED)
    } else {
        (theme::BG_DARK, theme::TEXT_SECONDARY, theme::TEXT_MUTED)
    };

    painter.rect(
        rect,
        CornerRadius::same(4),
        bg,
        egui::Stroke::new(if active { 1.0_f32 } else { 0.0_f32 }, theme::TEXT_ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center() + Vec2::new(0.0, -7.0),
        egui::Align2::CENTER_CENTER,
        year.to_string(),
        egui::FontId::proportional(13.0),
        fg,
    );
    let (total, done) = state.cacowards.year_summary(year);
    painter.text(
        rect.center() + Vec2::new(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        format!("{done}/{total}"),
        egui::FontId::proportional(11.0),
        sub,
    );
}

// ---------------------------------------------------------------------------
// Category sections
// ---------------------------------------------------------------------------

/// The year's categories outside the core four, in the order the query gave them.
fn other_categories(entries: &[(CacowardRecord, EffectiveStatus)]) -> Vec<&str> {
    let mut others: Vec<&str> = Vec::new();
    for (record, _) in entries {
        let category = record.category.as_str();
        if !is_core_category(category) && !others.contains(&category) {
            others.push(category);
        }
    }
    others
}

/// What separates the awards a year is finished by from the rest of what it gave.
fn render_other_awards_heading(ui: &mut egui::Ui) {
    ui.add_space(SECTION_PAD_Y);
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(SECTION_PAD_X as i8, 0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Other awards")
                    .size(18.0)
                    .color(theme::TEXT_PRIMARY),
            );
            ui.colored_label(
                theme::TEXT_SECONDARY,
                "These don't count towards the year's total.",
            );
        });
}

fn render_category_section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    category: &str,
    entries: &[&(CacowardRecord, EffectiveStatus)],
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) -> Option<ActionRequest> {
    // Completion math excludes unsupported entries so the meter / swept
    // chip reflect "of the playable entries". Unsupported entries are
    // still rendered (just visually distinct) so the user can see them
    // and re-mark them when caco grows the needed IWAD / sourceport
    // support.
    let total = entries.iter().filter(|(r, _)| r.supported).count();
    let done = entries
        .iter()
        .filter(|(r, s)| r.supported && matches!(s, EffectiveStatus::Library(Status::Completed)))
        .count();
    let absent = entries
        .iter()
        .filter(|(r, s)| r.supported && matches!(s, EffectiveStatus::Absent))
        .count();
    let unsupported = entries.iter().filter(|(r, _)| !r.supported).count();

    let mut action: Option<ActionRequest> = None;

    // Top padding
    ui.add_space(SECTION_PAD_Y);

    let swept = done == total && total > 0;

    // Section header: all on one line, since it is on the screen once per section.
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(SECTION_PAD_X as i8, 0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(category_label(category))
                        .size(12.0)
                        .strong()
                        .color(theme::GOLD)
                        .extra_letter_spacing(2.0),
                );
                ui.add_space(6.0);
                draw_meter(ui, done, total, 100.0);
                ui.add_space(4.0);
                let mut bits = vec![format!("{done} of {total} completed")];
                if absent > 0 {
                    bits.push(format!("{absent} not in library"));
                }
                if unsupported > 0 {
                    bits.push(format!("{unsupported} unsupported"));
                }
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    egui::RichText::new(bits.join(" \u{00b7} ")).size(12.0),
                );
                if swept {
                    ui.add_space(4.0);
                    paint_swept_chip(ui);
                }
            });
        });

    ui.add_space(6.0);

    let layout = state.cacowards.layout;
    let hide_finished = state.cacowards.hide_finished;
    let showing: Vec<&(CacowardRecord, EffectiveStatus)> = entries
        .iter()
        .copied()
        .filter(|(record, status)| !hide_finished || !is_finished(record, *status))
        .collect();
    let body = match layout {
        ViewLayout::List => render_rows(ui, state, &showing),
        ViewLayout::Grid => render_cards(ui, state, &showing, thumbnails),
    };
    if let Some(a) = body {
        action.get_or_insert(a);
    }

    action
}

/// Whether an entry is done with, for hiding: played to the end, or out of the reckoning.
fn is_finished(record: &CacowardRecord, status: EffectiveStatus) -> bool {
    !record.supported || matches!(status, EffectiveStatus::Library(Status::Completed))
}

/// Entries as a list, as many columns across as fit.
fn render_rows(
    ui: &mut egui::Ui,
    state: &mut AppState,
    entries: &[&(CacowardRecord, EffectiveStatus)],
) -> Option<ActionRequest> {
    let mut action = None;
    let available = ui.available_width() - SECTION_PAD_X * 2.0;
    let columns = ((available + LIST_COLUMN_GAP) / (LIST_COLUMN_MIN + LIST_COLUMN_GAP))
        .floor()
        .max(1.0) as usize;
    let width = (available - (columns - 1) as f32 * LIST_COLUMN_GAP) / columns as f32;

    ui.spacing_mut().item_spacing.y = 0.0;
    for row in entries.chunks(columns) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(SECTION_PAD_X as i8, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = LIST_COLUMN_GAP;
                    for (record, status) in row.iter().copied() {
                        if let Some(a) = render_row(ui, state, record, *status, width) {
                            action.get_or_insert(a);
                        }
                    }
                });
            });
    }
    action
}

/// One entry on one line: title, author, status, and what to do about it.
fn render_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    record: &CacowardRecord,
    status: EffectiveStatus,
    width: f32,
) -> Option<ActionRequest> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, LIST_ROW_HEIGHT), egui::Sense::click());
    let style = theme::cacoward_style(status, record.supported);
    let author = record.wad_author.as_deref().unwrap_or("");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{}, {author}, {}", record.wad_title, style.word),
        )
    });

    let selected = state.cacowards.selected_entry_pk == Some(record.id);
    if response.clicked() {
        state.cacowards.selected_entry_pk = Some(record.id);
    }
    if selected && std::mem::take(&mut state.cacowards.reveal_entry) {
        response.scroll_to_me(None);
    }
    let linked = record
        .wad_id
        .and_then(|id| state.cacowards.linked_wads.get(&id));
    let get = get_target(record, status, linked);
    let menu_action = entry_menu(&response, record, get);
    let double_click_action = if response.double_clicked() {
        leading_action(record, status, get)
    } else {
        None
    };

    let painter = ui.painter_at(rect);
    let pointed_at = response.contains_pointer();
    if selected {
        painter.rect_filled(rect, 4, theme::BG_SELECTED);
    } else if pointed_at {
        painter.rect_filled(rect, 4, theme::BG_MEDIUM);
    }

    // From the right, so that the words get whatever that leaves.
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(LIST_ROW_PADDING, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let waiting = get.is_some_and(|target| state.gets.is_waiting_for(target));
    let importing = state.cacowards.importing.contains(&record.id);
    let button_action = action_button(
        &mut right,
        record,
        status,
        get,
        ButtonState {
            waiting,
            importing,
            leading: selected || pointed_at,
        },
    );
    // Only for what is in the library or out of the reckoning: the rest is the button beside it
    // and the count over the section, and fifty pills saying "Not in Library" was most of what
    // a year looked like before any of it had been played.
    if has_status_to_show(status, record.supported) {
        theme::status_pill(&mut right, style, "");
    }

    let left = rect.min.x + LIST_ROW_PADDING;
    let room = (right.min_rect().min.x - LIST_ROW_PADDING - left).max(0.0);
    let (title_color, author_color) = entry_text_colors(record);
    let title_font = egui::FontId::proportional(13.0);
    let author_font = egui::FontId::proportional(11.0);
    let title_width = painter
        .layout_no_wrap(record.wad_title.clone(), title_font.clone(), title_color)
        .size()
        .x;
    let author_width = painter
        .layout_no_wrap(author.to_string(), author_font.clone(), author_color)
        .size()
        .x;
    let (title_room, author_room) = share_row(room, title_width, author_width);
    let y = rect.center().y;
    theme::paint_truncated(
        &painter,
        egui::pos2(left, y - 8.0),
        &record.wad_title,
        title_font,
        title_color,
        title_room,
    );
    if author_room > 0.0 {
        theme::paint_truncated(
            &painter,
            egui::pos2(left + title_room + ROW_TEXT_GAP, y - 6.5),
            author,
            author_font,
            author_color,
            author_room,
        );
    }
    response.on_hover_text(format!("{}\n{author}", record.wad_title));

    menu_action.or(button_action).or(double_click_action)
}

/// Whether an entry has a status worth a pill.
fn has_status_to_show(status: EffectiveStatus, supported: bool) -> bool {
    !supported || !matches!(status, EffectiveStatus::Absent)
}

const ROW_TEXT_GAP: f32 = 8.0;

/// How to share a row's width between a title and an author who may not both fit. As much
/// as each needs if they do, and otherwise the title has most of it.
fn share_row(room: f32, title: f32, author: f32) -> (f32, f32) {
    let room_for_both = room - ROW_TEXT_GAP;
    if title + author <= room_for_both {
        return (title, author);
    }
    let title_room = title.min((room_for_both - author).max(room_for_both * 0.62));
    (title_room, (room_for_both - title_room).max(0.0))
}

/// Unsupported entries are out of the game, so they recede. Nothing else does.
fn entry_text_colors(record: &CacowardRecord) -> (egui::Color32, egui::Color32) {
    if record.supported {
        (theme::TEXT_PRIMARY, theme::TEXT_SECONDARY)
    } else {
        (theme::TEXT_MUTED, theme::TEXT_MUTED)
    }
}

/// Entries as posters.
fn render_cards(
    ui: &mut egui::Ui,
    state: &mut AppState,
    entries: &[&(CacowardRecord, EffectiveStatus)],
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) -> Option<ActionRequest> {
    let mut action = None;
    // Manual layout to keep cards at a fixed width.
    let available = ui.available_width() - SECTION_PAD_X * 2.0;
    let card_w = card_width(available);
    let card_h = card_height(card_w);
    let columns = ((available + CARD_GAP) / (card_w + CARD_GAP))
        .floor()
        .max(1.0) as usize;

    let rows = entries.len().div_ceil(columns);
    for row in 0..rows {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(SECTION_PAD_X as i8, 0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = CARD_GAP;
                    for col in 0..columns {
                        let idx = row * columns + col;
                        if idx >= entries.len() {
                            break;
                        }
                        let (record, status) = entries[idx];
                        if let Some(a) =
                            render_card(ui, state, record, *status, card_w, card_h, thumbnails)
                        {
                            action.get_or_insert(a);
                        }
                    }
                });
            });
        if row + 1 < rows {
            ui.add_space(CARD_GAP);
        }
    }
    action
}

fn card_width(available: f32) -> f32 {
    let cols = ((available + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP))
        .floor()
        .max(1.0);
    let w = (available - (cols - 1.0) * CARD_GAP) / cols;
    w.clamp(CARD_MIN_WIDTH, CARD_MAX_WIDTH)
}

fn card_height(card_w: f32) -> f32 {
    (card_w * THUMB_ASPECT).round() + CARD_BODY_HEIGHT
}

// ---------------------------------------------------------------------------
// Single card — manual rect allocation + painter, no nested Frame.
// ---------------------------------------------------------------------------

fn render_card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    record: &CacowardRecord,
    status: EffectiveStatus,
    width: f32,
    height: f32,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) -> Option<ActionRequest> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let rounding = CornerRadius::same(CARD_ROUNDING);

    // Selection click on the card surface (the action button has its own
    // click handler, but Button is rendered on top so its events take
    // precedence — clicking elsewhere selects the card).
    let selected = state.cacowards.selected_entry_pk == Some(record.id);
    if response.clicked() {
        state.cacowards.selected_entry_pk = Some(record.id);
    }
    if selected && std::mem::take(&mut state.cacowards.reveal_entry) {
        response.scroll_to_me(None);
    }

    let linked = record
        .wad_id
        .and_then(|id| state.cacowards.linked_wads.get(&id));
    let get = get_target(record, status, linked);
    let menu_action = entry_menu(&response, record, get);
    let double_click_action = if response.double_clicked() {
        leading_action(record, status, get)
    } else {
        None
    };
    let pointed_at = response.contains_pointer();

    // Background — slightly lifted when selected.
    let bg = if selected {
        theme::BG_LIGHT
    } else {
        theme::BG_MEDIUM
    };
    painter.rect_filled(rect, rounding, bg);

    // ── Thumbnail ──────────────────────────────────────────────────────
    let thumb_h = (width * THUMB_ASPECT).round();
    let thumb_rect = Rect::from_min_size(rect.min, Vec2::new(width, thumb_h));
    let thumb_rounding = CornerRadius {
        nw: CARD_ROUNDING,
        ne: CARD_ROUNDING,
        sw: 0,
        se: 0,
    };
    paint_card_thumbnail(&painter, thumb_rect, thumb_rounding, record, thumbnails);

    // What is out of the game recedes. What has been finished does not: it used to be dimmed
    // too, which made the work done the hardest thing on the page to read.
    if !record.supported {
        painter.rect_filled(thumb_rect, thumb_rounding, theme::SCRIM);
    }

    // ── Body ──────────────────────────────────────────────────────────
    // (No status-colored left edge — the pill on the thumbnail conveys
    // the same info, and a square-cornered bar peeking past the card's
    // rounded border looked broken.)

    let pad_x = 10.0;
    let body = Rect::from_min_max(
        egui::pos2(rect.min.x + pad_x, thumb_rect.max.y + 8.0),
        egui::pos2(rect.max.x - pad_x, rect.max.y - 8.0),
    );

    let (title_color, author_color) = entry_text_colors(record);
    theme::paint_truncated(
        &painter,
        body.min,
        &record.wad_title,
        egui::FontId::proportional(14.0),
        title_color,
        body.width(),
    );
    if let Some(author) = record.wad_author.as_deref() {
        theme::paint_truncated(
            &painter,
            egui::pos2(body.min.x, body.min.y + 19.0),
            author,
            egui::FontId::proportional(11.0),
            author_color,
            body.width(),
        );
    }
    response.clone().on_hover_text(format!(
        "{}\n{}",
        record.wad_title,
        record.wad_author.as_deref().unwrap_or("")
    ));
    let style = theme::cacoward_style(status, record.supported);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{}, {}", record.wad_title, style.word),
        )
    });

    let button_rect = Rect::from_min_max(
        egui::pos2(body.min.x, body.max.y - 22.0),
        egui::pos2(body.max.x, body.max.y),
    );
    // On the picture rather than beside the buttons, since a card can be too narrow for an
    // entry's status and both of the things to do about it.
    if has_status_to_show(status, record.supported) {
        theme::paint_status_pill(
            &painter,
            thumb_rect.min.x + 8.0,
            thumb_rect.min.y + 16.0,
            style,
            "",
        );
    }
    let mut sub_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(button_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let waiting = get.is_some_and(|target| state.gets.is_waiting_for(target));
    let importing = state.cacowards.importing.contains(&record.id);
    let button_action = action_button(
        &mut sub_ui,
        record,
        status,
        get,
        ButtonState {
            waiting,
            importing,
            leading: selected || pointed_at,
        },
    );

    // Border painted INSIDE the rect — `StrokeKind::Outside` was
    // effectively halving the visible width because the row's
    // horizontal layout clipped to its allocated space, eating the
    // outer pixels of the ring.
    if selected {
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(2.0_f32, theme::TEXT_ACCENT),
            StrokeKind::Inside,
        );
    } else if pointed_at {
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.0_f32, theme::CONTROL_BORDER),
            StrokeKind::Inside,
        );
    }

    // Context-menu picks take precedence — they're the user's
    // most recent gesture and shouldn't get clobbered by a stray
    // button-region click.
    menu_action.or(button_action).or(double_click_action)
}

fn paint_card_thumbnail(
    painter: &egui::Painter,
    thumb_rect: Rect,
    thumb_rounding: CornerRadius,
    record: &CacowardRecord,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) {
    // A linked WAD's own picture, or for an entry not in the library, the one its wiki
    // page has, which the app asks for under a key of its own.
    let key = record
        .wad_id
        .unwrap_or_else(|| thumb_key_for_absent(record.id));
    let picture = thumbnails.and_then(|tm| tm.get(key));
    theme::paint_thumbnail(painter, thumb_rect, thumb_rounding, picture);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// What an entry offers on a right click: getting it, linking it by hand for the cases the
/// automatic link can't reach (renamed titles and so on), and whether it counts.
fn entry_menu(
    response: &egui::Response,
    record: &CacowardRecord,
    get: Option<GetTarget>,
) -> Option<ActionRequest> {
    let mut action = None;
    response.context_menu(|ui| {
        if let Some(target) = get
            && ui.button("Get in Browser").clicked()
        {
            action = Some(ActionRequest::Get(target));
            ui.close();
        }
        if let Some(wad_id) = record.wad_id
            && ui.button("Show in Library").clicked()
        {
            action = Some(ActionRequest::ShowWad(wad_id));
            ui.close();
        }
        if ui.button("Link to a WAD in Your Library\u{2026}").clicked() {
            action = Some(ActionRequest::LinkCacoward(record.id));
            ui.close();
        }
        if record.wad_id.is_some() && ui.button("Unlink").clicked() {
            action = Some(ActionRequest::UnlinkCacoward(record.id));
            ui.close();
        }
        ui.separator();
        let toggle_label = if record.supported {
            "Mark as Unsupported"
        } else {
            "Mark as Supported"
        };
        if ui.button(toggle_label).clicked() {
            action = Some(ActionRequest::SetCacowardSupported(
                record.id,
                !record.supported,
            ));
            ui.close();
        }
    });
    action
}

/// A section's heading. Categories are free text, so one this has never heard of
/// still gets a heading, spelled from its name.
fn category_label(category: &str) -> String {
    match category {
        "winner" => "WINNERS".to_string(),
        "runner-up" => "RUNNERS-UP".to_string(),
        "honorable-mention" => "HONORABLE MENTIONS".to_string(),
        "mordeth" => "MORDETH AWARD".to_string(),
        "special-feature" => "SPECIAL FEATURES".to_string(),
        "multiplayer" => "MULTIPLAYER AWARDS".to_string(),
        "gameplay-mod" => "GAMEPLAY MOD AWARDS".to_string(),
        "newcomer" => "HONORING A NEWCOMER".to_string(),
        "adrians-pen" => "ADRIAN'S PEN".to_string(),
        other => other.replace('-', " ").to_uppercase(),
    }
}

/// What one award in a category is called, for saying what a WAD won. Spelled from the name
/// for a category this has never heard of, as a heading is.
pub fn award_label(category: &str) -> String {
    match category {
        "winner" => "Winner".to_string(),
        "runner-up" => "Runner-up".to_string(),
        "honorable-mention" => "Honorable Mention".to_string(),
        "mordeth" => "Mordeth Award".to_string(),
        "adrians-pen" => "Adrian's Pen".to_string(),
        other => {
            let spaced = other.replace('-', " ");
            let mut letters = spaced.chars();
            match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect(),
                None => String::new(),
            }
        }
    }
}

/// What a card can get through the browser, if anything: an entry with no WAD yet, or
/// a WAD with no file that caco cannot fetch.
fn get_target(
    record: &CacowardRecord,
    status: EffectiveStatus,
    linked: Option<&caco_core::db::WadRecord>,
) -> Option<GetTarget> {
    match status {
        EffectiveStatus::Absent => Some(GetTarget::Cacoward(record.id)),
        EffectiveStatus::Library(_) => record
            .wad_id
            .filter(|_| linked.is_some_and(|wad| wad.is_lost()))
            .map(GetTarget::Wad),
    }
}

/// What an entry offers on its face. The rest is in its menu.
#[derive(Debug, PartialEq, Eq)]
enum CardButtons {
    Import,
    /// Get leads and Import sits beside it.
    GetOrImport(GetTarget),
    Get(GetTarget),
    Play(i64),
    None,
}

/// The one thing a card's face is for: what would get this entry closer to being played.
///
/// Import leads for an entry on idgames, which caco can fetch by itself. For one that is
/// not, the file is what's missing and only a browser can get it, so Get leads, with
/// Import beside it for the metadata on a day the forum lets caco in.
///
/// A WAD the user has finished with keeps Play, whether or not its file is here. A
/// missing file only needs doing something about if it is going to be played, and an
/// accent-coloured button on every card that was cleaned up after would read as a list
/// of things left to do.
fn card_buttons(
    record: &CacowardRecord,
    status: EffectiveStatus,
    get: Option<GetTarget>,
) -> CardButtons {
    match (status, record.wad_id, get) {
        (EffectiveStatus::Absent, _, _) if record.idgames_id.is_some() => CardButtons::Import,
        (EffectiveStatus::Absent, _, Some(target)) => CardButtons::GetOrImport(target),
        (EffectiveStatus::Absent, _, None) => CardButtons::Import,
        (EffectiveStatus::Library(_), None, _) => CardButtons::None,
        (EffectiveStatus::Library(status), Some(wad_id), get) => {
            let still_playing = matches!(status, Status::InProgress | Status::Unplayed);
            match get {
                Some(target) if still_playing => CardButtons::Get(target),
                _ => CardButtons::Play(wad_id),
            }
        }
    }
}

/// How an entry's button should look right now.
#[derive(Clone, Copy)]
struct ButtonState {
    /// The browser is out fetching this one.
    waiting: bool,
    /// An import of it is under way.
    importing: bool,
    /// The pointer or the keyboard is on this entry, so this is the one thing on the page to
    /// do next. A year nothing has been imported from has fifty of these, and every one of them
    /// filled in was a wall of orange in which nothing stood out.
    leading: bool,
}

/// What a double click or Enter does: what the button most in view says.
fn leading_action(
    record: &CacowardRecord,
    status: EffectiveStatus,
    get: Option<GetTarget>,
) -> Option<ActionRequest> {
    match card_buttons(record, status, get) {
        CardButtons::Import => Some(ActionRequest::ImportCacoward(record.id)),
        CardButtons::GetOrImport(target) | CardButtons::Get(target) => {
            Some(ActionRequest::Get(target))
        }
        CardButtons::Play(wad_id) => Some(ActionRequest::Play(wad_id)),
        CardButtons::None => None,
    }
}

fn action_button(
    ui: &mut egui::Ui,
    record: &CacowardRecord,
    status: EffectiveStatus,
    get: Option<GetTarget>,
    state: ButtonState,
) -> Option<ActionRequest> {
    if state.importing {
        ui.add(egui::Spinner::new().size(14.0))
            .on_hover_text("Importing\u{2026}");
        return None;
    }
    let get_label = if state.waiting {
        "Waiting\u{2026}"
    } else {
        "Get"
    };
    let lead = state.leading;
    let mut action = None;
    match card_buttons(record, status, get) {
        CardButtons::Import => {
            if leading_button(ui, "Import", lead).clicked() {
                action = Some(ActionRequest::ImportCacoward(record.id));
            }
        }
        CardButtons::GetOrImport(target) => {
            if get_button(ui, get_label, lead).clicked() {
                action = Some(ActionRequest::Get(target));
            }
            if secondary_button(ui, "Import").clicked() {
                action = Some(ActionRequest::ImportCacoward(record.id));
            }
        }
        CardButtons::Get(target) => {
            if get_button(ui, get_label, lead).clicked() {
                action = Some(ActionRequest::Get(target));
            }
        }
        CardButtons::Play(wad_id) => {
            if secondary_button(ui, "Play").clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }
        }
        CardButtons::None => {}
    }
    action
}

/// [`leading_button`], saying what it does, since "Get" alone does not say how.
fn get_button(ui: &mut egui::Ui, label: &str, lead: bool) -> egui::Response {
    leading_button(ui, label, lead).on_hover_text(
        "Opens the page for it in your browser, and watches your Downloads folder for the file.",
    )
}

/// The button that matters on an entry, filled in only on the one being looked at.
fn leading_button(ui: &mut egui::Ui, label: &str, lead: bool) -> egui::Response {
    if !lead {
        return secondary_button(ui, label);
    }
    ui.add(
        theme::small_primary_button(label)
            .corner_radius(4.0)
            .min_size(Vec2::new(0.0, 22.0)),
    )
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(11.0)
                .color(theme::TEXT_PRIMARY),
        )
        .fill(theme::CONTROL_FILL)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED))
        .corner_radius(4.0)
        .min_size(Vec2::new(0.0, 22.0)),
    )
}

fn draw_meter(ui: &mut egui::Ui, done: usize, total: usize, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(3), theme::BG_LIGHT);
    if total > 0 {
        let fill_w = rect.width() * (done as f32 / total as f32);
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(
            fill_rect,
            CornerRadius::same(3),
            theme::status_color(Status::Completed),
        );
    }
}

/// Small gold "✓ SWEPT" chip — shown on category headers when every
/// entry in the section is `Library(Completed)`. The point is reward
/// surface: a single Mordeth Award sweep is a one-clickaway badge of
/// honour, a 12/12 Winners sweep is a year-long victory.
fn paint_swept_chip(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::blend(theme::BG_DARK, theme::GOLD, 0.16))
        .corner_radius(11.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("✓ SWEPT")
                    .color(theme::GOLD)
                    .size(11.0)
                    .strong()
                    .extra_letter_spacing(1.0),
            );
        });
}

/// Map a cacoward entry pk to a synthetic ThumbnailManager key for absent
/// entries. Wad ids are always positive SQLite primary keys, so negating
/// the cacoward pk gives a stable, collision-free namespace we can share
/// the existing thumbnail cache + worker pool with.
pub fn thumb_key_for_absent(cacoward_pk: i64) -> i64 {
    -cacoward_pk
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(year: i64, category: &str) -> (CacowardRecord, EffectiveStatus) {
        let record = CacowardRecord {
            id: 1,
            key: String::new(),
            year,
            category: category.to_string(),
            page_order: 1,
            wad_title: String::new(),
            wad_author: None,
            idgames_id: None,
            doomworld_id: None,
            doomwiki_page: None,
            sha256: None,
            wad_id: None,
            manual_override: false,
            supported_default: true,
            supported_override: None,
            supported: true,
        };
        (record, EffectiveStatus::Absent)
    }

    #[test]
    fn an_absent_entry_can_be_got_and_so_can_a_wad_with_no_way_back() {
        let conn = caco_core::db::open_memory().unwrap();
        caco_core::db::init_db(&conn).unwrap();
        let (mut record, _) = entry(2025, "winner");
        record.id = 7;
        assert_eq!(
            get_target(&record, EffectiveStatus::Absent, None),
            Some(GetTarget::Cacoward(7))
        );

        // A manual WAD with no file: the row exists, and only the browser can fill it.
        let id = caco_core::db::add_wad(
            &conn,
            &caco_core::db::NewWad::new("Road", caco_core::db::SourceType::Doomworld),
        )
        .unwrap();
        record.wad_id = Some(id);
        let library = EffectiveStatus::Library(Status::Unplayed);
        let lost = caco_core::db::get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(
            get_target(&record, library, Some(&lost)),
            Some(GetTarget::Wad(id))
        );

        // With a file it is just something to play.
        let update = caco_core::db::WadUpdate::new()
            .set_text("cached_path", Some("/keep/road.zip".to_string()));
        caco_core::db::update_wad(&conn, id, &update).unwrap();
        let here = caco_core::db::get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(get_target(&record, library, Some(&here)), None);

        // On idgames, it comes by itself when played.
        let update = caco_core::db::WadUpdate::new()
            .set_text("cached_path", None)
            .set_text("idgames_id", Some("42".to_string()));
        caco_core::db::update_wad(&conn, id, &update).unwrap();
        let fetchable = caco_core::db::get_wad(&conn, id, false).unwrap().unwrap();
        assert_eq!(get_target(&record, library, Some(&fetchable)), None);
    }

    #[test]
    fn a_card_leads_with_what_gets_it_closer_to_being_played() {
        let (mut record, _) = entry(2025, "winner");
        record.id = 7;
        let cacoward = GetTarget::Cacoward(7);
        let absent = EffectiveStatus::Absent;

        // Not on idgames: only a browser can get it.
        assert_eq!(
            card_buttons(&record, absent, Some(cacoward)),
            CardButtons::GetOrImport(cacoward)
        );
        // On idgames: caco can fetch it by itself, and the menu still has Get for when
        // idgames is behind a challenge too.
        record.idgames_id = Some(42);
        assert_eq!(
            card_buttons(&record, absent, Some(cacoward)),
            CardButtons::Import
        );

        record.wad_id = Some(3);
        let wad = GetTarget::Wad(3);
        let library = |status| EffectiveStatus::Library(status);

        // In the library, and being played, with a file or a way to one.
        for status in [Status::Unplayed, Status::InProgress] {
            assert_eq!(
                card_buttons(&record, library(status), None),
                CardButtons::Play(3)
            );
            assert_eq!(
                card_buttons(&record, library(status), Some(wad)),
                CardButtons::Get(wad),
                "no file and no way back is what stands between it and being played"
            );
        }

        // Finished with: a missing file is not something left to do.
        for status in [Status::Completed, Status::Abandoned] {
            assert_eq!(
                card_buttons(&record, library(status), None),
                CardButtons::Play(3)
            );
            assert_eq!(
                card_buttons(&record, library(status), Some(wad)),
                CardButtons::Play(3)
            );
        }
    }

    /// Enter and a double click do what the button most in view says, so nobody has to learn
    /// anything to use them.
    #[test]
    fn the_key_does_what_the_leading_button_says() {
        let (mut record, _) = entry(2025, "winner");
        record.id = 7;
        let absent = EffectiveStatus::Absent;
        assert!(matches!(
            leading_action(&record, absent, Some(GetTarget::Cacoward(7))),
            Some(ActionRequest::Get(GetTarget::Cacoward(7)))
        ));

        record.idgames_id = Some(42);
        assert!(matches!(
            leading_action(&record, absent, None),
            Some(ActionRequest::ImportCacoward(7))
        ));

        record.wad_id = Some(3);
        let completed = EffectiveStatus::Library(Status::Completed);
        assert!(matches!(
            leading_action(&record, completed, None),
            Some(ActionRequest::Play(3))
        ));
    }

    /// The button says it, and the count over the section does.
    #[test]
    fn only_what_is_in_the_library_or_out_of_the_reckoning_has_a_status_pill() {
        assert!(!has_status_to_show(EffectiveStatus::Absent, true));
        assert!(has_status_to_show(EffectiveStatus::Absent, false));
        assert!(has_status_to_show(
            EffectiveStatus::Library(Status::Unplayed),
            true
        ));
    }

    #[test]
    fn what_is_finished_with_is_what_gets_hidden() {
        let (record, _) = entry(2025, "winner");
        assert!(!is_finished(&record, EffectiveStatus::Absent));
        assert!(!is_finished(
            &record,
            EffectiveStatus::Library(Status::InProgress)
        ));
        assert!(is_finished(
            &record,
            EffectiveStatus::Library(Status::Completed)
        ));
        let unsupported = CacowardRecord {
            supported: false,
            ..record
        };
        assert!(is_finished(&unsupported, EffectiveStatus::Absent));
    }

    #[test]
    fn the_kicker_names_only_what_the_year_gave() {
        let entries = [
            entry(2011, "winner"),
            entry(2011, "winner"),
            entry(2011, "mordeth"),
            entry(2023, "winner"),
            entry(2023, "runner-up"),
            entry(2023, "honorable-mention"),
        ];
        assert_eq!(hero_kicker(&entries, 2011), "2 winners");
        assert_eq!(
            hero_kicker(&entries, 2023),
            "1 winner · 1 runner-up · 1 honorable mention"
        );
    }

    #[test]
    fn a_row_gives_the_title_most_of_what_there_is_when_both_will_not_fit() {
        // Room for both, and each has what it needs.
        assert_eq!(share_row(300.0, 100.0, 50.0), (100.0, 50.0));
        // A short title leaves the long author the rest.
        assert_eq!(share_row(300.0, 100.0, 400.0), (100.0, 192.0));
        // A long title takes most of it, and the author still gets something.
        let (title, author) = share_row(300.0, 400.0, 400.0);
        assert!(title > author, "{title} {author}");
        assert_eq!(title + author, 300.0 - ROW_TEXT_GAP);
    }

    /// Why this is a list: a year's entries nearly fit on a screen, where six cards did.
    #[test]
    fn a_list_at_the_default_window_has_two_columns_and_the_smallest_has_one() {
        let columns = |window: f32| {
            let available: f32 = window - 200.0 - SECTION_PAD_X * 2.0;
            ((available + LIST_COLUMN_GAP) / (LIST_COLUMN_MIN + LIST_COLUMN_GAP)).floor() as usize
        };
        assert_eq!(columns(1200.0), 2);
        assert_eq!(columns(800.0), 1);
    }

    #[test]
    fn other_categories_keep_the_querys_order_and_leave_out_the_core_four() {
        let entries = [
            entry(2025, "winner"),
            entry(2025, "mordeth"),
            entry(2025, "doom-moma"),
            entry(2025, "doom-moma"),
            entry(2025, "multiplayer"),
        ];
        assert_eq!(other_categories(&entries), ["doom-moma", "multiplayer"]);
    }

    /// The screenshot harness would catch a missing glyph too, by panicking, but only
    /// for the one year it opens. The app itself would draw tofu.
    #[test]
    fn the_bundled_fonts_can_draw_every_entry() {
        let ctx = egui::Context::default();
        theme::apply_doom_theme(&ctx);
        let font = egui::FontId::proportional(14.0);

        let mut missing = Vec::new();
        let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.ctx().fonts_mut(|fonts| {
                for entry in caco_core::cacowards::builtin().entries {
                    let author = entry.wad_author.unwrap_or_default();
                    for text in [entry.wad_title, author] {
                        if !fonts.has_glyphs(&font, &text) {
                            missing.push(text);
                        }
                    }
                }
                assert!(fonts.has_glyphs(&font, "\u{2197}"), "the page pill's arrow");
            });
        });
        // Nothing is going to upload the atlas, and it refuses to be dropped unseen.
        frame.textures_delta.clear();
        assert_eq!(missing, Vec::<String>::new());
    }

    #[test]
    fn what_a_wad_won_reads_as_a_person_would_say_it() {
        assert_eq!(award_label("winner"), "Winner");
        assert_eq!(award_label("honorable-mention"), "Honorable Mention");
        assert_eq!(award_label("doom-moma"), "Doom moma");
        assert_eq!(award_label(""), "");
    }

    #[test]
    fn a_category_nobody_told_the_gui_about_still_gets_a_heading() {
        assert_eq!(category_label("winner"), "WINNERS");
        assert_eq!(category_label("adrians-pen"), "ADRIAN'S PEN");
        assert_eq!(
            category_label("spaceship-of-theseus"),
            "SPACESHIP OF THESEUS"
        );
    }
}

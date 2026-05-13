//! Magazine-style Cacowards view — the GUI surface for browsing yearly
//! awards, completion stats, and entry-level actions (import / open / play).
//!
//! Layout mirrors `mockups/cacowards-gui.html`:
//! 1. Hero banner with year and Doomworld attribution.
//! 2. Year strip with per-year completion ratios.
//! 3. Category sections (winner / runner-up / honorable-mention / mordeth)
//!    rendered as poster cards, each tagged with the linked WAD's status.
//!
//! Layout is built from primitives (custom rect/text painting + `Frame`)
//! rather than reusing the library's table widgets — the magazine treatment
//! is the whole point, so it deliberately doesn't share visual code with
//! the rest of the app.

use caco_core::db::cacowards::{CacowardRecord, EffectiveStatus};
use caco_core::db::{CORE_CATEGORIES, Status};

use crate::state::{ActionRequest, AppState};
use crate::theme;

const HERO_HEIGHT: f32 = 160.0;
const YEAR_STRIP_HEIGHT: f32 = 64.0;
const CARD_WIDTH: f32 = 280.0;
const CARD_HEIGHT: f32 = 130.0;
const CARD_GAP: f32 = 14.0;
const SECTION_PAD: f32 = 28.0;

/// Render the Cacowards central panel. Returns the first action request
/// produced this frame (typically a button click).
pub fn render(ui: &mut egui::Ui, state: &mut AppState) -> Option<ActionRequest> {
    if state.cacowards.all_entries.is_empty() {
        return render_empty(ui);
    }

    let mut action: Option<ActionRequest> = None;
    let year = match state.cacowards.selected_year {
        Some(y) => y,
        None => return render_empty(ui),
    };

    render_hero(ui, state, year);
    let year_change = render_year_strip(ui, state);
    if let Some(y) = year_change {
        state.cacowards.selected_year = Some(y);
    }

    // Snapshot the entries for the selected year so we can iterate without
    // holding a borrow on `state.cacowards`.
    let year_entries: Vec<(CacowardRecord, EffectiveStatus)> = state
        .cacowards
        .all_entries
        .iter()
        .filter(|(r, _)| r.year == year)
        .cloned()
        .collect();

    for &category in CORE_CATEGORIES {
        let in_section: Vec<&(CacowardRecord, EffectiveStatus)> = year_entries
            .iter()
            .filter(|(r, _)| r.category == category)
            .collect();
        if in_section.is_empty() {
            continue;
        }
        if let Some(a) = render_category_section(ui, category, &in_section) {
            action.get_or_insert(a);
        }
    }

    action
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

fn render_empty(ui: &mut egui::Ui) -> Option<ActionRequest> {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.colored_label(
            theme::TEXT_ACCENT,
            egui::RichText::new("No Cacoward data yet")
                .size(20.0)
                .strong(),
        );
        ui.add_space(8.0);
        ui.colored_label(
            theme::TEXT_SECONDARY,
            "Run `caco enrich --cacowards --year YYYY` to populate this view \
             from the Doom Wiki.",
        );
    });
    None
}

// ---------------------------------------------------------------------------
// Hero banner
// ---------------------------------------------------------------------------

fn render_hero(ui: &mut egui::Ui, state: &AppState, year: i64) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), HERO_HEIGHT),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);

    // Background: layered fills to approximate the mockup's radial gradient.
    // egui has no gradient primitive, so we stack a base + an offset accent
    // panel on the right to fake the warm glow.
    painter.rect_filled(rect, 0.0, theme::BG_MEDIUM);
    let glow_rect = egui::Rect::from_min_max(egui::pos2(rect.max.x - 360.0, rect.min.y), rect.max);
    painter.rect_filled(
        glow_rect,
        0.0,
        egui::Color32::from_rgba_unmultiplied(0xff, 0x66, 0x33, 24),
    );
    // Subtle bottom rule.
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y - 0.5),
            egui::pos2(rect.max.x, rect.max.y - 0.5),
        ],
        egui::Stroke::new(1.0, theme::BORDER_MED),
    );

    let pad_x = 40.0;
    let mut cursor_y = rect.min.y + 28.0;

    // Source pill — top-right corner.
    let source_text = "FROM DOOMWORLD · VIA DOOM WIKI";
    let source_font = egui::FontId::proportional(10.0);
    let source_galley = painter.layout_no_wrap(
        source_text.to_string(),
        source_font.clone(),
        theme::TEXT_MUTED,
    );
    let pill_pad = egui::vec2(10.0, 4.0);
    let pill_size = source_galley.size() + pill_pad * 2.0;
    let pill_rect = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - pad_x - pill_size.x, rect.min.y + 18.0),
        pill_size,
    );
    painter.rect(
        pill_rect,
        12.0,
        egui::Color32::from_black_alpha(80),
        egui::Stroke::new(1.0, theme::BORDER_MED),
        egui::StrokeKind::Inside,
    );
    painter.text(
        pill_rect.min + pill_pad,
        egui::Align2::LEFT_TOP,
        source_text,
        source_font,
        theme::TEXT_MUTED,
    );

    // Kicker — small all-caps editorial intro.
    let (total, done) = state.cacowards.year_summary(year);
    let by_cat = category_counts(&state.cacowards.all_entries, year);
    let kicker = format!(
        "ANNUAL CACOWARDS · {w} winners · {r} runners-up · {hm} honorable mentions",
        w = by_cat.get("winner").copied().unwrap_or(0),
        r = by_cat.get("runner-up").copied().unwrap_or(0),
        hm = by_cat.get("honorable-mention").copied().unwrap_or(0),
    );
    painter.text(
        egui::pos2(rect.min.x + pad_x, cursor_y),
        egui::Align2::LEFT_TOP,
        kicker,
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(0xd4, 0xa1, 0x4a), // gold
    );
    cursor_y += 22.0;

    // Title — "Cacowards" + accent year.
    let title_font = egui::FontId::proportional(48.0);
    let prefix = painter.layout_no_wrap(
        "Cacowards ".to_string(),
        title_font.clone(),
        theme::TEXT_PRIMARY,
    );
    let prefix_size = prefix.size();
    painter.galley(
        egui::pos2(rect.min.x + pad_x, cursor_y),
        prefix,
        theme::TEXT_PRIMARY,
    );
    painter.text(
        egui::pos2(rect.min.x + pad_x + prefix_size.x, cursor_y),
        egui::Align2::LEFT_TOP,
        format!("{year}"),
        title_font,
        theme::TEXT_ACCENT,
    );
    cursor_y += prefix_size.y + 12.0;

    // Byline — completion + descriptor.
    let pct = if total > 0 {
        done as f32 * 100.0 / total as f32
    } else {
        0.0
    };
    let byline = format!(
        "{done} of {total} entries completed ({pct:.0}%) · Doomworld's annual selection of the year's best WADs"
    );
    painter.text(
        egui::pos2(rect.min.x + pad_x, cursor_y),
        egui::Align2::LEFT_TOP,
        byline,
        egui::FontId::proportional(13.0),
        theme::TEXT_SECONDARY,
    );
}

// ---------------------------------------------------------------------------
// Year strip
// ---------------------------------------------------------------------------

/// Renders the horizontal year selector. Returns `Some(year)` if the user
/// clicked a year other than the current selection.
fn render_year_strip(ui: &mut egui::Ui, state: &AppState) -> Option<i64> {
    let mut clicked: Option<i64> = None;
    let selected = state.cacowards.selected_year;

    let years = state.cacowards.years();
    egui::Frame::new()
        .fill(theme::BG_MEDIUM)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .inner_margin(egui::Margin::symmetric(20, 12))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for year in years {
                            if year_chip(ui, state, year, selected == Some(year)) {
                                clicked = Some(year);
                            }
                            ui.add_space(6.0);
                        }
                    });
                });
        });

    clicked
}

fn year_chip(ui: &mut egui::Ui, state: &AppState, year: i64, active: bool) -> bool {
    let size = egui::vec2(80.0, YEAR_STRIP_HEIGHT - 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let hovered = response.hovered();

    let (bg, fg, sub) = if active {
        (
            theme::BG_SELECTED,
            theme::TEXT_ACCENT,
            theme::TEXT_SECONDARY,
        )
    } else if hovered {
        (theme::BG_LIGHT, theme::TEXT_PRIMARY, theme::TEXT_MUTED)
    } else {
        (theme::BG_MEDIUM, theme::TEXT_SECONDARY, theme::TEXT_MUTED)
    };

    painter.rect(
        rect,
        4.0,
        bg,
        egui::Stroke::new(if active { 1.0 } else { 0.0 }, theme::TEXT_ACCENT),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center() + egui::vec2(0.0, -8.0),
        egui::Align2::CENTER_CENTER,
        format!("{year}"),
        egui::FontId::proportional(14.0),
        fg,
    );
    let (total, done) = state.cacowards.year_summary(year);
    painter.text(
        rect.center() + egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        format!("{done}/{total}"),
        egui::FontId::proportional(10.0),
        sub,
    );

    response.clicked()
}

// ---------------------------------------------------------------------------
// Category sections
// ---------------------------------------------------------------------------

fn render_category_section(
    ui: &mut egui::Ui,
    category: &str,
    entries: &[&(CacowardRecord, EffectiveStatus)],
) -> Option<ActionRequest> {
    let total = entries.len();
    let done = entries
        .iter()
        .filter(|(_, s)| matches!(s, EffectiveStatus::Library(Status::Completed)))
        .count();
    let absent = entries
        .iter()
        .filter(|(_, s)| matches!(s, EffectiveStatus::Absent))
        .count();

    let mut action: Option<ActionRequest> = None;

    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 40,
            right: 40,
            top: SECTION_PAD as i8,
            bottom: SECTION_PAD as i8,
        })
        .show(ui, |ui| {
            // Section header
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(category_label(category))
                        .size(13.0)
                        .strong()
                        .color(theme::TEXT_ACCENT)
                        .extra_letter_spacing(2.0),
                );
            });
            ui.add_space(4.0);

            // Progress line: meter + counts
            ui.horizontal(|ui| {
                draw_meter(ui, done, total, 110.0);
                ui.add_space(8.0);
                let summary = format!(
                    "{done} of {total} completed{}",
                    if absent > 0 {
                        format!(" · {absent} absent")
                    } else {
                        String::new()
                    }
                );
                ui.colored_label(theme::TEXT_SECONDARY, summary);
            });
            ui.add_space(14.0);

            // Card grid: wrap based on available width.
            let avail = ui.available_width();
            let cols = ((avail + CARD_GAP) / (CARD_WIDTH + CARD_GAP))
                .floor()
                .max(1.0) as usize;

            egui::Grid::new(("cacoward-grid", category))
                .num_columns(cols)
                .spacing(egui::vec2(CARD_GAP, CARD_GAP))
                .show(ui, |ui| {
                    for (i, (record, status)) in entries.iter().enumerate() {
                        if let Some(a) = render_card(ui, record, *status) {
                            action.get_or_insert(a);
                        }
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });

            ui.add_space(8.0);
        });

    // Divider between sections
    let rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x + 40.0, rect.min.y),
            egui::pos2(rect.max.x - 40.0, rect.min.y),
        ],
        egui::Stroke::new(1.0, theme::BORDER),
    );

    action
}

fn render_card(
    ui: &mut egui::Ui,
    record: &CacowardRecord,
    status: EffectiveStatus,
) -> Option<ActionRequest> {
    let mut action: Option<ActionRequest> = None;
    let accent = status_accent(status);
    let absent = matches!(status, EffectiveStatus::Absent);

    let stroke = if absent {
        egui::Stroke::new(1.0, theme::BORDER_MED)
    } else {
        egui::Stroke::NONE
    };

    egui::Frame::new()
        .fill(theme::BG_MEDIUM)
        .stroke(stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(CARD_WIDTH - 16.0, CARD_HEIGHT - 16.0));
            ui.set_max_width(CARD_WIDTH - 16.0);

            // Status-colored left edge (skip for absent — already dashed).
            if !absent {
                let rect = ui.max_rect();
                let edge = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x - 16.0, rect.min.y - 16.0),
                    egui::vec2(3.0, rect.height() + 32.0),
                );
                ui.painter().rect_filled(edge, 0.0, accent);
            }

            // Rank + status badge row
            ui.horizontal(|ui| {
                if let Some(rank) = record.rank {
                    ui.colored_label(
                        theme::TEXT_MUTED,
                        egui::RichText::new(format!("#{rank}")).size(11.0).strong(),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, status);
                });
            });
            ui.add_space(4.0);

            // Title
            let title_color = if absent {
                theme::TEXT_SECONDARY
            } else {
                theme::TEXT_PRIMARY
            };
            ui.label(
                egui::RichText::new(&record.wad_title)
                    .size(15.0)
                    .strong()
                    .color(title_color),
            );

            // Author
            if let Some(author) = record.wad_author.as_deref() {
                ui.colored_label(
                    theme::TEXT_SECONDARY,
                    egui::RichText::new(author).size(12.0),
                );
            }

            // Action button — pushed to the bottom of the card.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    if let Some(a) = action_button(ui, record, status) {
                        action = Some(a);
                    }
                });
            });
        });

    action
}

// ---------------------------------------------------------------------------
// Small visual helpers
// ---------------------------------------------------------------------------

fn category_label(category: &str) -> &'static str {
    match category {
        "winner" => "WINNERS",
        "runner-up" => "RUNNERS-UP",
        "honorable-mention" => "HONORABLE MENTIONS",
        "mordeth" => "MORDETH AWARD",
        _ => "OTHER",
    }
}

fn category_counts(
    entries: &[(CacowardRecord, EffectiveStatus)],
    year: i64,
) -> std::collections::HashMap<String, usize> {
    let mut m: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (r, _) in entries.iter().filter(|(r, _)| r.year == year) {
        *m.entry(r.category.clone()).or_insert(0) += 1;
    }
    m
}

fn status_accent(status: EffectiveStatus) -> egui::Color32 {
    match status {
        EffectiveStatus::Library(Status::Completed) => theme::COLOR_SUCCESS,
        EffectiveStatus::Library(Status::InProgress) => theme::COLOR_WARNING,
        EffectiveStatus::Library(Status::Abandoned) => theme::COLOR_ERROR,
        EffectiveStatus::Library(Status::Unplayed) => egui::Color32::from_rgb(0x33, 0x66, 0xcc),
        EffectiveStatus::Absent => theme::TEXT_MUTED,
    }
}

fn status_pill(ui: &mut egui::Ui, status: EffectiveStatus) {
    let (text, color) = match status {
        EffectiveStatus::Library(Status::Completed) => ("completed", theme::COLOR_SUCCESS),
        EffectiveStatus::Library(Status::InProgress) => ("in progress", theme::COLOR_WARNING),
        EffectiveStatus::Library(Status::Abandoned) => ("dropped", theme::COLOR_ERROR),
        EffectiveStatus::Library(Status::Unplayed) => {
            ("unplayed", egui::Color32::from_rgb(0x33, 0x66, 0xcc))
        }
        EffectiveStatus::Absent => ("absent", theme::TEXT_SECONDARY),
    };
    let bg = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 32);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(11.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(color)
                    .size(10.0)
                    .strong()
                    .extra_letter_spacing(0.4),
            );
        });
}

fn action_button(
    ui: &mut egui::Ui,
    record: &CacowardRecord,
    status: EffectiveStatus,
) -> Option<ActionRequest> {
    match status {
        EffectiveStatus::Absent => {
            let btn = egui::Button::new(
                egui::RichText::new("Import")
                    .size(12.0)
                    .strong()
                    .color(egui::Color32::from_rgb(0x1a, 0x0a, 0x04)),
            )
            .fill(theme::TEXT_ACCENT)
            .corner_radius(4.0);
            if ui.add(btn).clicked() {
                return Some(ActionRequest::ImportCacoward(record.id));
            }
            None
        }
        EffectiveStatus::Library(_) => {
            // Library row exists — the headline action is "play it". The
            // wad_id is guaranteed Some when status is Library(_).
            let wad_id = record.wad_id?;
            let label = match status {
                EffectiveStatus::Library(Status::InProgress)
                | EffectiveStatus::Library(Status::Unplayed) => "Play",
                _ => "Open",
            };
            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(12.0)
                    .color(theme::TEXT_PRIMARY),
            )
            .fill(theme::BG_LIGHT)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_MED))
            .corner_radius(4.0);
            if ui.add(btn).clicked() {
                return Some(ActionRequest::Play(wad_id));
            }
            None
        }
    }
}

fn draw_meter(ui: &mut egui::Ui, done: usize, total: usize, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::BG_LIGHT);
    if total > 0 {
        let fill_w = rect.width() * (done as f32 / total as f32);
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 3.0, theme::TEXT_ACCENT);
    }
}

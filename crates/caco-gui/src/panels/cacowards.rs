//! Magazine-style Cacowards view — the GUI surface for browsing yearly
//! awards, completion stats, and entry-level actions (import / open / play).
//!
//! Layout mirrors `mockups/cacowards-gui.html`:
//! 1. Hero banner with year and Doomworld attribution.
//! 2. Year strip with per-year completion ratios.
//! 3. Category sections (winner / runner-up / honorable-mention / mordeth)
//!    rendered as poster cards, each tagged with the linked WAD's status.
//!
//! Cards are drawn with manual `allocate_exact_size` + direct painter calls
//! (the same pattern as `wad_grid.rs`) rather than `egui::Grid`. Using Grid
//! sized columns to the widest content, which made cards expand to the
//! full content area and overlap.

use caco_core::db::cacowards::{CacowardRecord, EffectiveStatus};
use caco_core::db::{CORE_CATEGORIES, Status};
use egui::{Color32, CornerRadius, Rect, StrokeKind, Vec2};

use crate::state::{ActionRequest, AppState};
use crate::theme;

const HERO_HEIGHT: f32 = 150.0;
const HERO_PAD_X: f32 = 40.0;
const SECTION_PAD_X: f32 = 40.0;
const SECTION_PAD_Y: f32 = 24.0;

const YEAR_CHIP_WIDTH: f32 = 78.0;
const YEAR_CHIP_HEIGHT: f32 = 40.0;
const YEAR_STRIP_VPAD: f32 = 14.0;

const CARD_MIN_WIDTH: f32 = 240.0;
const CARD_MAX_WIDTH: f32 = 320.0;
const CARD_GAP: f32 = 14.0;
const CARD_ROUNDING: u8 = 8;
const STATUS_EDGE_WIDTH: f32 = 3.0;
/// Thumbnail height as a multiple of card width. Matches the 5:3 poster
/// proportion the magazine view leans on; a touch shorter than the
/// library grid's 4:3 so the body has room for title + button.
const THUMB_ASPECT: f32 = 0.6;
/// Fixed body height (title up to 2 lines + author + action button +
/// padding). Card height is `thumb_h + CARD_BODY_HEIGHT`.
const CARD_BODY_HEIGHT: f32 = 96.0;

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
        state.cacowards.selected_year = Some(y);
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

    // Esc clears any card selection. Library-mode shortcuts (j/k/etc.)
    // aren't reused here yet — the cacoward grid is keyboard-navigable in
    // a follow-up.
    if !state.has_dialog() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.cacowards.selected_entry_pk = None;
    }

    for &category in CORE_CATEGORIES {
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
        Vec2::new(ui.available_width(), HERO_HEIGHT),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);

    // Flat dark background with a very slightly warmer right edge — the
    // previous attempt used a semi-transparent overlay rect which painted
    // as a hard-edged solid block. Easier to just blend the warm tone
    // into the base color directly.
    painter.rect_filled(rect, 0, theme::BG_MEDIUM);

    // Subtle warm tint on the right third — built from rect bands so it
    // fades rather than presenting as a colored panel.
    let bands = 12;
    let band_w = 160.0 / bands as f32;
    for i in 0..bands {
        let t = (i + 1) as f32 / bands as f32; // 1/bands … 1.0
        let x = rect.max.x - 160.0 + band_w * i as f32;
        let band_rect = Rect::from_min_max(
            egui::pos2(x, rect.min.y),
            egui::pos2(x + band_w + 0.5, rect.max.y),
        );
        // Lerp BG_MEDIUM → slightly warmer; max contribution stays
        // dim enough that it reads as ambient rather than a panel.
        let r = lerp_byte(theme::BG_MEDIUM.r(), 0x32, t * 0.45);
        let g = lerp_byte(theme::BG_MEDIUM.g(), 0x1c, t * 0.45);
        let b = lerp_byte(theme::BG_MEDIUM.b(), 0x14, t * 0.45);
        painter.rect_filled(band_rect, 0, Color32::from_rgb(r, g, b));
    }

    // Bottom rule
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y - 0.5),
            egui::pos2(rect.max.x, rect.max.y - 0.5),
        ],
        egui::Stroke::new(1.0, theme::BORDER_MED),
    );

    // Source pill — top-right.
    let source_text = "FROM DOOMWORLD · VIA DOOM WIKI";
    let source_font = egui::FontId::proportional(10.0);
    let galley = painter.layout_no_wrap(
        source_text.to_string(),
        source_font.clone(),
        theme::TEXT_MUTED,
    );
    let pill_pad = Vec2::new(10.0, 4.0);
    let pill_size = galley.size() + pill_pad * 2.0;
    let pill_rect = Rect::from_min_size(
        egui::pos2(rect.max.x - HERO_PAD_X - pill_size.x, rect.min.y + 18.0),
        pill_size,
    );
    painter.rect(
        pill_rect,
        CornerRadius::same(11),
        Color32::from_black_alpha(80),
        egui::Stroke::new(1.0, theme::BORDER_MED),
        StrokeKind::Inside,
    );
    painter.text(
        pill_rect.min + pill_pad,
        egui::Align2::LEFT_TOP,
        source_text,
        source_font,
        theme::TEXT_MUTED,
    );

    let mut cursor_y = rect.min.y + 24.0;

    // Kicker
    let by_cat = category_counts(&state.cacowards.all_entries, year);
    let kicker = format!(
        "ANNUAL CACOWARDS · {w} winners · {r} runners-up · {hm} honorable mentions",
        w = by_cat.get("winner").copied().unwrap_or(0),
        r = by_cat.get("runner-up").copied().unwrap_or(0),
        hm = by_cat.get("honorable-mention").copied().unwrap_or(0),
    );
    painter.text(
        egui::pos2(rect.min.x + HERO_PAD_X, cursor_y),
        egui::Align2::LEFT_TOP,
        kicker,
        egui::FontId::proportional(11.0),
        Color32::from_rgb(0xd4, 0xa1, 0x4a), // gold
    );
    cursor_y += 20.0;

    // Title — "Cacowards YEAR"
    let title_font = egui::FontId::proportional(40.0);
    let prefix_galley = painter.layout_no_wrap(
        "Cacowards ".to_string(),
        title_font.clone(),
        theme::TEXT_PRIMARY,
    );
    let prefix_w = prefix_galley.size().x;
    painter.galley(
        egui::pos2(rect.min.x + HERO_PAD_X, cursor_y),
        prefix_galley,
        theme::TEXT_PRIMARY,
    );
    painter.text(
        egui::pos2(rect.min.x + HERO_PAD_X + prefix_w, cursor_y),
        egui::Align2::LEFT_TOP,
        year.to_string(),
        title_font,
        theme::TEXT_ACCENT,
    );
    cursor_y += 48.0;

    // Byline
    let (total, done) = state.cacowards.year_summary(year);
    let pct = if total > 0 {
        done as f32 * 100.0 / total as f32
    } else {
        0.0
    };
    let byline = format!(
        "{done} of {total} entries completed ({pct:.0}%) · Doomworld's annual selection of the year's best WADs"
    );
    painter.text(
        egui::pos2(rect.min.x + HERO_PAD_X, cursor_y),
        egui::Align2::LEFT_TOP,
        byline,
        egui::FontId::proportional(13.0),
        theme::TEXT_SECONDARY,
    );
}

// ---------------------------------------------------------------------------
// Year strip
// ---------------------------------------------------------------------------

fn render_year_strip(ui: &mut egui::Ui, state: &AppState) -> Option<i64> {
    let years = state.cacowards.years();
    let selected = state.cacowards.selected_year;
    let strip_height = YEAR_CHIP_HEIGHT + YEAR_STRIP_VPAD * 2.0;

    // Background strip the full width of the panel, with horizontal scroll
    // for the chip row.
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
        egui::Stroke::new(1.0, theme::BORDER),
    );

    // Layout chips manually inside the strip.
    let mut x = strip_rect.min.x + HERO_PAD_X;
    let y = strip_rect.min.y + YEAR_STRIP_VPAD;
    let mut clicked: Option<i64> = None;
    for year in years {
        let chip_rect = Rect::from_min_size(
            egui::pos2(x, y),
            Vec2::new(YEAR_CHIP_WIDTH, YEAR_CHIP_HEIGHT),
        );
        // Bail out if we run out of room (scroll arrives in a follow-up;
        // 30+ years of awards at 78px each fits in a 2400px+ window
        // already and breaks gracefully when it doesn't).
        if chip_rect.max.x > strip_rect.max.x - HERO_PAD_X {
            break;
        }
        let response = ui.interact(
            chip_rect,
            ui.id().with(("cacoward-year", year)),
            egui::Sense::click(),
        );
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
        }
        x += YEAR_CHIP_WIDTH + 6.0;
    }
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
        egui::Stroke::new(if active { 1.0 } else { 0.0 }, theme::TEXT_ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center() + Vec2::new(0.0, -8.0),
        egui::Align2::CENTER_CENTER,
        year.to_string(),
        egui::FontId::proportional(14.0),
        fg,
    );
    let (total, done) = state.cacowards.year_summary(year);
    painter.text(
        rect.center() + Vec2::new(0.0, 9.0),
        egui::Align2::CENTER_CENTER,
        format!("{done}/{total}"),
        egui::FontId::proportional(10.0),
        sub,
    );
}

// ---------------------------------------------------------------------------
// Category sections
// ---------------------------------------------------------------------------

fn render_category_section(
    ui: &mut egui::Ui,
    state: &mut AppState,
    category: &str,
    entries: &[&(CacowardRecord, EffectiveStatus)],
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
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

    // Top padding
    ui.add_space(SECTION_PAD_Y);

    // Section header — small caps, accent
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: SECTION_PAD_X as i8,
            right: SECTION_PAD_X as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(category_label(category))
                    .size(12.0)
                    .strong()
                    .color(theme::TEXT_ACCENT)
                    .extra_letter_spacing(2.0),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                draw_meter(ui, done, total, 120.0);
                ui.add_space(10.0);
                let summary = if absent > 0 {
                    format!("{done} of {total} completed · {absent} absent")
                } else {
                    format!("{done} of {total} completed")
                };
                ui.colored_label(theme::TEXT_SECONDARY, summary);
            });
        });

    ui.add_space(14.0);

    // Card grid — manual layout to keep cards at a fixed width.
    let available = ui.available_width() - SECTION_PAD_X * 2.0;
    let card_w = card_width(available);
    let card_h = card_height(card_w);
    let columns = ((available + CARD_GAP) / (card_w + CARD_GAP))
        .floor()
        .max(1.0) as usize;

    let rows = entries.len().div_ceil(columns);
    for row in 0..rows {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: SECTION_PAD_X as i8,
                right: SECTION_PAD_X as i8,
                top: 0,
                bottom: 0,
            })
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

    ui.add_space(SECTION_PAD_Y);

    // Section divider
    let avail_rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(avail_rect.min.x + SECTION_PAD_X, avail_rect.min.y),
            egui::pos2(avail_rect.max.x - SECTION_PAD_X, avail_rect.min.y),
        ],
        egui::Stroke::new(1.0, theme::BORDER),
    );

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
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let rounding = CornerRadius::same(CARD_ROUNDING);
    let absent = matches!(status, EffectiveStatus::Absent);

    // Selection click on the card surface (the action button has its own
    // click handler, but Button is rendered on top so its events take
    // precedence — clicking elsewhere selects the card).
    let selected = state.cacowards.selected_entry_pk == Some(record.id);
    if response.clicked() {
        state.cacowards.selected_entry_pk = Some(record.id);
    }

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

    // Top overlay strip + rank / status pill.
    let strip = Rect::from_min_max(
        thumb_rect.min,
        egui::pos2(thumb_rect.max.x, thumb_rect.min.y + 32.0),
    );
    painter.rect_filled(strip, thumb_rounding, Color32::from_black_alpha(110));
    if let Some(rank) = record.rank {
        painter.text(
            thumb_rect.min + Vec2::new(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            format!("#{rank}"),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(0xe8, 0xd8, 0xc8),
        );
    }
    paint_status_pill(&painter, thumb_rect.shrink2(Vec2::new(8.0, 8.0)), status);

    // ── Body ──────────────────────────────────────────────────────────
    // Status-colored left edge — only across the body so it doesn't
    // collide with the thumbnail's rounded corners.
    if !absent {
        let accent = status_accent(status);
        let edge = Rect::from_min_size(
            egui::pos2(rect.min.x, thumb_rect.max.y),
            Vec2::new(STATUS_EDGE_WIDTH, rect.max.y - thumb_rect.max.y),
        );
        let edge_rounding = CornerRadius {
            nw: 0,
            sw: CARD_ROUNDING,
            ne: 0,
            se: 0,
        };
        painter.rect_filled(edge, edge_rounding, accent);
    }

    let pad_x = 12.0;
    let body = Rect::from_min_max(
        egui::pos2(
            rect.min.x + pad_x + STATUS_EDGE_WIDTH,
            thumb_rect.max.y + 10.0,
        ),
        egui::pos2(rect.max.x - pad_x, rect.max.y - 10.0),
    );

    let title_color = if absent {
        theme::TEXT_SECONDARY
    } else {
        theme::TEXT_PRIMARY
    };
    let mut title_job = egui::text::LayoutJob::single_section(
        record.wad_title.clone(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(14.0),
            color: title_color,
            ..Default::default()
        },
    );
    title_job.wrap.max_width = body.width();
    title_job.wrap.max_rows = 2;
    title_job.wrap.break_anywhere = false;
    let title_galley = painter.layout_job(title_job);
    let title_h = title_galley.size().y;
    painter.galley(body.min, title_galley, title_color);

    if let Some(author) = record.wad_author.as_deref() {
        painter.text(
            egui::pos2(body.min.x, body.min.y + title_h + 3.0),
            egui::Align2::LEFT_TOP,
            truncate(author, 36),
            egui::FontId::proportional(11.0),
            theme::TEXT_SECONDARY,
        );
    }

    let button_rect = Rect::from_min_max(
        egui::pos2(body.min.x, body.max.y - 22.0),
        egui::pos2(body.max.x, body.max.y),
    );
    let mut sub_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(button_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let button_action = action_button(&mut sub_ui, record, status);

    // Border painted INSIDE the rect — `StrokeKind::Outside` was
    // effectively halving the visible width because the row's
    // horizontal layout clipped to its allocated space, eating the
    // outer pixels of the ring.
    if selected {
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(3.0, theme::TEXT_ACCENT),
            StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.5, theme::BORDER_MED),
            StrokeKind::Inside,
        );
    } else if absent {
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.0, theme::BORDER_MED),
            StrokeKind::Inside,
        );
    }

    button_action
}

fn paint_card_thumbnail(
    painter: &egui::Painter,
    thumb_rect: Rect,
    thumb_rounding: CornerRadius,
    record: &CacowardRecord,
    thumbnails: Option<&crate::thumbnails::ThumbnailManager>,
) {
    // Try the linked WAD's TITLEPIC first.
    let thumb_key = record
        .wad_id
        .unwrap_or_else(|| thumb_key_for_absent(record.id));
    if let Some(tm) = thumbnails
        && let Some(tex) = tm.get(thumb_key)
    {
        painter.rect_filled(thumb_rect, thumb_rounding, Color32::BLACK);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(tex.id(), thumb_rect, uv, Color32::WHITE);
        return;
    }

    // Gradient placeholder seeded by the cacoward pk so the same entry
    // always paints the same way across renders.
    let (c1, c2, ci) = theme::thumb_colors(record.id);
    painter.rect_filled(thumb_rect, thumb_rounding, c1);
    let steps = 32;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let band_y0 = thumb_rect.min.y + thumb_rect.height() * t0;
        let band_y1 = thumb_rect.min.y + thumb_rect.height() * t1;
        let r = lerp_byte(c1.r(), c2.r(), t0);
        let g = lerp_byte(c1.g(), c2.g(), t0);
        let b = lerp_byte(c1.b(), c2.b(), t0);
        let band = Rect::from_min_max(
            egui::pos2(thumb_rect.min.x, band_y0),
            egui::pos2(thumb_rect.max.x, band_y1),
        );
        painter.rect_filled(band, 0, Color32::from_rgb(r, g, b));
    }
    let initials: String = record
        .wad_title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .flat_map(|c| c.to_uppercase())
        .collect();
    if !initials.is_empty() {
        painter.text(
            thumb_rect.center(),
            egui::Align2::CENTER_CENTER,
            &initials,
            egui::FontId::proportional(32.0),
            ci,
        );
    }
}

fn paint_status_pill(painter: &egui::Painter, content: Rect, status: EffectiveStatus) {
    let (text, color) = status_label_and_color(status);
    let font = egui::FontId::proportional(10.0);
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), color);
    let pad = Vec2::new(8.0, 2.0);
    let pill_size = galley.size() + pad * 2.0;
    let pill_rect = Rect::from_min_size(
        egui::pos2(content.max.x - pill_size.x, content.min.y - 2.0),
        pill_size,
    );
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 32);
    painter.rect_filled(pill_rect, CornerRadius::same(11), bg);
    painter.galley(pill_rect.min + pad, galley, color);
}

// ---------------------------------------------------------------------------
// Helpers
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

fn status_accent(status: EffectiveStatus) -> Color32 {
    match status {
        EffectiveStatus::Library(Status::Completed) => theme::COLOR_SUCCESS,
        EffectiveStatus::Library(Status::InProgress) => theme::COLOR_WARNING,
        EffectiveStatus::Library(Status::Abandoned) => theme::COLOR_ERROR,
        EffectiveStatus::Library(Status::Unplayed) => Color32::from_rgb(0x33, 0x66, 0xcc),
        EffectiveStatus::Absent => theme::TEXT_MUTED,
    }
}

fn status_label_and_color(status: EffectiveStatus) -> (&'static str, Color32) {
    match status {
        EffectiveStatus::Library(Status::Completed) => ("completed", theme::COLOR_SUCCESS),
        EffectiveStatus::Library(Status::InProgress) => ("in progress", theme::COLOR_WARNING),
        EffectiveStatus::Library(Status::Abandoned) => ("dropped", theme::COLOR_ERROR),
        EffectiveStatus::Library(Status::Unplayed) => {
            ("unplayed", Color32::from_rgb(0x33, 0x66, 0xcc))
        }
        EffectiveStatus::Absent => ("absent", theme::TEXT_SECONDARY),
    }
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
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(0x1a, 0x0a, 0x04)),
            )
            .fill(theme::TEXT_ACCENT)
            .corner_radius(4.0)
            .min_size(Vec2::new(0.0, 22.0));
            if ui.add(btn).clicked() {
                return Some(ActionRequest::ImportCacoward(record.id));
            }
            None
        }
        EffectiveStatus::Library(s) => {
            let wad_id = record.wad_id?;
            let label = if matches!(s, Status::InProgress | Status::Unplayed) {
                "Play"
            } else {
                "Open"
            };
            let btn = egui::Button::new(
                egui::RichText::new(label)
                    .size(11.0)
                    .color(theme::TEXT_PRIMARY),
            )
            .fill(theme::BG_LIGHT)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_MED))
            .corner_radius(4.0)
            .min_size(Vec2::new(0.0, 22.0));
            if ui.add(btn).clicked() {
                return Some(ActionRequest::Play(wad_id));
            }
            None
        }
    }
}

fn draw_meter(ui: &mut egui::Ui, done: usize, total: usize, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(3), theme::BG_LIGHT);
    if total > 0 {
        let fill_w = rect.width() * (done as f32 / total as f32);
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(fill_rect, CornerRadius::same(3), theme::TEXT_ACCENT);
    }
}

/// Map a cacoward entry pk to a synthetic ThumbnailManager key for absent
/// entries. Wad ids are always positive SQLite primary keys, so negating
/// the cacoward pk gives a stable, collision-free namespace we can share
/// the existing thumbnail cache + worker pool with.
pub fn thumb_key_for_absent(cacoward_pk: i64) -> i64 {
    -cacoward_pk
}

fn lerp_byte(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

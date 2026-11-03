//! Magazine-style Cacowards view — the GUI surface for browsing yearly
//! awards, completion stats, and entry-level actions (import / get / open / play).
//!
//! Layout:
//! 1. Hero banner with the year, and a link to Doomworld's write-ups for it.
//! 2. Year strip with per-year completion ratios.
//! 3. Category sections (winner / runner-up / honorable-mention / mordeth)
//!    rendered as poster cards, each tagged with the linked WAD's status.
//! 4. Every other award the year gave, the same way, under a heading saying that
//!    they don't count towards its completion.
//!
//! Cards are drawn with manual `allocate_exact_size` + direct painter calls
//! (the same pattern as `wad_grid.rs`) rather than `egui::Grid`. Using Grid
//! sized columns to the widest content, which made cards expand to the
//! full content area and overlap.

use caco_core::db::cacowards::{CacowardRecord, EffectiveStatus};
use caco_core::db::{CORE_CATEGORIES, Status, is_core_category};
use egui::{Color32, CornerRadius, Rect, StrokeKind, Vec2};

use crate::get::GetTarget;
use crate::state::{ActionRequest, AppState};
use crate::theme;

const HERO_HEIGHT: f32 = 150.0;
const HERO_PAD_X: f32 = 40.0;
const SECTION_PAD_X: f32 = 40.0;
const SECTION_PAD_Y: f32 = 24.0;

const YEAR_CHIP_WIDTH: f32 = 78.0;
const YEAR_CHIP_HEIGHT: f32 = 40.0;
const YEAR_CHIP_GAP: f32 = 6.0;
const YEAR_STRIP_VPAD: f32 = 14.0;

const CARD_MIN_WIDTH: f32 = 240.0;
const CARD_MAX_WIDTH: f32 = 320.0;
const CARD_GAP: f32 = 14.0;
const CARD_ROUNDING: u8 = 8;
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

    let others = other_categories(&year_entries);
    let core = CORE_CATEGORIES.iter().copied();
    for (index, category) in core.chain(others.iter().copied()).enumerate() {
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
            "The Cacowards list could not be loaded when caco started.",
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
        egui::Stroke::new(1.0_f32, theme::BORDER_MED),
    );

    render_page_pill(
        ui,
        rect,
        state.cacowards.pages.get(&year).map(String::as_str),
    );

    let mut cursor_y = rect.min.y + 24.0;

    // Kicker
    let kicker = hero_kicker(&state.cacowards.all_entries, year);
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

    // Hero progress bar — wide, thin, just above the bottom rule.
    // Replaces the byline as the gut-feel "look how far I've come"
    // signal; the byline becomes the precise label below it. Fills
    // green at 100% to mirror the per-category meter's swept state.
    let bar_h = 4.0;
    let bar_y = rect.max.y - bar_h - 12.0;
    let bar_rect = Rect::from_min_max(
        egui::pos2(rect.min.x + HERO_PAD_X, bar_y),
        egui::pos2(rect.max.x - HERO_PAD_X, bar_y + bar_h),
    );
    painter.rect_filled(bar_rect, CornerRadius::same(2), theme::BG_LIGHT);
    if total > 0 {
        let fill_w = bar_rect.width() * (done as f32 / total as f32);
        let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_h));
        let fill_color = if done == total {
            theme::COLOR_SUCCESS
        } else {
            theme::TEXT_ACCENT
        };
        painter.rect_filled(fill_rect, CornerRadius::same(2), fill_color);
    }
}

/// The year's Doomworld page, top-right of the hero. The write-ups are Doomworld's
/// own text, which is why caco links to them rather than carrying them.
fn render_page_pill(ui: &mut egui::Ui, hero: Rect, page: Option<&str>) {
    let painter = ui.painter_at(hero);
    let text = if page.is_some() {
        "WRITE-UPS ON DOOMWORLD \u{2197}"
    } else {
        "FROM DOOMWORLD"
    };
    let font = egui::FontId::proportional(10.0);
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), theme::TEXT_MUTED);
    let pad = Vec2::new(10.0, 4.0);
    let size = galley.size() + pad * 2.0;
    // On the title's row, whose right end is free at any window size. The kicker
    // above it reaches the pill's edge at the minimum width.
    let pill = Rect::from_min_size(
        egui::pos2(hero.max.x - HERO_PAD_X - size.x, hero.min.y + 62.0),
        size,
    );

    let mut color = theme::TEXT_MUTED;
    if let Some(page) = page {
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
    }

    painter.rect(
        pill,
        CornerRadius::same(11),
        Color32::from_black_alpha(80),
        egui::Stroke::new(1.0_f32, theme::BORDER_MED),
        StrokeKind::Inside,
    );
    painter.text(pill.min + pad, egui::Align2::LEFT_TOP, text, font, color);
}

/// "ANNUAL CACOWARDS · 12 winners · 2 runners-up", naming only what the year gave:
/// most of them had no honorable mentions at all.
fn hero_kicker(entries: &[(CacowardRecord, EffectiveStatus)], year: i64) -> String {
    let mut parts = vec!["ANNUAL CACOWARDS".to_string()];
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

fn render_year_strip(ui: &mut egui::Ui, state: &AppState) -> Option<i64> {
    let years = state.cacowards.years();
    let selected = state.cacowards.selected_year;
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
            .max_rect(strip_rect.shrink2(Vec2::new(HERO_PAD_X, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    // Solid rather than the floating bar that only shows on hover: the strip can
    // end exactly on a chip's edge, which looks like the end of the years.
    strip_ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
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

    // Section header — small caps, accent
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: SECTION_PAD_X as i8,
            right: SECTION_PAD_X as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(category_label(category))
                        .size(12.0)
                        .strong()
                        .color(theme::TEXT_ACCENT)
                        .extra_letter_spacing(2.0),
                );
                if swept {
                    ui.add_space(8.0);
                    paint_swept_chip(ui);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                draw_meter(ui, done, total, 120.0, swept);
                ui.add_space(10.0);
                let mut bits = vec![format!("{done} of {total} completed")];
                if absent > 0 {
                    bits.push(format!("{absent} absent"));
                }
                if unsupported > 0 {
                    bits.push(format!("{unsupported} unsupported"));
                }
                ui.colored_label(theme::TEXT_SECONDARY, bits.join(" · "));
            });
        });

    ui.add_space(14.0);

    // Reorder so undone supported entries render first, completed entries
    // pile up next, and unsupported entries trail at the very end. Stable
    // sort preserves page order within each group.
    let mut ordered: Vec<&(CacowardRecord, EffectiveStatus)> = entries.to_vec();
    ordered.sort_by_key(|(r, s)| {
        if !r.supported {
            2u8
        } else if matches!(s, EffectiveStatus::Library(Status::Completed)) {
            1
        } else {
            0
        }
    });

    // Card grid — manual layout to keep cards at a fixed width.
    let available = ui.available_width() - SECTION_PAD_X * 2.0;
    let card_w = card_width(available);
    let card_h = card_height(card_w);
    let columns = ((available + CARD_GAP) / (card_w + CARD_GAP))
        .floor()
        .max(1.0) as usize;

    let rows = ordered.len().div_ceil(columns);
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
                        if idx >= ordered.len() {
                            break;
                        }
                        let (record, status) = ordered[idx];
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
        egui::Stroke::new(1.0_f32, theme::BORDER),
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
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click_and_drag());
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

    // Right-click context menu: manual linking + supported toggle. Used
    // for the long tail of cases the auto-linker can't reach (renamed
    // titles, alternative IWAD families, etc.).
    let linked = record
        .wad_id
        .and_then(|id| state.cacowards.linked_wads.get(&id));
    let get = get_target(record, status, linked);
    let mut menu_action: Option<ActionRequest> = None;
    response.context_menu(|ui| {
        if let Some(target) = get
            && ui.button("Get in Browser").clicked()
        {
            menu_action = Some(ActionRequest::Get(target));
            ui.close();
        }
        if ui.button("Link to library WAD…").clicked() {
            menu_action = Some(ActionRequest::LinkCacoward(record.id));
            ui.close();
        }
        if record.wad_id.is_some() && ui.button("Unlink").clicked() {
            menu_action = Some(ActionRequest::UnlinkCacoward(record.id));
            ui.close();
        }
        ui.separator();
        let toggle_label = if record.supported {
            "Mark as unsupported"
        } else {
            "Mark as supported"
        };
        if ui.button(toggle_label).clicked() {
            menu_action = Some(ActionRequest::SetCacowardSupported(
                record.id,
                !record.supported,
            ));
            ui.close();
        }
    });

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

    // Completed and unsupported cards both get a dark wash over the
    // thumbnail. Completed = "ticked off"; unsupported = "out of the
    // game" — they recede for different reasons but the visual cue is
    // the same so the eye lands on actionable cards first.
    let dim = matches!(status, EffectiveStatus::Library(Status::Completed)) || !record.supported;
    if dim {
        painter.rect_filled(thumb_rect, thumb_rounding, Color32::from_black_alpha(140));
    }

    // Top overlay strip + status pill.
    let strip = Rect::from_min_max(
        thumb_rect.min,
        egui::pos2(thumb_rect.max.x, thumb_rect.min.y + 32.0),
    );
    painter.rect_filled(strip, thumb_rounding, Color32::from_black_alpha(110));
    paint_status_pill(
        &painter,
        thumb_rect.shrink2(Vec2::new(8.0, 8.0)),
        status,
        record.supported,
    );

    // ── Body ──────────────────────────────────────────────────────────
    // (No status-colored left edge — the pill on the thumbnail conveys
    // the same info, and a square-cornered bar peeking past the card's
    // rounded border looked broken.)

    let pad_x = 12.0;
    let body = Rect::from_min_max(
        egui::pos2(rect.min.x + pad_x, thumb_rect.max.y + 10.0),
        egui::pos2(rect.max.x - pad_x, rect.max.y - 10.0),
    );

    let completed = matches!(status, EffectiveStatus::Library(Status::Completed));
    let title_color = if completed || absent {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_PRIMARY
    };
    let author_color = if completed {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_SECONDARY
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
            author_color,
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
    let waiting = get.is_some_and(|target| state.gets.is_waiting_for(target));
    let button_action = action_button(&mut sub_ui, record, status, get, waiting);

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
    } else if response.hovered() || absent {
        // Hover and absent share the same muted ring — absent acts as
        // the persistent "this row isn't in your library" affordance,
        // hover is the transient pointer feedback.
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.0_f32, theme::BORDER_MED),
            StrokeKind::Inside,
        );
    }

    // Context-menu picks take precedence — they're the user's
    // most recent gesture and shouldn't get clobbered by a stray
    // button-region click.
    menu_action.or(button_action)
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

fn paint_status_pill(
    painter: &egui::Painter,
    content: Rect,
    status: EffectiveStatus,
    supported: bool,
) {
    // Unsupported overrides whatever the linked-WAD status would say —
    // the user has explicitly removed this entry from the completion
    // game, so it shouldn't read "in progress" or "completed" either.
    let (text, color) = if supported {
        status_label_and_color(status)
    } else {
        ("unsupported", theme::TEXT_MUTED)
    };
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

/// What a card offers on its face. The rest is in its menu.
#[derive(Debug, PartialEq, Eq)]
enum CardButtons {
    Import,
    /// Get leads and Import sits beside it.
    GetOrImport(GetTarget),
    Get(GetTarget),
    Play(i64),
    Open(i64),
    None,
}

/// The one thing a card's face is for: what would get this entry closer to being played.
///
/// Import leads for an entry on idgames, which caco can fetch by itself. For one that is
/// not, the file is what's missing and only a browser can get it, so Get leads, with
/// Import beside it for the metadata on a day the forum lets caco in.
///
/// A WAD the user has finished with keeps Open, whether or not its file is here. A
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
                _ if still_playing => CardButtons::Play(wad_id),
                _ => CardButtons::Open(wad_id),
            }
        }
    }
}

fn action_button(
    ui: &mut egui::Ui,
    record: &CacowardRecord,
    status: EffectiveStatus,
    get: Option<GetTarget>,
    waiting: bool,
) -> Option<ActionRequest> {
    let get_label = if waiting { "Waiting\u{2026}" } else { "Get" };
    let mut action = None;
    match card_buttons(record, status, get) {
        CardButtons::Import => {
            if primary_button(ui, "Import").clicked() {
                action = Some(ActionRequest::ImportCacoward(record.id));
            }
        }
        CardButtons::GetOrImport(target) => {
            if get_button(ui, get_label).clicked() {
                action = Some(ActionRequest::Get(target));
            }
            if secondary_button(ui, "Import").clicked() {
                action = Some(ActionRequest::ImportCacoward(record.id));
            }
        }
        CardButtons::Get(target) => {
            if get_button(ui, get_label).clicked() {
                action = Some(ActionRequest::Get(target));
            }
        }
        CardButtons::Play(wad_id) => {
            if secondary_button(ui, "Play").clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }
        }
        CardButtons::Open(wad_id) => {
            if secondary_button(ui, "Open").clicked() {
                action = Some(ActionRequest::Play(wad_id));
            }
        }
        CardButtons::None => {}
    }
    action
}

/// [`primary_button`], saying what it does, since "Get" alone does not say how.
fn get_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    primary_button(ui, label).on_hover_text(
        "Opens the page for it in your browser, and watches your Downloads folder for the file.",
    )
}

fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(11.0)
                .strong()
                .color(Color32::from_rgb(0x1a, 0x0a, 0x04)),
        )
        .fill(theme::TEXT_ACCENT)
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
        .fill(theme::BG_LIGHT)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_MED))
        .corner_radius(4.0)
        .min_size(Vec2::new(0.0, 22.0)),
    )
}

fn draw_meter(ui: &mut egui::Ui, done: usize, total: usize, width: f32, swept: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(3), theme::BG_LIGHT);
    if total > 0 {
        let fill_w = rect.width() * (done as f32 / total as f32);
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        // Fill turns green when the category is swept — the meter and
        // the chip read the same status, so a glance is enough.
        let fill_color = if swept {
            theme::COLOR_SUCCESS
        } else {
            theme::TEXT_ACCENT
        };
        painter.rect_filled(fill_rect, CornerRadius::same(3), fill_color);
    }
}

/// Small gold "✓ SWEPT" chip — shown on category headers when every
/// entry in the section is `Library(Completed)`. The point is reward
/// surface: a single Mordeth Award sweep is a one-clickaway badge of
/// honour, a 12/12 Winners sweep is a year-long victory.
fn paint_swept_chip(ui: &mut egui::Ui) {
    let gold = Color32::from_rgb(0xd4, 0xa1, 0x4a);
    let bg = Color32::from_rgba_unmultiplied(gold.r(), gold.g(), gold.b(), 40);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(11.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("✓ SWEPT")
                    .color(gold)
                    .size(10.0)
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
                CardButtons::Open(3)
            );
            assert_eq!(
                card_buttons(&record, library(status), Some(wad)),
                CardButtons::Open(3)
            );
        }
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
        assert_eq!(hero_kicker(&entries, 2011), "ANNUAL CACOWARDS · 2 winners");
        assert_eq!(
            hero_kicker(&entries, 2023),
            "ANNUAL CACOWARDS · 1 winner · 1 runner-up · 1 honorable mention"
        );
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
    fn a_category_nobody_told_the_gui_about_still_gets_a_heading() {
        assert_eq!(category_label("winner"), "WINNERS");
        assert_eq!(category_label("adrians-pen"), "ADRIAN'S PEN");
        assert_eq!(
            category_label("spaceship-of-theseus"),
            "SPACESHIP OF THESEUS"
        );
    }
}

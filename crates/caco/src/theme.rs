use caco_core::db::Status;
use caco_core::db::cacowards::EffectiveStatus;
use egui::{Color32, Visuals};

// ---------------------------------------------------------------------------
// Surfaces. Warm, as in brown and amber, inspired by game launchers.
// ---------------------------------------------------------------------------

pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0x16, 0x10, 0x0c);
pub const BG_DARK: Color32 = Color32::from_rgb(0x1c, 0x14, 0x10);
pub const BG_MEDIUM: Color32 = Color32::from_rgb(0x20, 0x18, 0x10);
pub const BG_LIGHT: Color32 = Color32::from_rgb(0x26, 0x1c, 0x14);
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x2a, 0x18, 0x08);
/// Dialogs, and the bar along the top of the app.
pub const BG_WINDOW: Color32 = Color32::from_rgb(0x1a, 0x14, 0x10);
/// The banner over the library.
pub const BG_HERO: Color32 = Color32::from_rgb(0x22, 0x18, 0x0c);
/// Behind a message saying something went wrong.
pub const BG_ERROR: Color32 = Color32::from_rgb(0x2a, 0x0d, 0x0d);

pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x1e, 0x16);
pub const BORDER_MED: Color32 = Color32::from_rgb(0x3a, 0x2e, 0x24);

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xe8, 0xd8, 0xc8);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xa9, 0x9a, 0x8a);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x96, 0x86, 0x76);
pub const TEXT_ACCENT: Color32 = Color32::from_rgb(0xff, 0x66, 0x33);
/// What is readable on anything filled with [`TEXT_ACCENT`] or [`COLOR_ERROR`]. White is not:
/// that is 2.9:1 on the accent.
pub const TEXT_ON_ACCENT: Color32 = Color32::from_rgb(0x1a, 0x0a, 0x04);
pub const TEXT_TAG: Color32 = Color32::from_rgb(0xcc, 0x77, 0x44);
/// An award: the year's kicker, and a category swept.
pub const GOLD: Color32 = Color32::from_rgb(0xd4, 0xa1, 0x4a);

// ---------------------------------------------------------------------------
// What went well, and what did not
// ---------------------------------------------------------------------------

pub const COLOR_SUCCESS: Color32 = Color32::from_rgb(0x4f, 0xc9, 0x6f);
pub const COLOR_WARNING: Color32 = Color32::from_rgb(0xcc, 0xcc, 0x33);
pub const COLOR_ERROR: Color32 = Color32::from_rgb(0xe8, 0x60, 0x5a);

/// The badge on the banner saying how many secret maps there are.
pub const COLOR_SECRET_FILL: Color32 = Color32::from_rgb(0x99, 0x44, 0x22);

/// Over the app, behind a dialog.
pub const SCRIM: Color32 = Color32::from_black_alpha(160);

/// What goes around a dialog: see `dialogs::Dialog`.
pub fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_WINDOW)
        .corner_radius(12)
        .stroke(egui::Stroke::new(1.0_f32, BORDER_MED))
        .shadow(egui::Shadow {
            offset: [0, 8],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(128),
        })
}

// ---------------------------------------------------------------------------
// Status: one colour and one word each, for everything that shows one. See docs/ui.md.
// ---------------------------------------------------------------------------

const STATUS_UNPLAYED: Color32 = Color32::from_rgb(0xa3, 0x93, 0x82);
const STATUS_IN_PROGRESS: Color32 = Color32::from_rgb(0x5c, 0xa8, 0xf0);
/// Finished is a thing that went well, so it is that green, and nothing else is.
const STATUS_COMPLETED: Color32 = COLOR_SUCCESS;
/// Not the red of an error: giving up on a WAD is a decision, and nothing went wrong.
const STATUS_ABANDONED: Color32 = Color32::from_rgb(0xd0, 0x70, 0x8a);

/// How much of a status's colour goes into the background of its pill.
const PILL_TINT: f32 = 0.12;

/// How to show a status: its word, its colour, and what goes behind the word in a pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusStyle {
    pub word: &'static str,
    pub color: Color32,
    pub fill: Color32,
}

impl StatusStyle {
    fn new(word: &'static str, color: Color32) -> Self {
        Self {
            word,
            color,
            fill: blend(BG_MEDIUM, color, PILL_TINT),
        }
    }
}

pub fn status_style(status: Status) -> StatusStyle {
    StatusStyle::new(status.display_name(), status_color(status))
}

/// How to show a Cacoward entry: a status like any other if it is in the library, and two
/// more for what only an entry can be.
///
/// Unsupported wins over whatever the linked WAD says: the entry has been taken out of the
/// reckoning, so it should not read as in progress or completed either.
pub fn cacoward_style(status: EffectiveStatus, supported: bool) -> StatusStyle {
    match status {
        _ if !supported => StatusStyle::new("Unsupported", TEXT_SECONDARY),
        EffectiveStatus::Absent => StatusStyle::new("Not in Library", TEXT_SECONDARY),
        EffectiveStatus::Library(status) => status_style(status),
    }
}

pub fn status_color(status: Status) -> Color32 {
    match status {
        Status::Unplayed => STATUS_UNPLAYED,
        Status::InProgress => STATUS_IN_PROGRESS,
        Status::Completed => STATUS_COMPLETED,
        Status::Abandoned => STATUS_ABANDONED,
    }
}

pub fn status_display(status: Status) -> &'static str {
    status_style(status).word
}

pub fn status_query(status: Status) -> &'static str {
    match status {
        Status::Unplayed => "status:unplayed",
        Status::InProgress => "status:in-progress",
        Status::Completed => "status:completed",
        Status::Abandoned => "status:abandoned",
    }
}

/// A status as a pill, in a layout. `extra` goes after its word, for what the word leaves out.
pub fn status_pill(ui: &mut egui::Ui, style: StatusStyle, extra: &str) -> egui::Response {
    let text = format!("{}{extra}", style.word);
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), pill_font(), style.color);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + PILL_PADDING * 2.0, egui::Sense::hover());
    paint_pill(ui.painter(), rect, galley, style);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
    response
}

/// A status as a pill, wherever it is wanted on a card, with its left edge at `left` and
/// centred on `center_y`. Returns where it went.
pub fn paint_status_pill(
    painter: &egui::Painter,
    left: f32,
    center_y: f32,
    style: StatusStyle,
    extra: &str,
) -> egui::Rect {
    let galley = painter.layout_no_wrap(format!("{}{extra}", style.word), pill_font(), style.color);
    let size = galley.size() + PILL_PADDING * 2.0;
    let rect = egui::Rect::from_min_size(egui::pos2(left, center_y - size.y / 2.0), size);
    paint_pill(painter, rect, galley, style);
    rect
}

/// How wide a pill would be, to tell whether it fits beside something else.
pub fn status_pill_width(ui: &egui::Ui, style: StatusStyle, extra: &str) -> f32 {
    let galley =
        ui.painter()
            .layout_no_wrap(format!("{}{extra}", style.word), pill_font(), style.color);
    galley.size().x + PILL_PADDING.x * 2.0
}

const PILL_PADDING: egui::Vec2 = egui::vec2(8.0, 2.0);

fn pill_font() -> egui::FontId {
    egui::FontId::proportional(11.0)
}

fn paint_pill(
    painter: &egui::Painter,
    rect: egui::Rect,
    galley: std::sync::Arc<egui::Galley>,
    style: StatusStyle,
) {
    painter.rect_filled(rect, 6, style.fill);
    painter.galley(rect.min + PILL_PADDING, galley, style.color);
}

/// A line of text painted at `pos`, cut short with an ellipsis at `max_width`. Measured rather
/// than cut at so many characters, since "WWWW" and "iiii" are not the same width.
pub fn paint_truncated(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    font: egui::FontId,
    color: Color32,
    max_width: f32,
) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    painter.galley(pos, painter.layout_job(job), color);
}

/// `amount` of the way from one colour to another.
pub fn blend(from: Color32, to: Color32, amount: f32) -> Color32 {
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// The one thing a surface wants done next.
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    filled_button(text, TEXT_ACCENT)
}

/// The same, for what cannot be taken back.
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    filled_button(text, COLOR_ERROR)
}

/// [`primary_button`] for a row or a card, where it is one of many and sits beside small text.
pub fn small_primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.into())
            .size(11.0)
            .strong()
            .color(TEXT_ON_ACCENT),
    )
    .fill(TEXT_ACCENT)
}

fn filled_button(text: impl Into<String>, fill: Color32) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.into())
            .strong()
            .color(TEXT_ON_ACCENT),
    )
    .fill(fill)
}

/// Render a clickable status filter pill. Returns true if clicked.
pub fn filter_pill(
    ui: &mut egui::Ui,
    label: &str,
    is_active: bool,
    accent: Option<Color32>,
    count: usize,
) -> bool {
    let text = format!("{label} ({count})");

    let (fill, text_color, stroke) = if is_active {
        let c = accent.unwrap_or(TEXT_ACCENT);
        // Active: tinted background, bright text, accent border
        let bg = Color32::from_rgba_premultiplied(c.r() / 5, c.g() / 5, c.b() / 5, 255);
        (bg, c, egui::Stroke::new(1.0_f32, c))
    } else {
        // Inactive: subtle background, muted text, no visible border
        (BG_LIGHT, TEXT_SECONDARY, egui::Stroke::new(1.0_f32, BORDER))
    };

    let btn = egui::Button::new(egui::RichText::new(&text).size(11.5).color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(14);

    ui.add(btn).clicked()
}

pub fn severity_color(severity: crate::message::Severity) -> Color32 {
    match severity {
        crate::message::Severity::Info => TEXT_PRIMARY,
        crate::message::Severity::Warning => COLOR_WARNING,
        crate::message::Severity::Error => COLOR_ERROR,
    }
}

pub fn rating_stars(rating: Option<i32>) -> String {
    match rating {
        Some(r) if r > 0 => {
            let filled = r.min(5) as usize;
            let empty = 5 - filled;
            "\u{2605}".repeat(filled) + &"\u{2606}".repeat(empty)
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Where a title picture would go
// ---------------------------------------------------------------------------

/// How far apart the lines of the placeholder's hatching are.
const HATCH_SPACING: f32 = 12.0;

/// A WAD's title picture if there is one, and otherwise the placeholder.
pub fn paint_thumbnail(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: egui::CornerRadius,
    picture: Option<&egui::TextureHandle>,
) {
    let Some(picture) = picture else {
        paint_thumb_placeholder(painter, rect, rounding);
        return;
    };
    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.add(
        egui::epaint::RectShape::filled(rect, rounding, Color32::WHITE)
            .with_texture(picture.id(), whole),
    );
}

/// What a WAD with no picture shows: a quiet hatched tile rather than a picture of something.
///
/// It used to be a saturated gradient, a different one per WAD, with the title's first two
/// letters on it, right above the title. That made the WADs caco knows least about the
/// loudest thing on the page.
pub fn paint_thumb_placeholder(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: egui::CornerRadius,
) {
    painter.rect_filled(rect, rounding, BG_LIGHT);
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let stroke = egui::Stroke::new(1.0_f32, BORDER_MED);
    let span = rect.width() + rect.height();
    let mut offset = HATCH_SPACING;
    while offset < span {
        painter.line_segment(
            [
                rect.left_top() + egui::vec2(offset, 0.0),
                rect.left_top() + egui::vec2(0.0, offset),
            ],
            stroke,
        );
        offset += HATCH_SPACING;
    }
}

// ---------------------------------------------------------------------------
// UI helpers — pills, badges, section labels
// ---------------------------------------------------------------------------

/// Render an uppercase section label.
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.colored_label(
        TEXT_MUTED,
        egui::RichText::new(text.to_uppercase()).small().strong(),
    );
    ui.add_space(2.0);
}

// ---------------------------------------------------------------------------
// How much room there is
// ---------------------------------------------------------------------------

/// A window shorter than this has its banners collapsed to a line, so that what they are
/// banners for gets the room. `main.rs` allows down to 400.
pub const SHORT_WINDOW: f32 = 560.0;

pub fn is_short_window(ctx: &egui::Context) -> bool {
    ctx.content_rect().height() < SHORT_WINDOW
}

// ---------------------------------------------------------------------------
// Sidebar helpers
// ---------------------------------------------------------------------------

/// Render a sidebar navigation item. Returns true if clicked.
pub fn sidebar_nav_item(ui: &mut egui::Ui, label: &str, is_active: bool) -> bool {
    let height = if is_short_window(ui.ctx()) {
        28.0
    } else {
        36.0
    };
    let desired_size = egui::vec2(ui.available_width(), height);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    name_row(&response, label);

    let is_hovered = response.hovered();
    let painter = ui.painter();

    // Background
    if is_active || is_hovered {
        painter.rect_filled(rect, 0.0, if is_active { BG_MEDIUM } else { BG_DARK });
    }

    // Left accent border
    if is_active {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            0.0,
            TEXT_ACCENT,
        );
    }

    // Text
    let text_color = if is_active {
        TEXT_ACCENT
    } else if is_hovered {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    painter.text(
        rect.min + egui::vec2(20.0, (rect.height() - 14.0) / 2.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(14.0),
        text_color,
    );

    response.clicked()
}

/// Give a row painted by hand the name a button gets from its text. These rows only
/// exist as pixels otherwise, so a screen reader finds an unnamed target, and nothing
/// can find "Mirrors".
fn name_row(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
}

/// Render a compact sidebar row for a tool.
///
/// Same shape as [`sidebar_nav_item`] but shorter and quieter, because these open a dialog
/// rather than switching what the main panel shows. Crucially it sizes to
/// `ui.available_width()`, so they stack inside the panel instead of running off its edge.
pub fn sidebar_tool_item(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let desired_size = egui::vec2(ui.available_width(), 26.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    name_row(&response, label);

    let is_hovered = response.hovered();
    let painter = ui.painter();

    if is_hovered {
        painter.rect_filled(rect, 0.0, BG_DARK);
    }
    painter.text(
        egui::pos2(rect.min.x + 20.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if is_hovered {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        },
    );

    response
}

/// Render a sidebar collection item (playlist-style). Returns the response.
pub fn sidebar_collection_item(ui: &mut egui::Ui, name: &str, is_active: bool) -> egui::Response {
    let desired_size = egui::vec2(ui.available_width(), 28.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    name_row(&response, name);

    let is_hovered = response.hovered();
    let painter = ui.painter();

    // Background highlight
    if is_active {
        painter.rect_filled(rect, 0.0, BG_MEDIUM);
    } else if is_hovered {
        painter.rect_filled(rect, 0.0, BG_DARK);
    }

    // Left accent bar when active
    if is_active {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            0.0,
            TEXT_ACCENT,
        );
    }

    // List icon
    let icon_color = if is_active { TEXT_ACCENT } else { TEXT_MUTED };
    painter.text(
        egui::pos2(rect.min.x + 20.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "\u{2022}",
        egui::FontId::proportional(13.0),
        icon_color,
    );

    // Label
    let text_color = if is_active {
        TEXT_ACCENT
    } else if is_hovered {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    painter.text(
        egui::pos2(rect.min.x + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.0),
        text_color,
    );

    response
}

// ---------------------------------------------------------------------------
// Theme application
// ---------------------------------------------------------------------------

/// Apply the warm dark theme to the egui context.
pub fn apply_doom_theme(ctx: &egui::Context) {
    // Bundle Noto Sans Symbols 1 + 2 as fallbacks. The two fonts are designed
    // to complement each other: Symbols 2 covers dingbats (✓ ✗ ★), geometric
    // shapes (▲ ● ○), and misc symbols (☆ ⚠) — the common UI glyphs — while
    // Symbols 1 covers arrows and more esoteric blocks. Neither egui's
    // bundled Ubuntu-Light nor NotoEmoji include plain dingbats like U+2713,
    // so without this any such character renders as tofu.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "symbols2".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansSymbols2-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "symbols".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansSymbols-Regular.ttf"
        ))),
    );
    for family_name in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(family) = fonts.families.get_mut(&family_name) {
            family.push("symbols2".to_owned());
            family.push("symbols".to_owned());
        }
    }
    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();

    // Panel / window backgrounds
    visuals.panel_fill = BG_DARK;
    visuals.window_fill = BG_WINDOW;
    visuals.extreme_bg_color = BG_SIDEBAR;
    visuals.faint_bg_color = BG_MEDIUM;

    // Selection
    visuals.selection.bg_fill = BG_SELECTED;
    visuals.selection.stroke.color = TEXT_ACCENT;

    // Widget rounding — softer, more modern
    let rounding = egui::CornerRadius::same(8);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.inactive.corner_radius = rounding;
    visuals.widgets.hovered.corner_radius = rounding;
    visuals.widgets.active.corner_radius = rounding;

    // Widget styles — noninteractive
    visuals.widgets.noninteractive.bg_fill = BG_MEDIUM;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT_SECONDARY;
    visuals.widgets.noninteractive.bg_stroke.color = BORDER;

    // Widget styles — inactive (hoverable but not hovered)
    visuals.widgets.inactive.bg_fill = BG_LIGHT;
    visuals.widgets.inactive.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.inactive.bg_stroke.color = BORDER_MED;

    // Widget styles — hovered
    visuals.widgets.hovered.bg_fill = BG_LIGHT;
    visuals.widgets.hovered.fg_stroke.color = TEXT_ACCENT;
    visuals.widgets.hovered.bg_stroke.color = TEXT_ACCENT;

    // Widget styles — active (being clicked)
    visuals.widgets.active.bg_fill = BG_SELECTED;
    visuals.widgets.active.fg_stroke.color = TEXT_ACCENT;
    visuals.widgets.active.bg_stroke.color = TEXT_ACCENT;

    // Hyperlinks
    visuals.hyperlink_color = TEXT_ACCENT;

    // Striped table rows
    visuals.striped = true;

    // Window shadow
    visuals.window_shadow.offset = [0, 8];
    visuals.window_shadow.blur = 32;
    visuals.window_shadow.color = Color32::from_black_alpha(128);

    ctx.set_visuals(visuals);

    // Spacing defaults. `all_styles_mut` rather than the old single
    // `set_style`: 0.36 keeps a Style per theme, and spacing is not a
    // light/dark decision, so both have to carry the same values.
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance.
    fn luminance(color: Color32) -> f32 {
        let linear = |channel: u8| {
            let c = channel as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// The line for small text, in WCAG AA.
    const READABLE: f32 = 4.5;

    #[test]
    fn text_tokens_are_readable_on_every_surface() {
        let surfaces = [
            ("BG_SIDEBAR", BG_SIDEBAR),
            ("BG_DARK", BG_DARK),
            ("BG_MEDIUM", BG_MEDIUM),
            ("BG_LIGHT", BG_LIGHT),
            ("BG_SELECTED", BG_SELECTED),
            ("BG_WINDOW", BG_WINDOW),
            ("BG_HERO", BG_HERO),
        ];
        let text = [
            ("TEXT_PRIMARY", TEXT_PRIMARY),
            ("TEXT_SECONDARY", TEXT_SECONDARY),
            ("TEXT_MUTED", TEXT_MUTED),
            ("TEXT_ACCENT", TEXT_ACCENT),
            ("TEXT_TAG", TEXT_TAG),
            ("GOLD", GOLD),
            ("COLOR_SUCCESS", COLOR_SUCCESS),
            ("COLOR_WARNING", COLOR_WARNING),
            ("COLOR_ERROR", COLOR_ERROR),
            ("STATUS_UNPLAYED", STATUS_UNPLAYED),
            ("STATUS_IN_PROGRESS", STATUS_IN_PROGRESS),
            ("STATUS_COMPLETED", STATUS_COMPLETED),
            ("STATUS_ABANDONED", STATUS_ABANDONED),
        ];
        for (surface_name, surface) in surfaces {
            for (text_name, color) in text {
                let ratio = contrast(color, surface);
                assert!(
                    ratio >= READABLE,
                    "{text_name} on {surface_name} is only {ratio:.2}:1"
                );
            }
        }
    }

    #[test]
    fn a_status_is_readable_in_its_own_pill() {
        let library = Status::ALL.iter().copied().map(status_style);
        let cacoward = [
            cacoward_style(EffectiveStatus::Absent, true),
            cacoward_style(EffectiveStatus::Absent, false),
        ];
        for style in library.chain(cacoward) {
            let ratio = contrast(style.color, style.fill);
            assert!(
                ratio >= READABLE,
                "{} is only {ratio:.2}:1 on its pill",
                style.word
            );
        }
    }

    #[test]
    fn what_goes_on_a_filled_button_is_readable() {
        assert!(contrast(TEXT_ON_ACCENT, TEXT_ACCENT) >= READABLE);
        assert!(contrast(TEXT_ON_ACCENT, COLOR_ERROR) >= READABLE);
    }

    /// The issue this came from: green used to mean in progress in the library and completed
    /// on a Cacoward card.
    #[test]
    fn each_status_has_a_colour_and_a_word_of_its_own() {
        let styles: Vec<StatusStyle> = Status::ALL.iter().copied().map(status_style).collect();
        for (i, a) in styles.iter().enumerate() {
            for b in &styles[i + 1..] {
                assert_ne!(a.color, b.color, "{} and {}", a.word, b.word);
                assert_ne!(a.word, b.word);
            }
        }
        for style in &styles {
            assert_ne!(
                style.color, COLOR_ERROR,
                "{} reads as a failure",
                style.word
            );
            assert_ne!(
                style.color, COLOR_WARNING,
                "{} reads as trouble",
                style.word
            );
        }
    }

    #[test]
    fn a_cacoward_reads_like_its_wad_unless_it_is_out_of_the_reckoning() {
        for &status in Status::ALL {
            assert_eq!(
                cacoward_style(EffectiveStatus::Library(status), true),
                status_style(status)
            );
            assert_eq!(
                cacoward_style(EffectiveStatus::Library(status), false).word,
                "Unsupported"
            );
        }
        assert_eq!(
            cacoward_style(EffectiveStatus::Absent, true).word,
            "Not in Library"
        );
    }
}

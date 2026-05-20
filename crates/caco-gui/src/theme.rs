use caco_core::db::Status;
use egui::{Color32, Visuals};

// ---------------------------------------------------------------------------
// Warm palette (brown/amber tones inspired by game launchers)
// ---------------------------------------------------------------------------

pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0x16, 0x10, 0x0c);
pub const BG_DARK: Color32 = Color32::from_rgb(0x1c, 0x14, 0x10);
pub const BG_MEDIUM: Color32 = Color32::from_rgb(0x20, 0x18, 0x10);
pub const BG_LIGHT: Color32 = Color32::from_rgb(0x26, 0x1c, 0x14);
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x2a, 0x18, 0x08);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xe8, 0xd8, 0xc8);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x8a, 0x7a, 0x6a);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x5a, 0x4a, 0x3a);
pub const TEXT_ACCENT: Color32 = Color32::from_rgb(0xff, 0x66, 0x33);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x1e, 0x16);
pub const BORDER_MED: Color32 = Color32::from_rgb(0x3a, 0x2e, 0x24);

// Semantic colors for notifications / severity indicators
pub const COLOR_SUCCESS: Color32 = Color32::from_rgb(0x33, 0xcc, 0x33);
pub const COLOR_WARNING: Color32 = Color32::from_rgb(0xcc, 0xcc, 0x33);
pub const COLOR_ERROR: Color32 = Color32::from_rgb(0xcc, 0x44, 0x44);

// Progress bar: secret map badge background
pub const COLOR_SECRET_FILL: Color32 = Color32::from_rgb(0x99, 0x44, 0x22);

// ---------------------------------------------------------------------------
// Status colors
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Status helpers
// ---------------------------------------------------------------------------

pub fn status_color(status: Status) -> Color32 {
    match status {
        Status::Unplayed => Color32::from_rgb(0x33, 0x66, 0xcc),
        Status::InProgress => Color32::from_rgb(0x33, 0xcc, 0x33),
        Status::Completed => Color32::from_rgb(0x80, 0x80, 0x80),
        Status::Abandoned => Color32::from_rgb(0xcc, 0x33, 0x33),
    }
}

pub fn status_bg(status: Status) -> Color32 {
    match status {
        Status::Unplayed => Color32::from_rgb(0x0d, 0x14, 0x2a),
        Status::InProgress => Color32::from_rgb(0x0d, 0x2a, 0x0d),
        Status::Completed => Color32::from_rgb(0x1a, 0x1a, 0x1a),
        Status::Abandoned => Color32::from_rgb(0x2a, 0x0d, 0x0d),
    }
}

pub fn status_display(status: Status) -> &'static str {
    status.display_name()
}

pub fn status_query(status: Status) -> &'static str {
    match status {
        Status::Unplayed => "status:unplayed",
        Status::InProgress => "status:in-progress",
        Status::Completed => "status:completed",
        Status::Abandoned => "status:abandoned",
    }
}

/// Render a status value as a colored pill badge.
pub fn status_pill(ui: &mut egui::Ui, status: Status) {
    let color = status_color(status);
    let label = status_display(status);
    let bg = status_bg(status);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.colored_label(color, egui::RichText::new(label).small().strong());
        });
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
        (bg, c, egui::Stroke::new(1.0, c))
    } else {
        // Inactive: subtle background, muted text, no visible border
        (BG_LIGHT, TEXT_SECONDARY, egui::Stroke::new(1.0, BORDER))
    };

    let btn = egui::Button::new(egui::RichText::new(&text).size(11.5).color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(14);

    ui.add(btn).clicked()
}

pub fn severity_color(severity: crate::message::Severity) -> Color32 {
    match severity {
        crate::message::Severity::Info => COLOR_SUCCESS,
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
// Placeholder thumbnail colors (cycled by WAD id)
// ---------------------------------------------------------------------------

pub const THUMB_COLORS: &[(Color32, Color32, Color32)] = &[
    // (gradient_start, gradient_end, initials_color)
    (
        Color32::from_rgb(0x3a, 0x08, 0x08),
        Color32::from_rgb(0x66, 0x10, 0x10),
        Color32::from_rgb(0x88, 0x22, 0x22),
    ),
    (
        Color32::from_rgb(0x08, 0x2a, 0x14),
        Color32::from_rgb(0x0a, 0x44, 0x22),
        Color32::from_rgb(0x22, 0x66, 0x44),
    ),
    (
        Color32::from_rgb(0x0a, 0x0a, 0x2a),
        Color32::from_rgb(0x1a, 0x1a, 0x55),
        Color32::from_rgb(0x33, 0x44, 0x99),
    ),
    (
        Color32::from_rgb(0x2a, 0x1a, 0x08),
        Color32::from_rgb(0x55, 0x33, 0x08),
        Color32::from_rgb(0x88, 0x55, 0x22),
    ),
    (
        Color32::from_rgb(0x2a, 0x08, 0x2a),
        Color32::from_rgb(0x4a, 0x0e, 0x4e),
        Color32::from_rgb(0x77, 0x33, 0x88),
    ),
    (
        Color32::from_rgb(0x2a, 0x2a, 0x08),
        Color32::from_rgb(0x4a, 0x44, 0x08),
        Color32::from_rgb(0x88, 0x77, 0x22),
    ),
];

/// Get placeholder thumbnail colors for a WAD id.
pub fn thumb_colors(wad_id: i64) -> (Color32, Color32, Color32) {
    THUMB_COLORS[(wad_id as usize) % THUMB_COLORS.len()]
}

// ---------------------------------------------------------------------------
// UI helpers — pills, badges, section labels
// ---------------------------------------------------------------------------

/// Render a tag as a small accent-tinted pill.
pub fn tag_pill(ui: &mut egui::Ui, tag: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(0x26, 0x1c, 0x14))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.colored_label(
                Color32::from_rgb(0xcc, 0x77, 0x44),
                egui::RichText::new(tag).small(),
            );
        });
}

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
// Sidebar helpers
// ---------------------------------------------------------------------------

/// Render a sidebar navigation item. Returns true if clicked.
pub fn sidebar_nav_item(ui: &mut egui::Ui, label: &str, is_active: bool) -> bool {
    let desired_size = egui::vec2(ui.available_width(), 36.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

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

/// Render a sidebar collection item (playlist-style). Returns the response.
pub fn sidebar_collection_item(ui: &mut egui::Ui, name: &str, is_active: bool) -> egui::Response {
    let desired_size = egui::vec2(ui.available_width(), 28.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

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
    visuals.window_fill = Color32::from_rgb(0x1a, 0x14, 0x10);
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

    // Spacing defaults
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    ctx.set_style(style);
}

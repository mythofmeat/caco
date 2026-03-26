use egui::{Color32, Visuals};

// ---------------------------------------------------------------------------
// Doom palette (matches Python gui/theme.py DOOM_PALETTE)
// ---------------------------------------------------------------------------

pub const BG_DARK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const BG_MEDIUM: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const BG_LIGHT: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x50, 0x28, 0x22);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xe0, 0xe0, 0xe0);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
pub const TEXT_ACCENT: Color32 = Color32::from_rgb(0xff, 0x66, 0x33);
pub const BORDER: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);

// Semantic colors for notifications / severity indicators
pub const COLOR_SUCCESS: Color32 = Color32::from_rgb(0x33, 0xcc, 0x33);
pub const COLOR_WARNING: Color32 = Color32::from_rgb(0xcc, 0xcc, 0x33);
pub const COLOR_ERROR: Color32 = Color32::from_rgb(0xcc, 0x33, 0x33);

// ---------------------------------------------------------------------------
// Status colors (matches TUI theme.rs and Python STATUS_METADATA)
// ---------------------------------------------------------------------------

pub fn status_color(status: &str) -> Color32 {
    match status {
        "to-play" => Color32::from_rgb(0x33, 0x66, 0xcc),
        "backlog" => Color32::from_rgb(0xcc, 0xcc, 0x33),
        "playing" => Color32::from_rgb(0x33, 0xcc, 0x33),
        "finished" => Color32::from_rgb(0x80, 0x80, 0x80),
        "abandoned" => Color32::from_rgb(0xcc, 0x33, 0x33),
        "awaiting-update" => Color32::from_rgb(0xcc, 0x33, 0xcc),
        _ => TEXT_PRIMARY,
    }
}

pub fn status_display(status: &str) -> &str {
    match status {
        "to-play" => "To Play",
        "backlog" => "Backlog",
        "playing" => "Playing",
        "finished" => "Finished",
        "abandoned" => "Abandoned",
        "awaiting-update" => "Awaiting Update",
        _ => status,
    }
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
// UI helpers — pills, badges, section labels
// ---------------------------------------------------------------------------

/// Render a status value as a colored pill badge.
pub fn status_pill(ui: &mut egui::Ui, status: &str) {
    let color = status_color(status);
    let label = status_display(status);
    let bg = Color32::from_rgba_premultiplied(color.r() / 4, color.g() / 4, color.b() / 4, 180);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.colored_label(color, egui::RichText::new(label).small());
        });
}

/// Render a tag as a small accent-tinted pill.
pub fn tag_pill(ui: &mut egui::Ui, tag: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgba_premultiplied(0x33, 0x14, 0x0a, 160))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.colored_label(TEXT_ACCENT, egui::RichText::new(tag).small());
        });
}

/// Render an uppercase section label.
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.colored_label(
        TEXT_SECONDARY,
        egui::RichText::new(text.to_uppercase()).small().strong(),
    );
    ui.add_space(2.0);
}

// ---------------------------------------------------------------------------
// Theme application
// ---------------------------------------------------------------------------

/// Apply the Doom-inspired dark theme to the egui context.
pub fn apply_doom_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    // Panel / window backgrounds
    visuals.panel_fill = BG_DARK;
    visuals.window_fill = BG_MEDIUM;
    visuals.extreme_bg_color = BG_DARK;
    visuals.faint_bg_color = BG_MEDIUM;

    // Selection
    visuals.selection.bg_fill = BG_SELECTED;
    visuals.selection.stroke.color = TEXT_ACCENT;

    // Widget rounding — softer, more modern
    let rounding = egui::CornerRadius::same(4);
    visuals.widgets.noninteractive.corner_radius = rounding;
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
    visuals.widgets.inactive.bg_stroke.color = BORDER;

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

    ctx.set_visuals(visuals);

    // Spacing defaults
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    ctx.set_style(style);
}

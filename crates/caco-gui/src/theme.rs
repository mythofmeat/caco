use egui::{Color32, Visuals};

// ---------------------------------------------------------------------------
// Doom palette (matches Python gui/theme.py DOOM_PALETTE)
// ---------------------------------------------------------------------------

pub const BG_DARK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const BG_MEDIUM: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const BG_LIGHT: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x4a, 0x2a, 0x2a);
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
}

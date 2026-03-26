use ratatui::style::{Color, Modifier, Style};

/// Map a status string to its ratatui Color (from STATUS_METADATA hex values).
pub fn status_color(status: &str) -> Color {
    match status {
        "to-play" => Color::Rgb(0x33, 0x66, 0xcc),
        "backlog" => Color::Rgb(0xcc, 0xcc, 0x33),
        "playing" => Color::Rgb(0x33, 0xcc, 0x33),
        "finished" => Color::Rgb(0x80, 0x80, 0x80),
        "abandoned" => Color::Rgb(0xcc, 0x33, 0x33),
        "awaiting-update" => Color::Rgb(0xcc, 0x33, 0xcc),
        _ => Color::Reset,
    }
}

/// Return a Style with the status foreground color.
pub fn status_style(status: &str) -> Style {
    Style::default().fg(status_color(status))
}

/// Human-readable display name for a status string.
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

/// Style for the selected/highlighted row.
pub fn highlight_style() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

/// Style for a tab label.
pub fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

/// Style for borders.
pub fn border_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Style for key hints in the status bar.
pub fn key_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Style for key descriptions in the status bar.
pub fn desc_style() -> Style {
    Style::default().fg(Color::Gray)
}

/// Style for notification messages.
pub fn notify_style(severity: &str) -> Style {
    match severity {
        "error" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "warning" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Green),
    }
}

/// Title header style.
pub fn title_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

/// Dim text style.
pub fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Rating stars string for a given rating (0-5).
pub fn rating_stars(rating: Option<i32>) -> String {
    match rating {
        Some(r) if r > 0 => "★".repeat(r as usize) + &"☆".repeat((5 - r) as usize),
        _ => String::new(),
    }
}

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme;

/// Render a status bar with key hints.
pub fn render_status_bar(hints: &[(&str, &str)], frame: &mut Frame, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    for (i, (key, desc)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(ratatui::style::Color::DarkGray)));
        }
        spans.push(Span::styled(*key, theme::key_style()));
        spans.push(Span::styled(format!(" {desc}"), theme::desc_style()));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

/// Render a status mode indicator bar.
pub fn render_status_mode_bar(frame: &mut Frame, area: Rect) {
    let hints = &[
        ("u", "unplayed"),
        ("p", "in-progress"),
        ("c", "completed"),
        ("a", "abandoned"),
        ("Esc", "cancel"),
    ];

    let mut spans = vec![Span::styled(
        "SET STATUS: ",
        Style::default()
            .fg(ratatui::style::Color::Yellow)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )];

    for (i, (key, desc)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(*key, theme::key_style()));
        spans.push(Span::styled(format!("={desc}"), theme::desc_style()));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(
        Style::default().bg(ratatui::style::Color::DarkGray),
    );
    frame.render_widget(paragraph, area);
}

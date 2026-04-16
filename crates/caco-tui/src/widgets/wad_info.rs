use caco_core::db::models::WadRecord;
use caco_core::db::sessions::WadStats;
use caco_core::player::format_duration;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::theme;

/// State for the WAD info panel widget.
pub struct WadInfoState {
    pub current_wad_id: Option<i64>,
}

impl Default for WadInfoState {
    fn default() -> Self {
        Self::new()
    }
}

impl WadInfoState {
    pub fn new() -> Self {
        Self {
            current_wad_id: None,
        }
    }
}

/// Render the WAD info panel showing details for the selected WAD.
pub fn render_wad_info(
    state: &mut WadInfoState,
    wad: Option<&WadRecord>,
    stats: Option<&WadStats>,
    frame: &mut Frame,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style())
        .title(" Info ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(wad) = wad else {
        let msg = Paragraph::new("No WAD selected").style(theme::dim_style());
        frame.render_widget(msg, inner);
        return;
    };

    state.current_wad_id = Some(wad.id);

    let mut lines = Vec::new();

    // Title
    lines.push(Line::from(Span::styled(&wad.title, theme::title_style())));
    lines.push(Line::from(""));

    // Author / Year
    let mut meta_parts = Vec::new();
    if let Some(ref author) = wad.author {
        meta_parts.push(author.clone());
    }
    if let Some(year) = wad.year {
        meta_parts.push(year.to_string());
    }
    if !meta_parts.is_empty() {
        lines.push(Line::from(meta_parts.join(" · ")));
    }

    // Status
    let status_display = theme::status_display(&wad.status);
    lines.push(Line::from(Span::styled(
        status_display,
        theme::status_style(&wad.status).add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(""));

    // Rating
    let stars = theme::rating_stars(wad.rating);
    if !stars.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Rating: ", theme::dim_style()),
            Span::styled(stars, Style::default().fg(ratatui::style::Color::Yellow)),
        ]));
    }

    // Play stats
    if let Some(s) = stats {
        if s.playtime > 0 {
            lines.push(Line::from(vec![
                Span::styled("Playtime: ", theme::dim_style()),
                Span::raw(format_duration(s.playtime)),
            ]));
        }
        if s.session_count > 0 {
            lines.push(Line::from(vec![
                Span::styled("Sessions: ", theme::dim_style()),
                Span::raw(s.session_count.to_string()),
            ]));
        }
        if s.times_beaten > 0 {
            lines.push(Line::from(vec![
                Span::styled("Beaten: ", theme::dim_style()),
                Span::raw(format!("{}×", s.times_beaten)),
            ]));
        }
        if let Some(ref last) = s.last_played {
            // Show just the date part
            let date = last.split('T').next().unwrap_or(last);
            let date = date.split(' ').next().unwrap_or(date);
            lines.push(Line::from(vec![
                Span::styled("Last played: ", theme::dim_style()),
                Span::raw(date.to_string()),
            ]));
        }
    }

    // Tags
    if !wad.tags.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Tags: ", theme::dim_style()),
            Span::raw(wad.tags.join(", ")),
        ]));
    }

    // Complevel
    if let Some(cl) = wad.complevel {
        let name = caco_core::complevel::complevel_name(Some(cl));
        lines.push(Line::from(vec![
            Span::styled("Complevel: ", theme::dim_style()),
            Span::raw(format!("{cl} ({name})")),
        ]));
    }

    // IWAD
    if let Some(ref iwad) = wad.custom_iwad {
        lines.push(Line::from(vec![
            Span::styled("IWAD: ", theme::dim_style()),
            Span::raw(iwad.clone()),
        ]));
    }

    // Config profile
    if let Some(ref config) = wad.custom_config {
        lines.push(Line::from(vec![
            Span::styled("Config: ", theme::dim_style()),
            Span::raw(config.clone()),
        ]));
    }

    // Description snippet
    if let Some(ref desc) = wad.description {
        let snippet: String = desc.chars().take(200).collect();
        if !snippet.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(snippet, theme::dim_style())));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, inner);
}

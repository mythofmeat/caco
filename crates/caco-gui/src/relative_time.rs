use chrono::{Local, NaiveDateTime};

/// Format a timestamp as relative time (e.g., "2h ago", "3d ago").
/// Used in the WAD table column.
pub fn relative_time(dt: &NaiveDateTime) -> String {
    let now = Local::now().naive_local();
    let duration = now.signed_duration_since(*dt);

    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;

    if minutes < 1 {
        "just now".to_string()
    } else if minutes < 60 {
        format!("{}m ago", minutes)
    } else if hours < 24 {
        format!("{}h ago", hours)
    } else if days < 14 {
        format!("{}d ago", days)
    } else if weeks < 9 {
        format!("{}w ago", weeks)
    } else if months < 12 {
        format!("{}mo ago", months)
    } else {
        format!("{}y ago", years)
    }
}

/// Format a timestamp as relative time with full date in parentheses.
/// Used in the detail panel (e.g., "3 days ago (2026-03-23)").
pub fn relative_time_full(dt: &NaiveDateTime) -> String {
    let now = Local::now().naive_local();
    let duration = now.signed_duration_since(*dt);

    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;

    let relative = if minutes < 1 {
        "just now".to_string()
    } else if minutes < 60 {
        let label = if minutes == 1 { "minute" } else { "minutes" };
        format!("{} {} ago", minutes, label)
    } else if hours < 24 {
        let label = if hours == 1 { "hour" } else { "hours" };
        format!("{} {} ago", hours, label)
    } else if days < 14 {
        let label = if days == 1 { "day" } else { "days" };
        format!("{} {} ago", days, label)
    } else if weeks < 9 {
        let label = if weeks == 1 { "week" } else { "weeks" };
        format!("{} {} ago", weeks, label)
    } else if months < 12 {
        let label = if months == 1 { "month" } else { "months" };
        format!("{} {} ago", months, label)
    } else {
        let label = if years == 1 { "year" } else { "years" };
        format!("{} {} ago", years, label)
    };

    let date = dt.format("%Y-%m-%d").to_string();
    format!("{} ({})", relative, date)
}

/// Parse an ISO 8601 timestamp string into NaiveDateTime.
pub fn parse_timestamp(ts: &str) -> Option<NaiveDateTime> {
    // Try common formats: "2024-06-15T18:30:00" or "2024-06-15 18:30:00"
    NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S"))
        .ok()
}

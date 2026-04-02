use chrono::{Local, NaiveDateTime};

struct RelativeDuration {
    minutes: i64,
    hours: i64,
    days: i64,
    weeks: i64,
    months: i64,
    years: i64,
}

fn compute_relative_duration(dt: &NaiveDateTime) -> RelativeDuration {
    let now = Local::now().naive_local();
    let duration = now.signed_duration_since(*dt);
    let days = duration.num_days();
    RelativeDuration {
        minutes: duration.num_minutes(),
        hours: duration.num_hours(),
        days,
        weeks: days / 7,
        months: days / 30,
        years: days / 365,
    }
}

/// Format a timestamp as relative time (e.g., "2h ago", "3d ago").
/// Used in the WAD table column.
pub fn relative_time(dt: &NaiveDateTime) -> String {
    let d = compute_relative_duration(dt);

    if d.minutes < 1 {
        "just now".to_string()
    } else if d.minutes < 60 {
        format!("{}m ago", d.minutes)
    } else if d.hours < 24 {
        format!("{}h ago", d.hours)
    } else if d.days < 14 {
        format!("{}d ago", d.days)
    } else if d.weeks < 9 {
        format!("{}w ago", d.weeks)
    } else if d.months < 12 {
        format!("{}mo ago", d.months)
    } else {
        format!("{}y ago", d.years)
    }
}

/// Format a timestamp as relative time with full date in parentheses.
/// Used in the detail panel (e.g., "3 days ago (2026-03-23)").
pub fn relative_time_full(dt: &NaiveDateTime) -> String {
    let d = compute_relative_duration(dt);

    let relative = if d.minutes < 1 {
        "just now".to_string()
    } else if d.minutes < 60 {
        let label = if d.minutes == 1 { "minute" } else { "minutes" };
        format!("{} {} ago", d.minutes, label)
    } else if d.hours < 24 {
        let label = if d.hours == 1 { "hour" } else { "hours" };
        format!("{} {} ago", d.hours, label)
    } else if d.days < 14 {
        let label = if d.days == 1 { "day" } else { "days" };
        format!("{} {} ago", d.days, label)
    } else if d.weeks < 9 {
        let label = if d.weeks == 1 { "week" } else { "weeks" };
        format!("{} {} ago", d.weeks, label)
    } else if d.months < 12 {
        let label = if d.months == 1 { "month" } else { "months" };
        format!("{} {} ago", d.months, label)
    } else {
        let label = if d.years == 1 { "year" } else { "years" };
        format!("{} {} ago", d.years, label)
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

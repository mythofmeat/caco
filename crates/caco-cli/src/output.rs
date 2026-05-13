//! Output formatting: table, plain (TSV), and JSON rendering.

use std::collections::HashMap;

use comfy_table::{Cell, CellAlignment, Color, Table, presets};

use caco_core::db::{
    CacowardRecord, CompletionRecord, Id24Record, IwadRecord, SessionRecord, StatsSnapshot, Status,
    WadCompanionRecord, WadRecord, WadStats,
};
use caco_core::player::format_duration;

/// Output format for CLI commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Table,
    Plain,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "plain" => Ok(OutputFormat::Plain),
            "json" => Ok(OutputFormat::Json),
            other => Err(format!("unknown output format: {other}")),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Plain => write!(f, "plain"),
            OutputFormat::Json => write!(f, "json"),
        }
    }
}

// ---------------------------------------------------------------------------
// WAD list rendering
// ---------------------------------------------------------------------------

pub fn render_wad_list(wads: &[WadRecord], stats: &HashMap<i64, WadStats>, format: OutputFormat) {
    match format {
        OutputFormat::Table => render_wad_list_table(wads, stats),
        OutputFormat::Plain => render_wad_list_plain(wads, stats),
        OutputFormat::Json => render_wad_list_json(wads, stats),
    }
}

fn render_wad_list_table(wads: &[WadRecord], stats: &HashMap<i64, WadStats>) {
    if wads.is_empty() {
        println!("No WADs found.");
        return;
    }

    let mut table = Table::new();
    table.load_preset(presets::NOTHING).set_header(vec![
        Cell::new("ID").fg(Color::DarkGrey),
        Cell::new("Title").fg(Color::DarkGrey),
        Cell::new("Author").fg(Color::DarkGrey),
        Cell::new("Status").fg(Color::DarkGrey),
        Cell::new("Beaten").fg(Color::DarkGrey),
        Cell::new("Playtime").fg(Color::DarkGrey),
        Cell::new("Last Played").fg(Color::DarkGrey),
    ]);

    for wad in wads {
        let ws = stats.get(&wad.id);
        let beaten = ws.map_or(0, |s| s.times_beaten);
        let playtime = ws.map_or(0, |s| s.playtime);
        let last = ws
            .and_then(|s| s.last_played.as_deref())
            .map(format_timestamp)
            .unwrap_or_default();

        let status_display = wad.status.display_name();

        let author = truncate_str(wad.author.as_deref().unwrap_or(""), 30);

        table.add_row(vec![
            Cell::new(wad.id).set_alignment(CellAlignment::Right),
            Cell::new(&wad.title),
            Cell::new(&author),
            Cell::new(status_display),
            Cell::new(beaten).set_alignment(CellAlignment::Right),
            Cell::new(if playtime > 0 {
                format_duration(playtime)
            } else {
                String::new()
            })
            .set_alignment(CellAlignment::Right),
            Cell::new(&last),
        ]);
    }

    println!("{table}");
    println!("{} WAD(s)", wads.len());
}

fn render_wad_list_plain(wads: &[WadRecord], stats: &HashMap<i64, WadStats>) {
    println!("ID\tTitle\tAuthor\tStatus\tBeaten\tPlaytime\tLastPlayed");
    for wad in wads {
        let ws = stats.get(&wad.id);
        let beaten = ws.map_or(0, |s| s.times_beaten);
        let playtime = ws.map_or(0, |s| s.playtime);
        let last = ws
            .and_then(|s| s.last_played.as_deref())
            .map(format_timestamp)
            .unwrap_or_default();

        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            wad.id,
            wad.title,
            wad.author.as_deref().unwrap_or(""),
            wad.status,
            beaten,
            if playtime > 0 {
                format_duration(playtime)
            } else {
                String::new()
            },
            last,
        );
    }
}

fn render_wad_list_json(wads: &[WadRecord], stats: &HashMap<i64, WadStats>) {
    let items: Vec<serde_json::Value> = wads
        .iter()
        .map(|wad| {
            let ws = stats.get(&wad.id);
            let mut val = serde_json::to_value(wad).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(ref mut map) = val {
                map.insert(
                    "playtime".to_string(),
                    serde_json::json!(ws.map_or(0, |s| s.playtime)),
                );
                map.insert(
                    "times_beaten".to_string(),
                    serde_json::json!(ws.map_or(0, |s| s.times_beaten)),
                );
                map.insert(
                    "session_count".to_string(),
                    serde_json::json!(ws.map_or(0, |s| s.session_count)),
                );
                map.insert(
                    "last_played".to_string(),
                    serde_json::json!(ws.and_then(|s| s.last_played.as_deref())),
                );
            }
            val
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&items).unwrap_or_default()
    );
}

// ---------------------------------------------------------------------------
// WAD detail rendering
// ---------------------------------------------------------------------------

pub fn render_wad_info(
    wad: &WadRecord,
    stats: &WadStats,
    completions: &[CompletionRecord],
    companions: &[WadCompanionRecord],
    format: OutputFormat,
) {
    match format {
        OutputFormat::Table => render_wad_info_table(wad, stats, completions, companions),
        OutputFormat::Plain => render_wad_info_plain(wad, stats, completions, companions),
        OutputFormat::Json => render_wad_info_json(wad, stats, completions, companions),
    }
}

fn render_wad_info_table(
    wad: &WadRecord,
    stats: &WadStats,
    completions: &[CompletionRecord],
    companions: &[WadCompanionRecord],
) {
    println!("{} (ID: {})", wad.title, wad.id);
    println!();

    if let Some(author) = &wad.author {
        println!("  Author:      {author}");
    }
    if let Some(year) = wad.year {
        println!("  Year:        {year}");
    }

    println!("  Status:      {}", wad.status.display_name());

    if let Some(version) = &wad.version {
        println!("  Version:     {version}");
    }
    if let Some(rating) = wad.rating {
        println!("  Rating:      {rating}/5");
    }
    if !wad.tags.is_empty() {
        println!("  Tags:        {}", wad.tags.join(", "));
    }

    println!("  Source:      {}", wad.source_type);
    if let Some(url) = &wad.source_url {
        println!("  URL:         {url}");
    }
    if let Some(idgames_id) = &wad.idgames_id {
        println!("  idgames ID:  {idgames_id}");
    }

    if let Some(desc) = &wad.description {
        println!();
        // Truncate long descriptions
        let lines: Vec<&str> = desc.lines().collect();
        if lines.len() > 10 {
            for line in &lines[..10] {
                println!("  {line}");
            }
            println!("  ... ({} more lines)", lines.len() - 10);
        } else {
            for line in &lines {
                println!("  {line}");
            }
        }
    }

    if stats.session_count > 0 {
        println!();
        println!("  Playtime:    {}", format_duration(stats.playtime));
        println!("  Sessions:    {}", stats.session_count);
        if let Some(last) = &stats.last_played {
            println!("  Last played: {}", format_timestamp(last));
        }
    }

    if let Some(notes) = &wad.notes {
        println!();
        println!("  Notes:       {notes}");
    }

    if !completions.is_empty() {
        println!();
        println!("  Completions ({}):", completions.len());
        for comp in completions {
            let ts = format_timestamp(&comp.completed_at);
            let has_stats = if comp.stats_snapshot.is_some() {
                " *"
            } else {
                ""
            };
            let notes = comp
                .notes
                .as_deref()
                .map(|n| format!(" - {n}"))
                .unwrap_or_default();
            println!("    {ts}{has_stats}{notes}");
        }
    }

    // Custom play config
    let has_custom = wad.custom_iwad.is_some()
        || wad.custom_sourceport.is_some()
        || wad.required_sourceport_family.is_some()
        || wad.complevel.is_some()
        || wad.custom_config.is_some()
        || wad.custom_args.is_some()
        || !companions.is_empty();

    if has_custom {
        println!();
        println!("  Play config:");
        if let Some(iwad) = &wad.custom_iwad {
            println!("    IWAD:       {iwad}");
        }
        if let Some(port) = &wad.custom_sourceport {
            println!("    Sourceport override: {port}");
        }
        if let Some(family) = &wad.required_sourceport_family {
            println!("    Compatibility family: {family}");
        }
        if let Some(cl) = wad.complevel {
            let label = caco_core::complevel::complevel_name(Some(cl));
            if label == "Unknown" {
                println!("    Complevel:  {cl}");
            } else {
                println!("    Complevel:  {cl} ({label})");
            }
        }
        if let Some(cfg) = &wad.custom_config {
            println!("    Config:     {cfg}");
        }
        if let Some(args) = &wad.custom_args {
            println!("    Args:       {args}");
        }
        for c in companions {
            let status = if c.enabled { "" } else { " (disabled)" };
            println!("    File:       {}{status}", c.filename);
        }
    }
}

fn render_wad_info_plain(
    wad: &WadRecord,
    stats: &WadStats,
    completions: &[CompletionRecord],
    companions: &[WadCompanionRecord],
) {
    println!("id={}", wad.id);
    println!("title={}", wad.title);
    if let Some(a) = &wad.author {
        println!("author={a}");
    }
    if let Some(y) = wad.year {
        println!("year={y}");
    }
    println!("status={}", wad.status);
    if let Some(r) = wad.rating {
        println!("rating={r}");
    }
    if !wad.tags.is_empty() {
        println!("tags={}", wad.tags.join(","));
    }
    println!("source_type={}", wad.source_type);
    if let Some(u) = &wad.source_url {
        println!("source_url={u}");
    }
    println!("playtime={}", stats.playtime);
    println!("sessions={}", stats.session_count);
    println!("times_beaten={}", stats.times_beaten);
    if let Some(lp) = &stats.last_played {
        println!("last_played={lp}");
    }
    if let Some(d) = &wad.description {
        println!("description={d}");
    }
    if let Some(n) = &wad.notes {
        println!("notes={n}");
    }
    for comp in completions {
        let notes = comp.notes.as_deref().unwrap_or("");
        let has_stats = if comp.stats_snapshot.is_some() {
            "1"
        } else {
            "0"
        };
        println!(
            "completion\t{}\t{}\t{}",
            comp.completed_at, has_stats, notes
        );
    }
    for c in companions {
        let enabled = if c.enabled { "1" } else { "0" };
        println!("companion\t{}\t{}", c.filename, enabled);
    }
}

fn render_wad_info_json(
    wad: &WadRecord,
    stats: &WadStats,
    completions: &[CompletionRecord],
    companions: &[WadCompanionRecord],
) {
    let mut val = serde_json::to_value(wad).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(ref mut map) = val {
        map.insert("playtime".to_string(), serde_json::json!(stats.playtime));
        map.insert(
            "times_beaten".to_string(),
            serde_json::json!(stats.times_beaten),
        );
        map.insert(
            "session_count".to_string(),
            serde_json::json!(stats.session_count),
        );
        map.insert(
            "last_played".to_string(),
            serde_json::json!(stats.last_played),
        );
        let completions_json: Vec<serde_json::Value> = completions
            .iter()
            .map(|c| {
                serde_json::json!({
                    "completed_at": c.completed_at,
                    "notes": c.notes,
                    "has_stats": c.stats_snapshot.is_some(),
                })
            })
            .collect();
        map.insert(
            "completions".to_string(),
            serde_json::json!(completions_json),
        );
        let companions_json: Vec<serde_json::Value> = companions
            .iter()
            .map(|c| {
                serde_json::json!({
                    "filename": c.filename,
                    "enabled": c.enabled,
                })
            })
            .collect();
        map.insert("companions".to_string(), serde_json::json!(companions_json));
    }
    println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
}

// ---------------------------------------------------------------------------
// Completion list rendering
// ---------------------------------------------------------------------------

pub fn render_completion_list(
    wad: &WadRecord,
    completions: &[CompletionRecord],
    format: OutputFormat,
) {
    match format {
        OutputFormat::Table => {
            println!("Completions for: {} (ID: {})", wad.title, wad.id);
            if completions.is_empty() {
                println!("  (none)");
                return;
            }
            let mut table = Table::new();
            table.load_preset(presets::NOTHING).set_header(vec![
                Cell::new("ID").fg(Color::DarkGrey),
                Cell::new("Date").fg(Color::DarkGrey),
                Cell::new("Stats").fg(Color::DarkGrey),
                Cell::new("Notes").fg(Color::DarkGrey),
            ]);
            for comp in completions {
                let has_stats = if comp.stats_snapshot.is_some() {
                    "yes"
                } else {
                    "--"
                };
                table.add_row(vec![
                    Cell::new(comp.id).set_alignment(CellAlignment::Right),
                    Cell::new(&comp.completed_at),
                    Cell::new(has_stats),
                    Cell::new(comp.notes.as_deref().unwrap_or("")),
                ]);
            }
            println!("{table}");
        }
        OutputFormat::Plain => {
            println!("ID\tDate\tHasStats\tNotes");
            for comp in completions {
                let has_stats = if comp.stats_snapshot.is_some() {
                    "1"
                } else {
                    "0"
                };
                println!(
                    "{}\t{}\t{has_stats}\t{}",
                    comp.id,
                    comp.completed_at,
                    comp.notes.as_deref().unwrap_or(""),
                );
            }
        }
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = completions
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "wad_id": c.wad_id,
                        "completed_at": c.completed_at,
                        "has_stats": c.stats_snapshot.is_some(),
                        "notes": c.notes,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&items).unwrap_or_default()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tag list rendering
// ---------------------------------------------------------------------------

pub fn render_tag_list(tags: &[(String, i64)], format: OutputFormat) {
    match format {
        OutputFormat::Table => {
            if tags.is_empty() {
                println!("No tags found.");
                return;
            }
            let mut table = Table::new();
            table.load_preset(presets::NOTHING).set_header(vec![
                Cell::new("Tag").fg(Color::DarkGrey),
                Cell::new("Count").fg(Color::DarkGrey),
            ]);
            for (tag, count) in tags {
                table.add_row(vec![
                    Cell::new(tag),
                    Cell::new(count).set_alignment(CellAlignment::Right),
                ]);
            }
            println!("{table}");
        }
        OutputFormat::Plain => {
            println!("Tag\tCount");
            for (tag, count) in tags {
                println!("{tag}\t{count}");
            }
        }
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = tags
                .iter()
                .map(|(tag, count)| serde_json::json!({"tag": tag, "count": count}))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&items).unwrap_or_default()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// IWAD list rendering
// ---------------------------------------------------------------------------

pub fn render_iwad_list(
    iwads: &[IwadRecord],
    preferred: &HashMap<String, String>,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Table => {
            if iwads.is_empty() {
                println!("No IWADs registered.");
                return;
            }
            let mut table = Table::new();
            table.load_preset(presets::NOTHING).set_header(vec![
                Cell::new("Family").fg(Color::DarkGrey),
                Cell::new("Variant").fg(Color::DarkGrey),
                Cell::new("Title").fg(Color::DarkGrey),
                Cell::new("Path").fg(Color::DarkGrey),
            ]);
            for iwad in iwads {
                let is_preferred = preferred
                    .get(&iwad.family)
                    .is_some_and(|v| v == &iwad.variant);
                let marker = if is_preferred { " *" } else { "" };
                table.add_row(vec![
                    Cell::new(&iwad.family),
                    Cell::new(format!("{}{marker}", iwad.variant)),
                    Cell::new(iwad.title.as_deref().unwrap_or("")),
                    Cell::new(&iwad.path),
                ]);
            }
            println!("{table}");
        }
        OutputFormat::Plain => {
            println!("Family\tVariant\tTitle\tPath");
            for iwad in iwads {
                let is_preferred = preferred
                    .get(&iwad.family)
                    .is_some_and(|v| v == &iwad.variant);
                let marker = if is_preferred { " *" } else { "" };
                println!(
                    "{}\t{}{marker}\t{}\t{}",
                    iwad.family,
                    iwad.variant,
                    iwad.title.as_deref().unwrap_or(""),
                    iwad.path,
                );
            }
        }
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = iwads
                .iter()
                .map(|iwad| {
                    serde_json::json!({
                        "family": iwad.family,
                        "variant": iwad.variant,
                        "title": iwad.title,
                        "path": iwad.path,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&items).unwrap_or_default()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// id24 list rendering
// ---------------------------------------------------------------------------

pub fn render_id24_list(id24s: &[Id24Record], format: OutputFormat) {
    match format {
        OutputFormat::Table => {
            if id24s.is_empty() {
                println!("No id24 WADs registered.");
                return;
            }
            let mut table = Table::new();
            table.load_preset(presets::NOTHING).set_header(vec![
                Cell::new("Name").fg(Color::DarkGrey),
                Cell::new("Version").fg(Color::DarkGrey),
                Cell::new("Title").fg(Color::DarkGrey),
                Cell::new("Path").fg(Color::DarkGrey),
            ]);
            for entry in id24s {
                table.add_row(vec![
                    Cell::new(&entry.name),
                    Cell::new(entry.version.as_deref().unwrap_or("")),
                    Cell::new(entry.title.as_deref().unwrap_or("")),
                    Cell::new(&entry.path),
                ]);
            }
            println!("{table}");
        }
        OutputFormat::Plain => {
            println!("Name\tVersion\tTitle\tPath");
            for entry in id24s {
                println!(
                    "{}\t{}\t{}\t{}",
                    entry.name,
                    entry.version.as_deref().unwrap_or(""),
                    entry.title.as_deref().unwrap_or(""),
                    entry.path,
                );
            }
        }
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = id24s
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "name": entry.name,
                        "version": entry.version,
                        "title": entry.title,
                        "path": entry.path,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&items).unwrap_or_default()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Session list rendering
// ---------------------------------------------------------------------------

pub fn render_session_list(
    sessions: &[SessionRecord],
    wad_title: &str,
    deltas: &[Option<Vec<String>>],
    format: OutputFormat,
) {
    match format {
        OutputFormat::Table | OutputFormat::Json => {
            if sessions.is_empty() {
                println!("No sessions for '{wad_title}'.");
                return;
            }
            let mut table = Table::new();
            table.load_preset(presets::NOTHING).set_header(vec![
                Cell::new("Date").fg(Color::DarkGrey),
                Cell::new("Started").fg(Color::DarkGrey),
                Cell::new("Duration").fg(Color::DarkGrey),
                Cell::new("Sourceport").fg(Color::DarkGrey),
                Cell::new("Maps").fg(Color::DarkGrey),
            ]);

            for (i, session) in sessions.iter().enumerate() {
                let date = format_timestamp(&session.started_at);
                let time = format_time(&session.started_at);
                let duration = session
                    .duration_seconds
                    .map(format_duration)
                    .unwrap_or_else(|| "--".to_string());
                let port = session.sourceport.as_deref().unwrap_or("--");
                let maps = deltas
                    .get(i)
                    .and_then(|d| d.as_ref())
                    .map(|maps| maps.join(", "))
                    .unwrap_or_else(|| "--".to_string());

                let crashed = session.exit_code.is_some_and(|c| c != 0);
                let crash_indicator = if crashed {
                    format!(" [Crash ({})]", session.exit_code.unwrap())
                } else {
                    String::new()
                };

                table.add_row(vec![
                    Cell::new(&date),
                    Cell::new(&time),
                    Cell::new(format!("{duration}{crash_indicator}")),
                    Cell::new(port),
                    Cell::new(&maps),
                ]);
            }
            println!("{table}");
        }
        OutputFormat::Plain => {
            println!("Date\tStarted\tDuration\tSourceport\tMaps");
            for (i, session) in sessions.iter().enumerate() {
                let date = format_timestamp(&session.started_at);
                let time = format_time(&session.started_at);
                let duration = session
                    .duration_seconds
                    .map(format_duration)
                    .unwrap_or_else(|| "--".to_string());
                let port = session.sourceport.as_deref().unwrap_or("--");
                let maps = deltas
                    .get(i)
                    .and_then(|d| d.as_ref())
                    .map(|maps| maps.join(","))
                    .unwrap_or_else(|| "--".to_string());
                println!("{date}\t{time}\t{duration}\t{port}\t{maps}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stats rendering
// ---------------------------------------------------------------------------

pub fn render_stats(snapshot: &StatsSnapshot, limit: usize, format: OutputFormat) {
    match format {
        OutputFormat::Plain => render_stats_plain(snapshot, limit),
        OutputFormat::Json => render_stats_json(snapshot, limit),
        OutputFormat::Table => render_stats_table(snapshot, limit),
    }
}

fn render_stats_json(snapshot: &StatsSnapshot, limit: usize) {
    let mut status_map = serde_json::Map::new();
    for (status, count) in &snapshot.wads_by_status {
        status_map.insert(status.clone(), serde_json::json!(count));
    }

    let activity: Vec<_> = snapshot
        .activity
        .iter()
        .take(limit)
        .map(|period| {
            serde_json::json!({
                "period": period.period,
                "wad_count": period.wad_count,
                "session_count": period.session_count,
                "total_playtime": period.total_playtime,
            })
        })
        .collect();

    let out = serde_json::json!({
        "total_wads": snapshot.total_wads,
        "total_playtime": snapshot.total_playtime,
        "total_sessions": snapshot.total_sessions,
        "wads_played": snapshot.wads_with_sessions,
        "completed_wads": snapshot.completed_wads,
        "played_wads": snapshot.played_wads,
        "completion_rate": snapshot.completion_rate,
        "total_completions": snapshot.total_completions,
        "wads_by_status": serde_json::Value::Object(status_map),
        "activity": activity,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
}

fn render_stats_table(snapshot: &StatsSnapshot, limit: usize) {
    println!("Library Statistics");
    println!("==================");
    println!();
    println!("  Total WADs:     {}", snapshot.total_wads);
    println!(
        "  Total playtime: {}",
        format_duration(snapshot.total_playtime)
    );
    println!("  Total sessions: {}", snapshot.total_sessions);
    println!("  WADs played:    {}", snapshot.wads_with_sessions);
    println!();
    println!(
        "  Completed:      {} / {} played ({:.0}%)",
        snapshot.completed_wads,
        snapshot.played_wads,
        snapshot.completion_rate * 100.0,
    );
    println!("  Total completions: {}", snapshot.total_completions);
    println!();

    // Status breakdown
    println!("  Status breakdown:");
    let status_order = ["unplayed", "in-progress", "completed", "abandoned"];
    for status in &status_order {
        let count = snapshot.wads_by_status.get(*status).copied().unwrap_or(0);
        if count > 0 {
            let display = Status::parse(status)
                .map(|s| s.display_name().to_string())
                .unwrap_or_else(|| (*status).to_string());
            println!("    {display:<18} {count}");
        }
    }

    // Activity
    if !snapshot.activity.is_empty() {
        println!();
        let mut table = Table::new();
        table.load_preset(presets::NOTHING).set_header(vec![
            Cell::new("Period").fg(Color::DarkGrey),
            Cell::new("WADs").fg(Color::DarkGrey),
            Cell::new("Sessions").fg(Color::DarkGrey),
            Cell::new("Playtime").fg(Color::DarkGrey),
        ]);

        for period in snapshot.activity.iter().take(limit) {
            table.add_row(vec![
                Cell::new(&period.period),
                Cell::new(period.wad_count).set_alignment(CellAlignment::Right),
                Cell::new(period.session_count).set_alignment(CellAlignment::Right),
                Cell::new(format_duration(period.total_playtime))
                    .set_alignment(CellAlignment::Right),
            ]);
        }
        println!("{table}");
    }
}

fn render_stats_plain(snapshot: &StatsSnapshot, limit: usize) {
    println!("total_wads={}", snapshot.total_wads);
    println!("total_playtime={}", snapshot.total_playtime);
    println!("total_sessions={}", snapshot.total_sessions);
    println!("wads_played={}", snapshot.wads_with_sessions);
    println!("completed_wads={}", snapshot.completed_wads);
    println!("played_wads={}", snapshot.played_wads);
    println!("completion_rate={:.2}", snapshot.completion_rate);
    println!("total_completions={}", snapshot.total_completions);

    let status_order = ["unplayed", "in-progress", "completed", "abandoned"];
    for status in &status_order {
        let count = snapshot.wads_by_status.get(*status).copied().unwrap_or(0);
        println!("status_{status}={count}");
    }

    for period in snapshot.activity.iter().take(limit) {
        println!(
            "activity\t{}\t{}\t{}\t{}",
            period.period, period.wad_count, period.session_count, period.total_playtime,
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format an RFC3339/ISO timestamp to a human-readable date (YYYY-MM-DD).
pub fn format_timestamp(ts: &str) -> String {
    // Try parsing as RFC3339 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt.format("%Y-%m-%d").to_string();
    }
    // Fallback: take first 10 chars if it looks like a date
    if ts.len() >= 10 {
        ts[..10].to_string()
    } else {
        ts.to_string()
    }
}

/// Format an RFC3339/ISO timestamp to time (HH:MM).
pub fn format_time(ts: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt.format("%H:%M").to_string();
    }
    if ts.len() >= 16 {
        ts[11..16].to_string()
    } else {
        String::new()
    }
}

/// Truncate a string to `max` characters, appending "…" if truncated.
pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp_rfc3339() {
        assert_eq!(format_timestamp("2024-06-15T18:30:00+00:00"), "2024-06-15");
    }

    #[test]
    fn test_format_timestamp_iso_prefix() {
        assert_eq!(format_timestamp("2024-06-15T18:30:00"), "2024-06-15");
    }

    #[test]
    fn test_format_timestamp_short() {
        assert_eq!(format_timestamp("2024"), "2024");
    }

    #[test]
    fn test_format_time_rfc3339() {
        assert_eq!(format_time("2024-06-15T18:30:00+00:00"), "18:30");
    }

    #[test]
    fn test_output_format_parse() {
        assert_eq!(
            "table".parse::<OutputFormat>().unwrap(),
            OutputFormat::Table
        );
        assert_eq!(
            "plain".parse::<OutputFormat>().unwrap(),
            OutputFormat::Plain
        );
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("invalid".parse::<OutputFormat>().is_err());
    }
}

// ---------------------------------------------------------------------------
// Cacoward rendering
// ---------------------------------------------------------------------------

/// One Cacoward entry plus the resolved play status of its linked WAD (if
/// any). Status is `None` when the entry isn't linked to a library WAD or
/// when the link points to a deleted row.
pub type CacowardView = (CacowardRecord, Option<Status>);

/// Per-(year, category) completion summary used by the aggregate view.
#[derive(Debug, Clone)]
pub struct CacowardSummaryRow {
    pub year: i64,
    pub category: String,
    pub total: usize,
    pub linked: usize,
    pub completed: usize,
    pub in_progress: usize,
}

pub fn render_cacowards_year(views: &[CacowardView], year: i64, format: OutputFormat) {
    match format {
        OutputFormat::Table => render_cacowards_year_table(views, year),
        OutputFormat::Plain => render_cacowards_year_plain(views, year),
        OutputFormat::Json => render_cacowards_year_json(views, year),
    }
}

pub fn render_cacowards_summary(rows: &[CacowardSummaryRow], format: OutputFormat) {
    match format {
        OutputFormat::Table => render_cacowards_summary_table(rows),
        OutputFormat::Plain => render_cacowards_summary_plain(rows),
        OutputFormat::Json => render_cacowards_summary_json(rows),
    }
}

fn category_display(category: &str) -> &'static str {
    match category {
        "winner" => "Winner",
        "runner-up" => "Runner-up",
        "honorable-mention" => "Honorable Mention",
        "mordeth" => "Mordeth",
        _ => "Other",
    }
}

fn status_label(status: Option<Status>) -> &'static str {
    match status {
        Some(Status::Completed) => "completed",
        Some(Status::InProgress) => "in-progress",
        Some(Status::Abandoned) => "abandoned",
        Some(Status::Unplayed) => "unplayed",
        None => "unlinked",
    }
}

fn status_color(status: Option<Status>) -> Color {
    match status {
        Some(Status::Completed) => Color::Green,
        Some(Status::InProgress) => Color::Yellow,
        Some(Status::Abandoned) => Color::Red,
        Some(Status::Unplayed) => Color::White,
        None => Color::DarkGrey,
    }
}

fn render_cacowards_year_table(views: &[CacowardView], year: i64) {
    if views.is_empty() {
        println!("No Cacoward entries recorded for {year}.");
        return;
    }

    let mut table = Table::new();
    table.load_preset(presets::NOTHING).set_header(vec![
        Cell::new("Category").fg(Color::DarkGrey),
        Cell::new("#").fg(Color::DarkGrey),
        Cell::new("Title").fg(Color::DarkGrey),
        Cell::new("Author").fg(Color::DarkGrey),
        Cell::new("Status").fg(Color::DarkGrey),
        Cell::new("WAD").fg(Color::DarkGrey),
    ]);

    let mut last_category: Option<&str> = None;
    for (record, status) in views {
        let cat = record.category.as_str();
        let cat_cell = if last_category == Some(cat) {
            String::new()
        } else {
            category_display(cat).to_string()
        };
        last_category = Some(cat);
        let rank = record.rank.map(|r| r.to_string()).unwrap_or_default();
        let author = record.wad_author.as_deref().unwrap_or("").to_string();
        let wad_cell = record
            .wad_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "—".to_string());

        table.add_row(vec![
            Cell::new(cat_cell),
            Cell::new(rank).set_alignment(CellAlignment::Right),
            Cell::new(&record.wad_title),
            Cell::new(author),
            Cell::new(status_label(*status)).fg(status_color(*status)),
            Cell::new(wad_cell),
        ]);
    }

    println!("Cacowards {year}");
    println!("{table}");
}

fn render_cacowards_year_plain(views: &[CacowardView], year: i64) {
    for (record, status) in views {
        // TSV: year, category, rank, title, author, status, wad_id, idgames_url
        println!(
            "{year}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            record.category,
            record.rank.map(|r| r.to_string()).unwrap_or_default(),
            record.wad_title,
            record.wad_author.as_deref().unwrap_or(""),
            status_label(*status),
            record.wad_id.map(|id| id.to_string()).unwrap_or_default(),
            record.idgames_url.as_deref().unwrap_or(""),
        );
    }
}

fn render_cacowards_year_json(views: &[CacowardView], year: i64) {
    let entries: Vec<serde_json::Value> = views
        .iter()
        .map(|(record, status)| {
            serde_json::json!({
                "id": record.id,
                "category": record.category,
                "rank": record.rank,
                "wad_title": record.wad_title,
                "wad_author": record.wad_author,
                "idgames_url": record.idgames_url,
                "doomwiki_url": record.doomwiki_url,
                "blurb": record.blurb,
                "wad_id": record.wad_id,
                "status": status.map(|s| s.as_str().to_string()),
                "manual_override": record.manual_override,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "year": year,
        "count": entries.len(),
        "entries": entries,
    });

    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
}

fn render_cacowards_summary_table(rows: &[CacowardSummaryRow]) {
    if rows.is_empty() {
        println!("No Cacoward entries in the database. Run `caco enrich --cacowards --year YYYY`.");
        return;
    }

    let mut table = Table::new();
    table.load_preset(presets::NOTHING).set_header(vec![
        Cell::new("Year").fg(Color::DarkGrey),
        Cell::new("Category").fg(Color::DarkGrey),
        Cell::new("Total").fg(Color::DarkGrey),
        Cell::new("Linked").fg(Color::DarkGrey),
        Cell::new("Done").fg(Color::DarkGrey),
        Cell::new("In Progress").fg(Color::DarkGrey),
    ]);

    let mut last_year: Option<i64> = None;
    for row in rows {
        let year_cell = if last_year == Some(row.year) {
            String::new()
        } else {
            row.year.to_string()
        };
        last_year = Some(row.year);
        let done_color = if row.completed == row.total && row.total > 0 {
            Color::Green
        } else if row.completed > 0 {
            Color::Yellow
        } else {
            Color::White
        };
        table.add_row(vec![
            Cell::new(year_cell),
            Cell::new(category_display(&row.category)),
            Cell::new(row.total).set_alignment(CellAlignment::Right),
            Cell::new(row.linked).set_alignment(CellAlignment::Right),
            Cell::new(row.completed)
                .set_alignment(CellAlignment::Right)
                .fg(done_color),
            Cell::new(row.in_progress).set_alignment(CellAlignment::Right),
        ]);
    }

    println!("{table}");
}

fn render_cacowards_summary_plain(rows: &[CacowardSummaryRow]) {
    for row in rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            row.year, row.category, row.total, row.linked, row.completed, row.in_progress,
        );
    }
}

fn render_cacowards_summary_json(rows: &[CacowardSummaryRow]) {
    // Group by year for a structured JSON document.
    let mut by_year: Vec<serde_json::Value> = Vec::new();
    let mut current_year: Option<i64> = None;
    let mut current_categories: Vec<serde_json::Value> = Vec::new();

    for row in rows {
        if current_year != Some(row.year) {
            if let Some(year) = current_year {
                by_year.push(serde_json::json!({
                    "year": year,
                    "categories": std::mem::take(&mut current_categories),
                }));
            }
            current_year = Some(row.year);
        }
        current_categories.push(serde_json::json!({
            "category": row.category,
            "total": row.total,
            "linked": row.linked,
            "completed": row.completed,
            "in_progress": row.in_progress,
        }));
    }
    if let Some(year) = current_year {
        by_year.push(serde_json::json!({
            "year": year,
            "categories": current_categories,
        }));
    }

    let payload = serde_json::json!({ "years": by_year });
    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
}

//! `caco gc` — garbage collection for finished/abandoned WAD data.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args;
use rusqlite::Connection;

use caco_core::config;
use caco_core::db::{self, WadRecord};
use caco_core::demos::DEMO_EXTENSION;
use caco_core::sourceports::ALL_SAVE_EXTENSIONS;

use crate::resolve;

// =============================================================================
// CLI args
// =============================================================================

#[derive(Args)]
pub struct GcArgs {
    /// Preview cleanup without deleting
    #[arg(long)]
    dry_run: bool,

    /// Skip all confirmation prompts
    #[arg(short = 'y', long)]
    yes: bool,

    /// Preserve save files (.dsg/.zds) in data dirs
    #[arg(long)]
    keep_saves: bool,

    /// Preserve demo files (.lmp) in data dirs
    #[arg(long)]
    keep_demos: bool,

    /// Skip data directory cleanup entirely
    #[arg(long)]
    keep_data: bool,

    /// Skip cache file cleanup entirely
    #[arg(long)]
    keep_cache: bool,

    /// Skip companion file cleanup
    #[arg(long)]
    keep_companions: bool,

    /// Only clean orphaned data dirs, backups, and companion files
    #[arg(long)]
    orphans_only: bool,

    /// Mark WAD(s) as GC-ignored
    #[arg(long)]
    ignore: Vec<String>,

    /// Remove GC-ignore from WAD(s)
    #[arg(long)]
    unignore: Vec<String>,
}

// =============================================================================
// Options bundle
// =============================================================================

struct GcOptions {
    keep_data: bool,
    keep_cache: bool,
    keep_saves: bool,
    keep_demos: bool,
    keep_companions: bool,
}

// =============================================================================
// Entry point
// =============================================================================

pub fn run(conn: &Connection, args: &GcArgs) -> Result<(), String> {
    // Handle --ignore / --unignore first
    if !args.ignore.is_empty() {
        handle_ignore(conn, &args.ignore, true)?;
        return Ok(());
    }
    if !args.unignore.is_empty() {
        handle_ignore(conn, &args.unignore, false)?;
        return Ok(());
    }

    let opts = GcOptions {
        keep_data: args.keep_data,
        keep_cache: args.keep_cache,
        keep_saves: args.keep_saves,
        keep_demos: args.keep_demos,
        keep_companions: args.keep_companions,
    };

    let mut total_freed: u64 = 0;

    // Phase 1: Clean finished/abandoned WADs (unless --orphans-only)
    if !args.orphans_only {
        total_freed += gc_finished_wads(conn, &opts, args.dry_run, args.yes)?;
    }

    // Phase 2: Orphaned data dirs
    let orphaned_dirs = find_orphaned_data_dirs(conn);
    if !orphaned_dirs.is_empty() {
        total_freed += gc_orphans(
            &orphaned_dirs,
            "orphaned data dirs",
            |p| {
                fs::remove_dir_all(p).ok();
            },
            None::<fn()>,
            args.dry_run,
            args.yes,
        )?;
    }

    // Phase 3: Orphaned companion files
    if !opts.keep_companions {
        let orphaned_companions = find_orphaned_companions(conn);
        if !orphaned_companions.is_empty() {
            let remove_records = || {
                let orphans = db::get_orphaned_companions(conn).unwrap_or_default();
                for c in orphans {
                    let _ = db::remove_companion(conn, c.id);
                }
            };
            total_freed += gc_orphans(
                &orphaned_companions,
                "orphaned companion files",
                |p| {
                    fs::remove_file(p).ok();
                },
                Some(remove_records),
                args.dry_run,
                args.yes,
            )?;
        }
    }

    // Phase 4: Orphaned backups
    let orphaned_backups = find_orphaned_backups(conn);
    if !orphaned_backups.is_empty() {
        total_freed += gc_orphans(
            &orphaned_backups,
            "orphaned backups",
            |p| {
                fs::remove_file(p).ok();
            },
            None::<fn()>,
            args.dry_run,
            args.yes,
        )?;
    }

    // Summary
    if total_freed > 0 || args.dry_run {
        println!();
        if args.dry_run {
            println!(
                "Total reclaimable: {}. No changes made (dry run).",
                format_size(total_freed)
            );
        } else {
            println!("Total freed: {}.", format_size(total_freed));
        }
    } else {
        println!("Nothing to clean up.");
    }

    Ok(())
}

// =============================================================================
// --ignore / --unignore
// =============================================================================

fn handle_ignore(conn: &Connection, query: &[String], ignore: bool) -> Result<(), String> {
    let wads = resolve::resolve_wads(conn, query, resolve::ResolveMode::Multiple, true, false)?;
    let flag = if ignore { 1i64 } else { 0i64 };
    let label = if ignore { "ignored" } else { "un-ignored" };

    for wad in &wads {
        let update = db::WadUpdate::new()
            .set_int("gc_ignore", Some(flag))
            .map_err(|e| format!("Failed to build update: {e}"))?;
        db::update_wad(conn, wad.id, &update).map_err(|e| format!("Failed to update WAD: {e}"))?;
        println!("GC {label}: {} (id:{})", wad.title, wad.id);
    }
    Ok(())
}

// =============================================================================
// Phase 1: Finished/abandoned WADs
// =============================================================================

fn gc_finished_wads(
    conn: &Connection,
    opts: &GcOptions,
    dry_run: bool,
    yes: bool,
) -> Result<u64, String> {
    let candidates = get_gc_candidates(conn)?;
    if candidates.is_empty() {
        return Ok(0);
    }

    let data_dir_map = build_data_dir_map();

    // Measure each candidate
    let mut entries: Vec<GcEntry> = Vec::new();
    for wad in &candidates {
        let entry = measure_wad(conn, wad, &data_dir_map, opts)?;
        if entry.total_size > 0 {
            entries.push(entry);
        }
    }

    if entries.is_empty() {
        return Ok(0);
    }

    gc_batch_clean(conn, &entries, opts, dry_run, yes)
}

fn get_gc_candidates(conn: &Connection) -> Result<Vec<WadRecord>, String> {
    // All abandoned and completed WADs.
    let query = "status:abandoned , status:completed";
    let wads = db::search_wads(conn, Some(query), None, true, false, 0)
        .map_err(|e| format!("Search error: {e}"))?;
    Ok(wads.into_iter().filter(|w| !w.gc_ignore).collect())
}

fn gc_batch_clean(
    conn: &Connection,
    entries: &[GcEntry],
    opts: &GcOptions,
    dry_run: bool,
    yes: bool,
) -> Result<u64, String> {
    println!("Finished/abandoned WADs:");
    println!(
        "  {:<6} {:<40} {:<12} {:<6} {:>10} {:>10} {:>10}",
        "ID", "Title", "Status", "Re-DL", "Data", "Cache", "Companions"
    );
    println!("  {}", "-".repeat(102));

    let mut total_size = 0u64;
    for entry in entries {
        let title = truncate(&entry.wad.title, 38);
        let redownloadable = if entry.wad.source_type == "idgames" || entry.wad.idgames_id.is_some() {
            "yes"
        } else {
            "no"
        };
        println!(
            "  {:<6} {:<40} {:<12} {:<6} {:>10} {:>10} {:>10}",
            entry.wad.id,
            title,
            entry.wad.status,
            redownloadable,
            format_size(entry.data_size),
            format_size(entry.cache_size),
            format_size(entry.companion_size),
        );
        total_size += entry.total_size;
    }

    println!(
        "\n  {} WADs, {} total",
        entries.len(),
        format_size(total_size)
    );

    if dry_run {
        println!("  (dry run — no changes)");
        return Ok(total_size);
    }

    if !yes && !resolve::confirm("  Clean all?") {
        println!("  Skipped.");
        return Ok(0);
    }

    let mut freed = 0u64;
    for entry in entries {
        freed += clean_wad_data(conn, entry, opts)?;
    }
    println!("  Cleaned {} WADs, {} freed.", entries.len(), format_size(freed));
    Ok(freed)
}

// =============================================================================
// WAD measurement & cleanup
// =============================================================================

struct GcEntry {
    wad: WadRecord,
    data_dir: Option<PathBuf>,
    data_size: u64,
    cache_path: Option<PathBuf>,
    cache_size: u64,
    companions: Vec<CompanionInfo>,
    companion_size: u64,
    total_size: u64,
}

struct CompanionInfo {
    companion_id: i64,
}

fn measure_wad(
    conn: &Connection,
    wad: &WadRecord,
    data_dir_map: &HashMap<i64, PathBuf>,
    opts: &GcOptions,
) -> Result<GcEntry, String> {
    // Data dir
    let (data_dir, data_size) = if !opts.keep_data {
        if let Some(dir) = data_dir_map.get(&wad.id) {
            let size = compute_data_size(dir, opts);
            (Some(dir.clone()), size)
        } else {
            (None, 0)
        }
    } else {
        (None, 0)
    };

    // Cache file
    let (cache_path, cache_size) = if !opts.keep_cache {
        if let Some(ref cp) = wad.cached_path {
            let p = PathBuf::from(cp);
            if p.is_file() {
                let size = p.metadata().map(|m| m.len()).unwrap_or(0);
                (Some(p), size)
            } else {
                (None, 0)
            }
        } else {
            (None, 0)
        }
    } else {
        (None, 0)
    };

    // Companions
    let (companions, companion_size) = if !opts.keep_companions {
        get_wad_companion_info(conn, wad.id)?
    } else {
        (Vec::new(), 0)
    };

    let total_size = data_size + cache_size + companion_size;

    Ok(GcEntry {
        wad: wad.clone(),
        data_dir,
        data_size,
        cache_path,
        cache_size,
        companions,
        companion_size,
        total_size,
    })
}

fn get_wad_companion_info(
    conn: &Connection,
    wad_id: i64,
) -> Result<(Vec<CompanionInfo>, u64), String> {
    let wad_companions = db::get_companions_for_wad(conn, wad_id)
        .map_err(|e| format!("Failed to get companions: {e}"))?;

    let mut infos = Vec::new();
    let mut total_size = 0u64;

    for wc in &wad_companions {
        let file_size = Path::new(&wc.path)
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0);

        let would_orphan = db::would_be_orphan(conn, wc.companion_id, wad_id)
            .map_err(|e| format!("Failed to check orphan: {e}"))?;

        // Only count size if this would become orphaned (i.e., actually freeable)
        if would_orphan {
            total_size += file_size;
        }

        infos.push(CompanionInfo {
            companion_id: wc.companion_id,
        });
    }

    Ok((infos, total_size))
}

fn clean_wad_data(conn: &Connection, entry: &GcEntry, opts: &GcOptions) -> Result<u64, String> {
    let mut freed = 0u64;

    // Clean data directory
    if let Some(ref data_dir) = entry.data_dir
        && !opts.keep_data
        && data_dir.is_dir()
    {
        if !opts.keep_saves && !opts.keep_demos {
            // Full removal
            freed += dir_size(data_dir);
            fs::remove_dir_all(data_dir).ok();
        } else {
            // Selective removal
            freed += selective_clean_data_dir(data_dir, opts);
        }
    }

    // Clean cache file
    if let Some(ref cache_path) = entry.cache_path
        && !opts.keep_cache
        && cache_path.is_file()
    {
        freed += cache_path.metadata().map(|m| m.len()).unwrap_or(0);
        fs::remove_file(cache_path).ok();
        db::clear_cached_path(conn, entry.wad.id)
            .map_err(|e| format!("Failed to clear cached_path: {e}"))?;
    }

    // Clean companion files
    if !opts.keep_companions {
        for comp in &entry.companions {
            db::unlink_companion_from_wad(conn, entry.wad.id, comp.companion_id)
                .map_err(|e| format!("Failed to unlink companion: {e}"))?;

            if db::is_orphan(conn, comp.companion_id)
                .map_err(|e| format!("Failed to check orphan: {e}"))?
                && let Some(path) = db::remove_companion_with_path(conn, comp.companion_id)
                    .map_err(|e| format!("Failed to remove companion: {e}"))?
            {
                let p = Path::new(&path);
                if p.is_file() {
                    freed += p.metadata().map(|m| m.len()).unwrap_or(0);
                    fs::remove_file(p).ok();
                }
            }
        }
    }

    // Clear stats snapshot (unless keep_data)
    if !opts.keep_data && entry.wad.stats_snapshot.is_some() {
        let update = db::WadUpdate::new()
            .set_text("stats_snapshot", None)
            .map_err(|e| format!("Failed to build update: {e}"))?;
        db::update_wad(conn, entry.wad.id, &update)
            .map_err(|e| format!("Failed to clear stats: {e}"))?;
    }

    Ok(freed)
}

fn selective_clean_data_dir(data_dir: &Path, opts: &GcOptions) -> u64 {
    let mut freed = 0u64;
    selective_clean_recursive(data_dir, opts, &mut freed);

    // Clean up empty subdirectories after selective removal
    remove_empty_dirs(data_dir);

    freed
}

fn selective_clean_recursive(dir: &Path, opts: &GcOptions, freed: &mut u64) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            selective_clean_recursive(&path, opts, freed);
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_lowercase()))
                .unwrap_or_default();

            // Skip protected file types
            if opts.keep_saves && ALL_SAVE_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            if opts.keep_demos && ext == DEMO_EXTENSION {
                continue;
            }

            // Delete everything else
            if let Ok(meta) = path.metadata() {
                *freed += meta.len();
            }
            fs::remove_file(&path).ok();
        }
    }
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                remove_empty_dirs(&path);
                // Try to remove — will fail if not empty
                fs::remove_dir(&path).ok();
            }
        }
    }
}

// =============================================================================
// Orphan detection
// =============================================================================

fn find_orphaned_data_dirs(conn: &Connection) -> Vec<(PathBuf, u64)> {
    let data_dir = config::get_data_dir();
    if !data_dir.is_dir() {
        return Vec::new();
    }

    let mut candidates: HashMap<i64, PathBuf> = HashMap::new();

    if let Ok(entries) = fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && let Some(id) = parse_wad_id_prefix(name)
            {
                candidates.insert(id, path);
            }
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Batch-check which IDs still exist (including soft-deleted)
    let candidate_ids: Vec<i64> = candidates.keys().copied().collect();
    let existing = get_existing_wad_ids(conn, &candidate_ids);

    candidates
        .into_iter()
        .filter(|(id, _)| !existing.contains(id))
        .map(|(_, path)| {
            let size = dir_size(&path);
            (path, size)
        })
        .collect()
}

fn find_orphaned_companions(conn: &Connection) -> Vec<(PathBuf, u64)> {
    let orphans = db::get_orphaned_companions(conn).unwrap_or_default();
    orphans
        .into_iter()
        .filter_map(|c| {
            let path = PathBuf::from(&c.path);
            if path.is_file() {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                Some((path, size))
            } else {
                // Clean up DB record for missing files
                let _ = db::remove_companion(conn, c.id);
                None
            }
        })
        .collect()
}

fn find_orphaned_backups(conn: &Connection) -> Vec<(PathBuf, u64)> {
    let backups = caco_core::saves::list_all_backups();
    if backups.is_empty() {
        return Vec::new();
    }

    let backup_ids: Vec<i64> = backups.iter().filter_map(|b| b.wad_id).collect();
    if backup_ids.is_empty() {
        return Vec::new();
    }

    let existing = get_existing_wad_ids(conn, &backup_ids);

    backups
        .into_iter()
        .filter(|b| b.wad_id.is_some_and(|id| !existing.contains(&id)))
        .map(|b| (b.path, b.size))
        .collect()
}

fn get_existing_wad_ids(conn: &Connection, ids: &[i64]) -> HashSet<i64> {
    let mut existing = HashSet::new();

    for chunk in ids.chunks(db::SQLITE_MAX_VARS) {
        let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT id FROM wads WHERE id IN ({placeholders})");
        if let Ok(mut stmt) = conn.prepare(&sql) {
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|id| id as &dyn rusqlite::types::ToSql)
                .collect();
            if let Ok(rows) = stmt.query_map(params.as_slice(), |row| row.get::<_, i64>(0)) {
                for row in rows.flatten() {
                    existing.insert(row);
                }
            }
        }
    }

    existing
}

// =============================================================================
// Generic orphan cleanup
// =============================================================================

fn gc_orphans<F, P>(
    orphans: &[(PathBuf, u64)],
    label: &str,
    delete_fn: F,
    post_delete_fn: Option<P>,
    dry_run: bool,
    yes: bool,
) -> Result<u64, String>
where
    F: Fn(&Path),
    P: FnOnce(),
{
    let total_size: u64 = orphans.iter().map(|(_, s)| *s).sum();

    println!("\nFound {} {} ({}):", orphans.len(), label, format_size(total_size));
    for (path, size) in orphans {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        println!("  {} ({})", name, format_size(*size));
    }

    if dry_run {
        println!("  (dry run — no changes)");
        return Ok(total_size);
    }

    if !yes && !resolve::confirm(&format!("  Clean {} {}?", orphans.len(), label)) {
        println!("  Skipped.");
        return Ok(0);
    }

    for (path, _) in orphans {
        delete_fn(path);
    }
    if let Some(post) = post_delete_fn {
        post();
    }

    println!("  Cleaned {} freed.", format_size(total_size));
    Ok(total_size)
}

// =============================================================================
// Helpers
// =============================================================================

fn build_data_dir_map() -> HashMap<i64, PathBuf> {
    let data_dir = config::get_data_dir();
    if !data_dir.is_dir() {
        return HashMap::new();
    }

    let mut map = HashMap::new();

    if let Ok(entries) = fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && let Some(id) = parse_wad_id_prefix(name)
            {
                map.insert(id, path);
            }
        }
    }

    map
}

/// Extract numeric WAD ID from a `{id}_...` name.
fn parse_wad_id_prefix(name: &str) -> Option<i64> {
    let underscore_pos = name.find('_')?;
    let prefix = &name[..underscore_pos];
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    prefix.parse().ok()
}

fn dir_size(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    dir_size_recursive(path, &mut total);
    total
}

fn dir_size_recursive(dir: &Path, total: &mut u64) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dir_size_recursive(&path, total);
            } else if let Ok(meta) = path.metadata() {
                *total += meta.len();
            }
        }
    }
}

fn compute_data_size(data_dir: &Path, opts: &GcOptions) -> u64 {
    if !data_dir.is_dir() {
        return 0;
    }
    if !opts.keep_saves && !opts.keep_demos {
        return dir_size(data_dir);
    }
    // Walk and exclude protected types
    let mut total = 0u64;
    compute_data_size_recursive(data_dir, opts, &mut total);
    total
}

fn compute_data_size_recursive(dir: &Path, opts: &GcOptions, total: &mut u64) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                compute_data_size_recursive(&path, opts, total);
            } else if let Ok(meta) = path.metadata() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| format!(".{}", e.to_lowercase()))
                    .unwrap_or_default();

                if opts.keep_saves && ALL_SAVE_EXTENSIONS.contains(&ext.as_str()) {
                    continue;
                }
                if opts.keep_demos && ext == DEMO_EXTENSION {
                    continue;
                }
                *total += meta.len();
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit_idx])
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::db::{self, init_db, open_memory};
    use caco_core::db::wads::{add_wad, NewWad};
    use caco_core::db::models::{SourceType, Status};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_wad_with_status(conn: &Connection, title: &str, status: Status) -> i64 {
        let wad_id = add_wad(conn, &NewWad::new(title, SourceType::Local).status(status)).unwrap();
        wad_id
    }

    // -- parse_wad_id_prefix tests --

    #[test]
    fn test_parse_wad_id_prefix_basic() {
        assert_eq!(parse_wad_id_prefix("42_my-wad"), Some(42));
    }

    #[test]
    fn test_parse_wad_id_prefix_large_id() {
        assert_eq!(parse_wad_id_prefix("12345_some-title"), Some(12345));
    }

    #[test]
    fn test_parse_wad_id_prefix_no_underscore() {
        assert_eq!(parse_wad_id_prefix("42"), None);
    }

    #[test]
    fn test_parse_wad_id_prefix_non_numeric() {
        assert_eq!(parse_wad_id_prefix("abc_wad"), None);
    }

    #[test]
    fn test_parse_wad_id_prefix_empty_prefix() {
        assert_eq!(parse_wad_id_prefix("_wad"), None);
    }

    #[test]
    fn test_parse_wad_id_prefix_mixed_prefix() {
        assert_eq!(parse_wad_id_prefix("12abc_wad"), None);
    }

    // -- format_size tests --

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "0 B");
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(512), "512 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    // -- truncate tests --

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate("hello world", 8);
        assert!(result.len() <= 10); // Unicode ellipsis
        assert!(result.ends_with('…'));
    }

    // -- get_gc_candidates tests --

    #[test]
    fn test_gc_candidates_finds_finished() {
        let conn = setup();
        add_wad_with_status(&conn, "Finished WAD", Status::Completed);
        add_wad_with_status(&conn, "Playing WAD", Status::InProgress);

        let candidates = get_gc_candidates(&conn).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Finished WAD");
    }

    #[test]
    fn test_gc_candidates_finds_abandoned() {
        let conn = setup();
        add_wad_with_status(&conn, "Abandoned WAD", Status::Abandoned);
        add_wad_with_status(&conn, "To Play WAD", Status::Unplayed);

        let candidates = get_gc_candidates(&conn).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Abandoned WAD");
    }

    #[test]
    fn test_gc_candidates_finds_both_statuses() {
        let conn = setup();
        add_wad_with_status(&conn, "Finished", Status::Completed);
        add_wad_with_status(&conn, "Abandoned", Status::Abandoned);
        add_wad_with_status(&conn, "Playing", Status::InProgress);

        let candidates = get_gc_candidates(&conn).unwrap();
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn test_gc_candidates_excludes_gc_ignore() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "Ignored WAD", Status::Completed);
        let update = db::WadUpdate::new().set_int("gc_ignore", Some(1)).unwrap();
        db::update_wad(&conn, wad_id, &update).unwrap();

        add_wad_with_status(&conn, "Not Ignored", Status::Completed);

        let candidates = get_gc_candidates(&conn).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Not Ignored");
    }

    #[test]
    fn test_gc_candidates_empty_db() {
        let conn = setup();
        let candidates = get_gc_candidates(&conn).unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_gc_candidates_no_finished_or_abandoned() {
        let conn = setup();
        add_wad_with_status(&conn, "Playing", Status::InProgress);
        add_wad_with_status(&conn, "To Play", Status::Unplayed);
        add_wad_with_status(&conn, "Backlog", Status::Unplayed);

        let candidates = get_gc_candidates(&conn).unwrap();
        assert!(candidates.is_empty());
    }

    // -- get_existing_wad_ids tests --

    #[test]
    fn test_get_existing_wad_ids_found() {
        let conn = setup();
        let id1 = add_wad_with_status(&conn, "WAD 1", Status::InProgress);
        let id2 = add_wad_with_status(&conn, "WAD 2", Status::InProgress);

        let existing = get_existing_wad_ids(&conn, &[id1, id2, 999]);
        assert!(existing.contains(&id1));
        assert!(existing.contains(&id2));
        assert!(!existing.contains(&999));
    }

    #[test]
    fn test_get_existing_wad_ids_empty() {
        let conn = setup();
        let existing = get_existing_wad_ids(&conn, &[]);
        assert!(existing.is_empty());
    }

    #[test]
    fn test_get_existing_wad_ids_all_missing() {
        let conn = setup();
        let existing = get_existing_wad_ids(&conn, &[100, 200, 300]);
        assert!(existing.is_empty());
    }

    // -- handle_ignore tests (via DB) --

    #[test]
    fn test_handle_ignore_sets_gc_ignore() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "My WAD", Status::Completed);

        handle_ignore(&conn, &["My WAD".to_string()], true).unwrap();

        let wad = db::get_wad(&conn, wad_id, false).unwrap().unwrap();
        assert!(wad.gc_ignore);
    }

    #[test]
    fn test_handle_unignore_clears_gc_ignore() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "My WAD", Status::Completed);

        // Set ignore first
        handle_ignore(&conn, &["My WAD".to_string()], true).unwrap();
        assert!(db::get_wad(&conn, wad_id, false).unwrap().unwrap().gc_ignore);

        // Unignore
        handle_ignore(&conn, &["My WAD".to_string()], false).unwrap();
        assert!(!db::get_wad(&conn, wad_id, false).unwrap().unwrap().gc_ignore);
    }

    // -- companion orphan measurement --

    #[test]
    fn test_get_wad_companion_info_empty() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "WAD", Status::Completed);

        let (infos, total_size) = get_wad_companion_info(&conn, wad_id).unwrap();
        assert!(infos.is_empty());
        assert_eq!(total_size, 0);
    }

    #[test]
    fn test_get_wad_companion_info_with_companion() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "WAD", Status::Completed);
        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", "/nonexistent/path.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        let (infos, _) = get_wad_companion_info(&conn, wad_id).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].companion_id, c_id);
    }

    // -- compute_data_size with keep flags --

    #[test]
    fn test_compute_data_size_full() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("stats.txt"), "some stats").unwrap();
        fs::write(data_dir.join("save1.dsg"), "save data").unwrap();
        fs::write(data_dir.join("demo.lmp"), "demo data").unwrap();

        let opts = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: false,
            keep_demos: false,
            keep_companions: false,
        };

        let size = compute_data_size(&data_dir, &opts);
        assert!(size > 0);
    }

    #[test]
    fn test_compute_data_size_keep_saves() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("stats.txt"), "some stats data").unwrap();
        fs::write(data_dir.join("save1.dsg"), "save data here").unwrap();

        let opts_full = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: false,
            keep_demos: false,
            keep_companions: false,
        };

        let opts_keep_saves = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: true,
            keep_demos: false,
            keep_companions: false,
        };

        let full_size = compute_data_size(&data_dir, &opts_full);
        let kept_size = compute_data_size(&data_dir, &opts_keep_saves);

        // Keeping saves should reduce the cleanable size
        assert!(kept_size < full_size);
    }

    #[test]
    fn test_compute_data_size_keep_demos() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        let demos_dir = data_dir.join("demos");
        fs::create_dir_all(&demos_dir).unwrap();
        fs::write(data_dir.join("stats.txt"), "some stats").unwrap();
        fs::write(demos_dir.join("demo1.lmp"), "demo data").unwrap();

        let opts_full = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: false,
            keep_demos: false,
            keep_companions: false,
        };

        let opts_keep_demos = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: false,
            keep_demos: true,
            keep_companions: false,
        };

        let full_size = compute_data_size(&data_dir, &opts_full);
        let kept_size = compute_data_size(&data_dir, &opts_keep_demos);

        // Keeping demos should reduce the cleanable size
        assert!(kept_size < full_size);
    }

    #[test]
    fn test_compute_data_size_nonexistent_dir() {
        let opts = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: false,
            keep_demos: false,
            keep_companions: false,
        };
        assert_eq!(compute_data_size(Path::new("/nonexistent/dir"), &opts), 0);
    }

    // -- selective clean tests --

    #[test]
    fn test_selective_clean_keeps_saves() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("stats.txt"), "stats").unwrap();
        fs::write(data_dir.join("save1.dsg"), "save data").unwrap();
        fs::write(data_dir.join("config.cfg"), "config").unwrap();

        let opts = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: true,
            keep_demos: false,
            keep_companions: false,
        };

        let freed = selective_clean_data_dir(&data_dir, &opts);
        assert!(freed > 0);

        // Save file should still exist
        assert!(data_dir.join("save1.dsg").exists());
        // Other files should be removed
        assert!(!data_dir.join("stats.txt").exists());
        assert!(!data_dir.join("config.cfg").exists());
    }

    #[test]
    fn test_selective_clean_keeps_demos() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        let demos_dir = data_dir.join("demos");
        fs::create_dir_all(&demos_dir).unwrap();
        fs::write(data_dir.join("stats.txt"), "stats").unwrap();
        fs::write(demos_dir.join("demo1.lmp"), "demo data").unwrap();

        let opts = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: false,
            keep_demos: true,
            keep_companions: false,
        };

        let freed = selective_clean_data_dir(&data_dir, &opts);
        assert!(freed > 0);

        // Demo file should still exist
        assert!(demos_dir.join("demo1.lmp").exists());
        // Other files should be removed
        assert!(!data_dir.join("stats.txt").exists());
    }

    #[test]
    fn test_selective_clean_keeps_both() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("save1.dsg"), "save").unwrap();
        fs::write(data_dir.join("demo.lmp"), "demo").unwrap();
        fs::write(data_dir.join("other.txt"), "other").unwrap();

        let opts = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: true,
            keep_demos: true,
            keep_companions: false,
        };

        selective_clean_data_dir(&data_dir, &opts);

        // Both protected types should survive
        assert!(data_dir.join("save1.dsg").exists());
        assert!(data_dir.join("demo.lmp").exists());
        // Other files removed
        assert!(!data_dir.join("other.txt").exists());
    }

    #[test]
    fn test_selective_clean_keeps_zds_saves() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("1_test-wad");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("save1.zds"), "zdoom save").unwrap();
        fs::write(data_dir.join("stats.txt"), "stats").unwrap();

        let opts = GcOptions {
            keep_data: false,
            keep_cache: false,
            keep_saves: true,
            keep_demos: false,
            keep_companions: false,
        };

        selective_clean_data_dir(&data_dir, &opts);

        // .zds save files should also be kept
        assert!(data_dir.join("save1.zds").exists());
        assert!(!data_dir.join("stats.txt").exists());
    }

    // -- dir_size tests --

    #[test]
    fn test_dir_size_basic() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "hello").unwrap();
        fs::write(dir.path().join("file2.txt"), "world").unwrap();

        let size = dir_size(dir.path());
        assert_eq!(size, 10);
    }

    #[test]
    fn test_dir_size_nested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.path().join("a.txt"), "aaa").unwrap();
        fs::write(sub.join("b.txt"), "bbb").unwrap();

        let size = dir_size(dir.path());
        assert_eq!(size, 6);
    }

    #[test]
    fn test_dir_size_nonexistent() {
        assert_eq!(dir_size(Path::new("/nonexistent/dir")), 0);
    }

    // -- orphan companion detection (DB level) --

    #[test]
    fn test_find_orphaned_companions_none() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "WAD", Status::InProgress);
        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", "/path/patch.deh", 100).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        let orphans = find_orphaned_companions(&conn);
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_find_orphaned_companions_with_orphan() {
        let conn = setup();
        // Create a companion with a real file but no WAD links
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("orphan.deh");
        fs::write(&file_path, "orphan content").unwrap();

        let _c_id = db::add_companion(
            &conn,
            "md5orphan",
            "orphan.deh",
            &file_path.to_string_lossy(),
            14,
        ).unwrap();

        let orphans = find_orphaned_companions(&conn);
        assert_eq!(orphans.len(), 1);
    }

    #[test]
    fn test_find_orphaned_companions_missing_file_cleanup() {
        let conn = setup();
        // Companion with path to nonexistent file
        let _c_id = db::add_companion(
            &conn,
            "md5missing",
            "missing.deh",
            "/nonexistent/missing.deh",
            100,
        ).unwrap();

        let orphans = find_orphaned_companions(&conn);
        // File doesn't exist, so not in orphans list
        assert!(orphans.is_empty());

        // But the DB record should have been cleaned up
        assert!(db::find_companion_by_md5(&conn, "md5missing").unwrap().is_none());
    }

    // -- GC companion size measurement --

    #[test]
    fn test_get_wad_companion_info_orphan_size() {
        let conn = setup();
        let w1 = add_wad_with_status(&conn, "WAD 1", Status::Completed);
        let w2 = add_wad_with_status(&conn, "WAD 2", Status::InProgress);
        let c_id = db::add_companion(&conn, "md5shared", "shared.deh", "/path/shared.deh", 100).unwrap();

        // Link to both WADs
        db::link_companion_to_wad(&conn, w1, c_id).unwrap();
        db::link_companion_to_wad(&conn, w2, c_id).unwrap();

        // For WAD 1: companion is shared with WAD 2, would NOT be orphaned
        let (infos, total_size) = get_wad_companion_info(&conn, w1).unwrap();
        assert_eq!(infos.len(), 1);
        // Size should be 0 because the companion wouldn't become orphaned
        assert_eq!(total_size, 0);
    }

    #[test]
    fn test_get_wad_companion_info_sole_owner() {
        let conn = setup();
        let wad_id = add_wad_with_status(&conn, "Solo WAD", Status::Completed);

        // Create a companion file at a real path
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("solo.deh");
        fs::write(&file_path, "solo content!!").unwrap();

        let c_id = db::add_companion(
            &conn,
            "md5solo",
            "solo.deh",
            &file_path.to_string_lossy(),
            14,
        ).unwrap();
        db::link_companion_to_wad(&conn, wad_id, c_id).unwrap();

        // This WAD is the sole owner — would be orphaned on cleanup
        let (infos, total_size) = get_wad_companion_info(&conn, wad_id).unwrap();
        assert_eq!(infos.len(), 1);
        // Should count the file size since it would become orphaned
        assert_eq!(total_size, 14);
    }

    // -- GC candidates with multiple statuses --

    #[test]
    fn test_gc_candidates_excludes_all_non_gc_statuses() {
        let conn = setup();
        add_wad_with_status(&conn, "Playing", Status::InProgress);
        add_wad_with_status(&conn, "To Play", Status::Unplayed);
        add_wad_with_status(&conn, "Backlog", Status::Unplayed);
        add_wad_with_status(&conn, "Awaiting", Status::Unplayed);

        let candidates = get_gc_candidates(&conn).unwrap();
        assert!(candidates.is_empty());
    }

    // -- remove_empty_dirs --

    #[test]
    fn test_remove_empty_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&sub).unwrap();

        // All empty — should be cleaned
        remove_empty_dirs(dir.path());
        assert!(!dir.path().join("a").exists());
    }

    #[test]
    fn test_remove_empty_dirs_preserves_nonempty() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("keep");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file.txt"), "content").unwrap();

        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        remove_empty_dirs(dir.path());

        // Non-empty dir preserved
        assert!(sub.exists());
        assert!(sub.join("file.txt").exists());
        // Empty dir removed
        assert!(!empty.exists());
    }
}

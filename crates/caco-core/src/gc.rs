//! Garbage collection for finished WAD data and orphaned files.
//!
//! Split into two halves on purpose. [`plan`] only measures — it walks the
//! data, cache, companion and backup directories and reports what *could* be
//! reclaimed, touching nothing. [`execute`] then deletes exactly the entries
//! it is handed back.
//!
//! That split is what lets both frontends confirm in their own idiom: the CLI
//! prints the plan and asks y/n per section, the GUI renders it as a checkbox
//! list and passes back the ticked subset. Neither has to re-derive what is
//! safe to delete, and nothing is deleted that the user did not see first.
//!
//! What counts as reclaimable:
//! - **Finished WADs** — completed or abandoned, not `gc_ignore`. Their data
//!   dir, cached download and companion files.
//! - **Orphans** — data dirs, backups and companion files whose WAD row is
//!   gone. These are unreachable regardless of any WAD's status.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::config;
use crate::db::{self, SourceType, Status, WadRecord};
use crate::demos::DEMO_EXTENSION;
use crate::sourceports::ALL_SAVE_EXTENSIONS;

/// What to leave alone during a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcOptions {
    /// Skip data directories entirely.
    pub keep_data: bool,
    /// Skip cached downloads.
    pub keep_cache: bool,
    /// Preserve save files inside data dirs.
    pub keep_saves: bool,
    /// Preserve demo files inside data dirs.
    pub keep_demos: bool,
    /// Skip companion files.
    pub keep_companions: bool,
    /// Only look for orphans; leave finished WADs alone.
    pub orphans_only: bool,
}

impl GcOptions {
    /// Whether a data dir can be removed wholesale rather than file by file.
    fn wholesale_data_removal(&self) -> bool {
        !self.keep_saves && !self.keep_demos
    }
}

/// The directories a run is allowed to touch.
///
/// Passed in rather than read from config inside [`plan`], because every path
/// here is a path something will be deleted from. A test (or any caller with
/// a different root) must be able to point this somewhere disposable, and it
/// must not be possible to forget to.
#[derive(Debug, Clone)]
pub struct GcPaths {
    /// Root holding the per-WAD `{id}_{slug}` directories.
    pub data_dir: PathBuf,
    /// Root holding save-backup archives.
    pub backup_dir: PathBuf,
}

impl GcPaths {
    /// The real, configured locations. Frontends use this; tests must not.
    pub fn from_config() -> Self {
        Self {
            data_dir: config::get_data_dir(),
            backup_dir: config::get_backup_dir(),
        }
    }
}

/// Reclaimable data belonging to one finished WAD.
#[derive(Debug, Clone)]
pub struct WadPlanEntry {
    pub wad_id: i64,
    pub title: String,
    pub status: Status,
    /// Whether the cached file could be fetched again if deleted.
    pub redownloadable: bool,
    pub data_dir: Option<PathBuf>,
    pub data_size: u64,
    pub cache_path: Option<PathBuf>,
    pub cache_size: u64,
    /// Companions linked to this WAD, in registry-id order.
    pub companion_ids: Vec<i64>,
    /// Only counts companions that *this* WAD is the last owner of — a file
    /// another WAD still links frees nothing.
    pub companion_size: u64,
    pub total_size: u64,
    /// Whether the WAD carries a stats snapshot that would be cleared.
    pub has_stats_snapshot: bool,
}

/// A file or directory nothing points at any more.
#[derive(Debug, Clone)]
pub struct OrphanEntry {
    pub path: PathBuf,
    pub size: u64,
    /// Registry row to drop along with the file. Set for companion orphans,
    /// `None` for data dirs and backups, which have no DB row of their own.
    pub companion_id: Option<i64>,
}

impl OrphanEntry {
    /// Filename for display, falling back to the full path.
    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Everything a run could reclaim, measured but not touched.
#[derive(Debug, Clone, Default)]
pub struct GcPlan {
    pub wads: Vec<WadPlanEntry>,
    pub orphan_data_dirs: Vec<OrphanEntry>,
    pub orphan_companions: Vec<OrphanEntry>,
    pub orphan_backups: Vec<OrphanEntry>,
}

impl GcPlan {
    pub fn is_empty(&self) -> bool {
        self.wads.is_empty()
            && self.orphan_data_dirs.is_empty()
            && self.orphan_companions.is_empty()
            && self.orphan_backups.is_empty()
    }

    pub fn total_size(&self) -> u64 {
        let orphans: u64 = self
            .orphan_data_dirs
            .iter()
            .chain(&self.orphan_companions)
            .chain(&self.orphan_backups)
            .map(|o| o.size)
            .sum();
        self.wads.iter().map(|w| w.total_size).sum::<u64>() + orphans
    }
}

/// The subset of a plan a caller confirmed. Borrowed so a frontend can filter
/// its own plan without cloning entries.
#[derive(Debug, Default)]
pub struct GcSelection<'a> {
    pub wads: &'a [WadPlanEntry],
    pub orphan_data_dirs: &'a [OrphanEntry],
    pub orphan_companions: &'a [OrphanEntry],
    pub orphan_backups: &'a [OrphanEntry],
}

impl<'a> GcSelection<'a> {
    /// Confirm an entire plan unchanged — the `-y` path.
    pub fn all(plan: &'a GcPlan) -> Self {
        Self {
            wads: &plan.wads,
            orphan_data_dirs: &plan.orphan_data_dirs,
            orphan_companions: &plan.orphan_companions,
            orphan_backups: &plan.orphan_backups,
        }
    }
}

// =============================================================================
// Planning
// =============================================================================

/// Measure everything a run with `opts` could reclaim. Read-only.
///
/// One exception to "touches nothing": companion registry rows whose file has
/// already vanished are dropped, because a row pointing at a missing file is
/// not a decision for the user to make — there is nothing left to reclaim.
pub fn plan(conn: &Connection, opts: GcOptions, paths: &GcPaths) -> crate::Result<GcPlan> {
    let mut plan = GcPlan::default();
    let data_dirs = data_dir_map(&paths.data_dir);

    if !opts.orphans_only {
        for wad in gc_candidates(conn)? {
            let entry = measure_wad(conn, &wad, &data_dirs, opts)?;
            if entry.total_size > 0 {
                plan.wads.push(entry);
            }
        }
    }

    plan.orphan_data_dirs = find_orphaned_data_dirs(conn, data_dirs);
    if !opts.keep_companions {
        plan.orphan_companions = find_orphaned_companions(conn);
    }
    plan.orphan_backups = find_orphaned_backups(conn, &paths.backup_dir);

    Ok(plan)
}

/// Completed or abandoned WADs that have not been excluded from GC.
fn gc_candidates(conn: &Connection) -> crate::Result<Vec<WadRecord>> {
    let wads = db::search_wads(
        conn,
        Some("status:abandoned , status:completed"),
        None,
        true,
        false,
        0,
    )?;
    Ok(wads.into_iter().filter(|w| !w.gc_ignore).collect())
}

fn measure_wad(
    conn: &Connection,
    wad: &WadRecord,
    data_dirs: &HashMap<i64, PathBuf>,
    opts: GcOptions,
) -> crate::Result<WadPlanEntry> {
    let (data_dir, data_size) = match (opts.keep_data, data_dirs.get(&wad.id)) {
        (false, Some(dir)) => (Some(dir.clone()), reclaimable_data_size(dir, opts)),
        _ => (None, 0),
    };

    let (cache_path, cache_size) = match (opts.keep_cache, wad.cached_path.as_deref()) {
        (false, Some(cached)) => {
            let path = PathBuf::from(cached);
            match path.metadata() {
                Ok(meta) if path.is_file() => (Some(path), meta.len()),
                _ => (None, 0),
            }
        }
        _ => (None, 0),
    };

    let (companion_ids, companion_size) = if opts.keep_companions {
        (Vec::new(), 0)
    } else {
        measure_companions(conn, wad.id)?
    };

    Ok(WadPlanEntry {
        wad_id: wad.id,
        title: wad.title.clone(),
        status: wad.status,
        redownloadable: wad.source_type == SourceType::Idgames || wad.idgames_id.is_some(),
        data_dir,
        data_size,
        cache_path,
        cache_size,
        companion_ids,
        companion_size,
        total_size: data_size + cache_size + companion_size,
        has_stats_snapshot: wad.stats_snapshot.is_some(),
    })
}

/// Companions linked to a WAD, and how much unlinking them would actually
/// free — a file another WAD still links stays on disk, so it counts as 0.
fn measure_companions(conn: &Connection, wad_id: i64) -> crate::Result<(Vec<i64>, u64)> {
    let mut ids = Vec::new();
    let mut freeable = 0u64;

    for companion in db::get_companions_for_wad(conn, wad_id)? {
        if db::would_be_orphan(conn, companion.companion_id, wad_id)? {
            freeable += file_size(Path::new(&companion.path));
        }
        ids.push(companion.companion_id);
    }

    Ok((ids, freeable))
}

// =============================================================================
// Execution
// =============================================================================

/// Delete exactly what `selection` names. Returns bytes actually freed.
///
/// Sizes are re-measured at deletion time rather than trusted from the plan,
/// so the reported total reflects the disk as it is now, not as it was when
/// the plan was built.
pub fn execute(
    conn: &Connection,
    selection: &GcSelection<'_>,
    opts: GcOptions,
) -> crate::Result<u64> {
    let mut freed = 0u64;

    for entry in selection.wads {
        freed += clean_wad(conn, entry, opts)?;
    }

    for orphan in selection.orphan_data_dirs {
        freed += dir_size(&orphan.path);
        let _ = fs::remove_dir_all(&orphan.path);
    }

    for orphan in selection.orphan_companions {
        freed += file_size(&orphan.path);
        let _ = fs::remove_file(&orphan.path);
        if let Some(id) = orphan.companion_id {
            db::remove_companion(conn, id)?;
        }
    }

    for orphan in selection.orphan_backups {
        freed += file_size(&orphan.path);
        let _ = fs::remove_file(&orphan.path);
    }

    Ok(freed)
}

fn clean_wad(conn: &Connection, entry: &WadPlanEntry, opts: GcOptions) -> crate::Result<u64> {
    let mut freed = 0u64;

    if let Some(ref data_dir) = entry.data_dir
        && data_dir.is_dir()
    {
        if opts.wholesale_data_removal() {
            freed += dir_size(data_dir);
            let _ = fs::remove_dir_all(data_dir);
        } else {
            freed += clean_data_dir_selectively(data_dir, opts);
        }
    }

    if let Some(ref cache_path) = entry.cache_path
        && cache_path.is_file()
    {
        freed += file_size(cache_path);
        let _ = fs::remove_file(cache_path);
        db::clear_cached_path(conn, entry.wad_id)?;
    }

    for companion_id in &entry.companion_ids {
        db::unlink_companion_from_wad(conn, entry.wad_id, *companion_id)?;

        // Only the last owner's unlink actually frees anything.
        if db::is_orphan(conn, *companion_id)?
            && let Some(path) = db::remove_companion_with_path(conn, *companion_id)?
        {
            let path = Path::new(&path);
            if path.is_file() {
                freed += file_size(path);
                let _ = fs::remove_file(path);
            }
        }
    }

    // The snapshot describes progress inside a data dir that no longer exists.
    if !opts.keep_data && entry.has_stats_snapshot {
        db::update_wad(
            conn,
            entry.wad_id,
            &db::WadUpdate::new().set_text("stats_snapshot", None),
        )?;
    }

    Ok(freed)
}

/// Exclude a WAD from GC, or put it back in scope.
pub fn set_gc_ignore(conn: &Connection, wad_id: i64, ignore: bool) -> crate::Result<()> {
    let update = db::WadUpdate::new().set_int("gc_ignore", Some(i64::from(ignore)));
    db::update_wad(conn, wad_id, &update)?;
    Ok(())
}

// =============================================================================
// Data directory walking
// =============================================================================

/// Whether a file is protected by `keep_saves` / `keep_demos`.
fn is_protected(path: &Path, opts: GcOptions) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();

    (opts.keep_saves && ALL_SAVE_EXTENSIONS.contains(&ext.as_str()))
        || (opts.keep_demos && ext == DEMO_EXTENSION)
}

/// Bytes in a data dir that a run with `opts` would actually delete.
fn reclaimable_data_size(data_dir: &Path, opts: GcOptions) -> u64 {
    if !data_dir.is_dir() {
        return 0;
    }
    if opts.wholesale_data_removal() {
        return dir_size(data_dir);
    }

    let mut total = 0u64;
    walk_files(data_dir, &mut |path, size| {
        if !is_protected(path, opts) {
            total += size;
        }
    });
    total
}

fn clean_data_dir_selectively(data_dir: &Path, opts: GcOptions) -> u64 {
    let mut doomed = Vec::new();
    let mut freed = 0u64;

    // Collect first, delete after: mutating a tree while walking it invites
    // skipped entries on some filesystems.
    walk_files(data_dir, &mut |path, size| {
        if !is_protected(path, opts) {
            doomed.push((path.to_path_buf(), size));
        }
    });

    for (path, size) in doomed {
        if fs::remove_file(&path).is_ok() {
            freed += size;
        }
    }

    remove_empty_dirs(data_dir);
    freed
}

/// Call `visit(path, size)` for every file under `dir`, recursively.
fn walk_files(dir: &Path, visit: &mut dyn FnMut(&Path, u64)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, visit);
        } else if let Ok(meta) = path.metadata() {
            visit(&path, meta.len());
        }
    }
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                remove_empty_dirs(&path);
                // Fails harmlessly when the directory still has contents.
                let _ = fs::remove_dir(&path);
            }
        }
    }
}

fn dir_size(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    walk_files(path, &mut |_, size| total += size);
    total
}

fn file_size(path: &Path) -> u64 {
    path.metadata().map(|m| m.len()).unwrap_or(0)
}

// =============================================================================
// Orphan detection
// =============================================================================

fn find_orphaned_data_dirs(
    conn: &Connection,
    candidates: HashMap<i64, PathBuf>,
) -> Vec<OrphanEntry> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let ids: Vec<i64> = candidates.keys().copied().collect();
    let existing = existing_wad_ids(conn, &ids);

    let mut orphans: Vec<OrphanEntry> = candidates
        .into_iter()
        .filter(|(id, _)| !existing.contains(id))
        .map(|(_, path)| OrphanEntry {
            size: dir_size(&path),
            path,
            companion_id: None,
        })
        .collect();
    orphans.sort_by(|a, b| a.path.cmp(&b.path));
    orphans
}

fn find_orphaned_companions(conn: &Connection) -> Vec<OrphanEntry> {
    db::get_orphaned_companions(conn)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let path = PathBuf::from(&c.path);
            if path.is_file() {
                return Some(OrphanEntry {
                    size: file_size(&path),
                    path,
                    companion_id: Some(c.id),
                });
            }
            // The file is already gone; the row is bookkeeping with nothing
            // behind it, so there is no decision to put to the user.
            let _ = db::remove_companion(conn, c.id);
            None
        })
        .collect()
}

fn find_orphaned_backups(conn: &Connection, backup_dir: &Path) -> Vec<OrphanEntry> {
    let backups = crate::saves::list_backups_in(backup_dir);
    let ids: Vec<i64> = backups.iter().filter_map(|b| b.wad_id).collect();
    if ids.is_empty() {
        return Vec::new();
    }

    let existing = existing_wad_ids(conn, &ids);
    backups
        .into_iter()
        .filter(|b| b.wad_id.is_some_and(|id| !existing.contains(&id)))
        .map(|b| OrphanEntry {
            path: b.path,
            size: b.size,
            companion_id: None,
        })
        .collect()
}

/// Which of `ids` still have a `wads` row — soft-deleted rows count, since a
/// restore would want its data back.
fn existing_wad_ids(conn: &Connection, ids: &[i64]) -> HashSet<i64> {
    let mut existing = HashSet::new();

    for chunk in ids.chunks(db::SQLITE_MAX_VARS) {
        let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT id FROM wads WHERE id IN ({placeholders})");
        let Ok(mut stmt) = conn.prepare(&sql) else {
            continue;
        };
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        if let Ok(rows) = stmt.query_map(params.as_slice(), |row| row.get::<_, i64>(0)) {
            existing.extend(rows.flatten());
        }
    }

    existing
}

/// Map of `{wad_id: data_dir}` for every `{id}_...` directory on disk.
fn data_dir_map(data_dir: &Path) -> HashMap<i64, PathBuf> {
    let Ok(entries) = fs::read_dir(data_dir) else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && let Some(id) = parse_wad_id_prefix(name)
        {
            map.insert(id, path);
        }
    }
    map
}

/// Extract the numeric WAD id from a `{id}_...` directory name.
pub fn parse_wad_id_prefix(name: &str) -> Option<i64> {
    let (prefix, _) = name.split_once('_')?;
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    prefix.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::SourceType;
    use crate::db::wads::{NewWad, add_wad};
    use crate::db::{init_db, open_memory};

    /// Every test in this module deletes files. `GcPaths` is therefore built
    /// only ever from `Sandbox` below — never `GcPaths::from_config()`, which
    /// points at the user's real library. Planning against a fresh in-memory
    /// DB makes *every* directory on disk look like an orphan, so a plan built
    /// over real paths and then executed would wipe the whole data dir.
    struct Sandbox {
        _root: tempfile::TempDir,
        paths: GcPaths,
    }

    impl Sandbox {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let paths = GcPaths {
                data_dir: root.path().join("data"),
                backup_dir: root.path().join("backups"),
            };
            fs::create_dir_all(&paths.data_dir).unwrap();
            fs::create_dir_all(&paths.backup_dir).unwrap();
            Self { _root: root, paths }
        }

        /// Create `data/{id}_{slug}/<rel>` with `bytes` bytes.
        fn data_file(&self, wad_id: i64, rel: &str, bytes: usize) -> PathBuf {
            let path = self.paths.data_dir.join(format!("{wad_id}_wad")).join(rel);
            write(&path, bytes);
            path
        }

        fn backup(&self, name: &str, bytes: usize) -> PathBuf {
            let path = self.paths.backup_dir.join(name);
            write(&path, bytes);
            path
        }
    }

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn write(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    /// Add a WAD in the given status, optionally with a cached file.
    fn add_status_wad(conn: &Connection, title: &str, status: &str, cached: Option<&Path>) -> i64 {
        let wad_id = add_wad(conn, &NewWad::new(title, SourceType::Local)).unwrap();
        let mut update = db::WadUpdate::new().set_text("status", Some(status.to_string()));
        if let Some(path) = cached {
            update = update.set_text("cached_path", Some(path.to_string_lossy().to_string()));
        }
        db::update_wad(conn, wad_id, &update).unwrap();
        wad_id
    }

    // -- parse_wad_id_prefix --

    #[test]
    fn test_parse_wad_id_prefix() {
        assert_eq!(parse_wad_id_prefix("42_scythe-2"), Some(42));
        assert_eq!(parse_wad_id_prefix("7_a_b_c"), Some(7));
        assert_eq!(parse_wad_id_prefix("noprefix"), None);
        assert_eq!(parse_wad_id_prefix("_leading"), None);
        assert_eq!(parse_wad_id_prefix("12a_mixed"), None);
    }

    // -- data dir sizing / protection --

    #[test]
    fn test_reclaimable_data_size_counts_everything_by_default() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("doomsav0.dsg"), 10);
        write(&dir.path().join("demos/run.lmp"), 20);
        write(&dir.path().join("stats.txt"), 5);

        assert_eq!(reclaimable_data_size(dir.path(), GcOptions::default()), 35);
    }

    #[test]
    fn test_reclaimable_data_size_excludes_protected() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("doomsav0.dsg"), 10);
        write(&dir.path().join("demos/run.lmp"), 20);
        write(&dir.path().join("stats.txt"), 5);

        let opts = GcOptions {
            keep_saves: true,
            ..Default::default()
        };
        assert_eq!(reclaimable_data_size(dir.path(), opts), 25);

        let opts = GcOptions {
            keep_saves: true,
            keep_demos: true,
            ..Default::default()
        };
        assert_eq!(reclaimable_data_size(dir.path(), opts), 5);
    }

    #[test]
    fn test_selective_clean_keeps_protected_files() {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("doomsav0.dsg");
        let demo = dir.path().join("demos/run.lmp");
        let junk = dir.path().join("stats.txt");
        write(&save, 10);
        write(&demo, 20);
        write(&junk, 5);

        let opts = GcOptions {
            keep_saves: true,
            keep_demos: true,
            ..Default::default()
        };
        let freed = clean_data_dir_selectively(dir.path(), opts);

        assert_eq!(freed, 5);
        assert!(save.exists());
        assert!(demo.exists());
        assert!(!junk.exists());
    }

    #[test]
    fn test_selective_clean_removes_emptied_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("demos/run.lmp"), 20);

        let opts = GcOptions {
            keep_saves: true,
            ..Default::default()
        };
        clean_data_dir_selectively(dir.path(), opts);

        assert!(
            !dir.path().join("demos").exists(),
            "a subdir emptied by the sweep should not be left behind"
        );
    }

    // -- planning --

    #[test]
    fn test_plan_skips_gc_ignored_wads() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let wad_id = add_status_wad(&conn, "Done", "completed", None);
        sandbox.data_file(wad_id, "stats.txt", 100);

        let measured = plan(&conn, GcOptions::default(), &sandbox.paths).unwrap();
        assert_eq!(measured.wads.len(), 1);
        assert_eq!(measured.wads[0].data_size, 100);

        set_gc_ignore(&conn, wad_id, true).unwrap();
        assert!(
            plan(&conn, GcOptions::default(), &sandbox.paths)
                .unwrap()
                .wads
                .is_empty()
        );

        set_gc_ignore(&conn, wad_id, false).unwrap();
        assert_eq!(
            plan(&conn, GcOptions::default(), &sandbox.paths)
                .unwrap()
                .wads
                .len(),
            1
        );
    }

    #[test]
    fn test_plan_ignores_unfinished_wads() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let wad_id = add_status_wad(&conn, "Playing", "in-progress", None);
        sandbox.data_file(wad_id, "stats.txt", 100);

        let measured = plan(&conn, GcOptions::default(), &sandbox.paths).unwrap();
        assert!(measured.wads.is_empty());
        // ...and its data dir is not an orphan either, since the row exists.
        assert!(measured.orphan_data_dirs.is_empty());
    }

    #[test]
    fn test_plan_finds_orphaned_data_dir() {
        let sandbox = Sandbox::new();
        let conn = setup();
        // No WAD row 999 — its directory is unreachable.
        sandbox.data_file(999, "stats.txt", 70);

        let measured = plan(&conn, GcOptions::default(), &sandbox.paths).unwrap();
        assert_eq!(measured.orphan_data_dirs.len(), 1);
        assert_eq!(measured.orphan_data_dirs[0].size, 70);
        assert_eq!(measured.total_size(), 70);
    }

    #[test]
    fn test_plan_finds_orphaned_backups_only() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let live = add_status_wad(&conn, "Live", "unplayed", None);
        sandbox.backup(&format!("{live}_live_20250101_000000.zip"), 10);
        sandbox.backup("999_gone_20250101_000000.zip", 20);
        // No parseable id — not attributable to any WAD, so never an orphan.
        sandbox.backup("library.pre-repair.zip", 40);

        let measured = plan(&conn, GcOptions::default(), &sandbox.paths).unwrap();
        assert_eq!(measured.orphan_backups.len(), 1);
        assert_eq!(measured.orphan_backups[0].size, 20);
    }

    #[test]
    fn test_plan_respects_keep_cache() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let cached = sandbox.paths.data_dir.join("cache/game.wad");
        write(&cached, 100);
        add_status_wad(&conn, "Done", "completed", Some(&cached));

        let opts = GcOptions {
            keep_cache: true,
            keep_data: true,
            ..Default::default()
        };
        // Nothing left to reclaim, so the entry drops out of the plan entirely.
        assert!(plan(&conn, opts, &sandbox.paths).unwrap().wads.is_empty());
    }

    #[test]
    fn test_plan_orphans_only_skips_wads() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let wad_id = add_status_wad(&conn, "Done", "completed", None);
        sandbox.data_file(wad_id, "stats.txt", 100);

        let opts = GcOptions {
            orphans_only: true,
            ..Default::default()
        };
        let measured = plan(&conn, opts, &sandbox.paths).unwrap();
        assert!(measured.wads.is_empty());
        assert!(measured.orphan_data_dirs.is_empty(), "the row still exists");
    }

    #[test]
    fn test_shared_companion_counts_as_no_savings() {
        let dir = tempfile::tempdir().unwrap();
        let conn = setup();
        let companion = dir.path().join("patch.deh");
        write(&companion, 500);

        let done = add_status_wad(&conn, "Done", "completed", None);
        let other = add_status_wad(&conn, "Other", "unplayed", None);

        let c_id = db::add_companion(
            &conn,
            "md5abc",
            "patch.deh",
            &companion.to_string_lossy(),
            500,
        )
        .unwrap();
        db::link_companion_to_wad(&conn, done, c_id).unwrap();
        db::link_companion_to_wad(&conn, other, c_id).unwrap();

        // `other` still wants the file, so cleaning `done` frees nothing.
        let (ids, size) = measure_companions(&conn, done).unwrap();
        assert_eq!(ids, vec![c_id]);
        assert_eq!(size, 0);

        // Once it is the sole owner, the file counts.
        db::unlink_companion_from_wad(&conn, other, c_id).unwrap();
        let (_, size) = measure_companions(&conn, done).unwrap();
        assert_eq!(size, 500);
    }

    // -- plan totals --

    #[test]
    fn test_plan_total_and_emptiness() {
        let mut measured = GcPlan::default();
        assert!(measured.is_empty());
        assert_eq!(measured.total_size(), 0);

        measured.orphan_backups.push(OrphanEntry {
            path: PathBuf::from("/tmp/1_x.zip"),
            size: 30,
            companion_id: None,
        });
        measured.wads.push(WadPlanEntry {
            wad_id: 1,
            title: "T".into(),
            status: Status::Completed,
            redownloadable: true,
            data_dir: None,
            data_size: 0,
            cache_path: None,
            cache_size: 12,
            companion_ids: Vec::new(),
            companion_size: 0,
            total_size: 12,
            has_stats_snapshot: false,
        });

        assert!(!measured.is_empty());
        assert_eq!(measured.total_size(), 42);
    }

    // -- execution --

    #[test]
    fn test_execute_only_touches_the_selection() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let keep = sandbox.backup("1_keep.zip", 10);
        let drop = sandbox.backup("2_drop.zip", 20);

        let orphans = [
            OrphanEntry {
                path: keep.clone(),
                size: 10,
                companion_id: None,
            },
            OrphanEntry {
                path: drop.clone(),
                size: 20,
                companion_id: None,
            },
        ];

        // Confirm only the second entry.
        let selection = GcSelection {
            orphan_backups: &orphans[1..],
            ..Default::default()
        };
        let freed = execute(&conn, &selection, GcOptions::default()).unwrap();

        assert_eq!(freed, 20);
        assert!(keep.exists(), "an unconfirmed entry must survive");
        assert!(!drop.exists());
    }

    #[test]
    fn test_execute_drops_companion_registry_row() {
        let dir = tempfile::tempdir().unwrap();
        let conn = setup();
        let managed = dir.path().join("patch.deh");
        write(&managed, 40);

        let c_id = db::add_companion(&conn, "md5abc", "patch.deh", &managed.to_string_lossy(), 40)
            .unwrap();

        let orphans = vec![OrphanEntry {
            path: managed.clone(),
            size: 40,
            companion_id: Some(c_id),
        }];
        let selection = GcSelection {
            orphan_companions: &orphans,
            ..Default::default()
        };

        assert_eq!(
            execute(&conn, &selection, GcOptions::default()).unwrap(),
            40
        );
        assert!(!managed.exists());
        assert!(
            db::find_companion_by_md5(&conn, "md5abc")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_execute_clears_cached_path_after_deleting_the_file() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let cached = sandbox.paths.data_dir.join("cache/game.wad");
        write(&cached, 100);
        let wad_id = add_status_wad(&conn, "Done", "completed", Some(&cached));

        let opts = GcOptions {
            keep_data: true,
            ..Default::default()
        };
        let measured = plan(&conn, opts, &sandbox.paths).unwrap();
        let freed = execute(&conn, &GcSelection::all(&measured), opts).unwrap();

        assert_eq!(freed, 100);
        assert!(!cached.exists());
        assert_eq!(
            db::get_wad(&conn, wad_id, false)
                .unwrap()
                .unwrap()
                .cached_path,
            None,
            "a dangling cached_path would make the WAD look downloaded"
        );
    }

    #[test]
    fn test_execute_removes_finished_wad_data_dir() {
        let sandbox = Sandbox::new();
        let conn = setup();
        let wad_id = add_status_wad(&conn, "Done", "completed", None);
        let stats = sandbox.data_file(wad_id, "stats.txt", 60);
        let data_dir = stats.parent().unwrap().to_path_buf();

        let measured = plan(&conn, GcOptions::default(), &sandbox.paths).unwrap();
        let freed = execute(&conn, &GcSelection::all(&measured), GcOptions::default()).unwrap();

        assert_eq!(freed, 60);
        assert!(!data_dir.exists());
    }

    #[test]
    fn test_execute_empty_selection_is_a_no_op() {
        let conn = setup();
        let selection = GcSelection::default();
        assert_eq!(execute(&conn, &selection, GcOptions::default()).unwrap(), 0);
    }
}

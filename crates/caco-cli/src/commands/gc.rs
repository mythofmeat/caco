//! `caco gc` — garbage collection for finished/abandoned WAD data.
//!
//! Output and prompting only. The measuring and deleting live in
//! `caco_core::gc`, which hands back a plan this command prints section by
//! section and confirms before executing.

use rusqlite::Connection;

use caco_core::gc::{self, GcOptions, GcPaths, GcPlan, GcSelection, OrphanEntry, WadPlanEntry};
use caco_core::utils::format_size;

use clap::Args;

use crate::output::truncate_str;
use crate::resolve;

#[derive(Args, Default)]
pub struct GcArgs {
    /// Preview cleanup without deleting
    #[arg(long)]
    dry_run: bool,

    /// Skip all confirmation prompts
    #[arg(short = 'y', long)]
    yes: bool,

    /// Preserve save files (.dsg/.zds/.hsg) in data dirs
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

impl GcArgs {
    fn options(&self) -> GcOptions {
        GcOptions {
            keep_data: self.keep_data,
            keep_cache: self.keep_cache,
            keep_saves: self.keep_saves,
            keep_demos: self.keep_demos,
            keep_companions: self.keep_companions,
            orphans_only: self.orphans_only,
        }
    }
}

pub fn run(conn: &Connection, args: &GcArgs) -> Result<(), String> {
    if !args.ignore.is_empty() {
        return set_ignore(conn, &args.ignore, true);
    }
    if !args.unignore.is_empty() {
        return set_ignore(conn, &args.unignore, false);
    }

    let opts = args.options();
    let plan = gc::plan(conn, opts, &GcPaths::from_config()).map_err(|e| e.to_string())?;

    if plan.is_empty() {
        println!("Nothing to clean up.");
        return Ok(());
    }

    if args.dry_run {
        report_plan(&plan);
        println!(
            "\nTotal reclaimable: {}. No changes made (dry run).",
            format_size(plan.total_size())
        );
        return Ok(());
    }

    // Each section is confirmed on its own so declining the WAD sweep doesn't
    // also skip the orphans, which are unreachable files either way.
    let selection = confirm_sections(&plan, args.yes);
    let freed = gc::execute(conn, &selection, opts).map_err(|e| e.to_string())?;

    if freed == 0 {
        println!("\nNothing cleaned.");
    } else {
        println!("\nTotal freed: {}.", format_size(freed));
    }
    Ok(())
}

// =============================================================================
// --ignore / --unignore
// =============================================================================

fn set_ignore(conn: &Connection, query: &[String], ignore: bool) -> Result<(), String> {
    let wads = resolve::resolve_wads(conn, query, resolve::ResolveMode::Multiple, true, false)?;
    let label = if ignore { "ignored" } else { "un-ignored" };

    for wad in &wads {
        gc::set_gc_ignore(conn, wad.id, ignore).map_err(|e| e.to_string())?;
        println!("GC {label}: {} (id:{})", wad.title, wad.id);
    }
    Ok(())
}

// =============================================================================
// Reporting
// =============================================================================

fn report_plan(plan: &GcPlan) {
    if !plan.wads.is_empty() {
        print_wad_table(&plan.wads);
    }
    print_orphans("orphaned data dirs", &plan.orphan_data_dirs);
    print_orphans("orphaned companion files", &plan.orphan_companions);
    print_orphans("orphaned backups", &plan.orphan_backups);
}

fn print_wad_table(entries: &[WadPlanEntry]) {
    println!("Finished/abandoned WADs:");
    println!(
        "  {:<6} {:<40} {:<12} {:<6} {:>10} {:>10} {:>10}",
        "ID", "Title", "Status", "Re-DL", "Data", "Cache", "Companions"
    );
    println!("  {}", "-".repeat(102));

    for entry in entries {
        println!(
            "  {:<6} {:<40} {:<12} {:<6} {:>10} {:>10} {:>10}",
            entry.wad_id,
            truncate_str(&entry.title, 38),
            entry.status,
            if entry.redownloadable { "yes" } else { "no" },
            format_size(entry.data_size),
            format_size(entry.cache_size),
            format_size(entry.companion_size),
        );
    }

    let total: u64 = entries.iter().map(|e| e.total_size).sum();
    println!("\n  {} WADs, {} total", entries.len(), format_size(total));
}

fn print_orphans(label: &str, orphans: &[OrphanEntry]) {
    if orphans.is_empty() {
        return;
    }
    let total: u64 = orphans.iter().map(|o| o.size).sum();
    println!(
        "\nFound {} {label} ({}):",
        orphans.len(),
        format_size(total)
    );
    for orphan in orphans {
        println!("  {} ({})", orphan.display_name(), format_size(orphan.size));
    }
}

// =============================================================================
// Confirmation
// =============================================================================

/// Print each section of the plan and keep the ones the user accepts.
fn confirm_sections<'a>(plan: &'a GcPlan, yes: bool) -> GcSelection<'a> {
    let mut selection = GcSelection::default();

    if !plan.wads.is_empty() {
        print_wad_table(&plan.wads);
        if accept(yes, "  Clean all?") {
            selection.wads = &plan.wads;
        }
    }

    let sections: [(&str, &[OrphanEntry], &mut &[OrphanEntry]); 3] = [
        (
            "orphaned data dirs",
            &plan.orphan_data_dirs,
            &mut selection.orphan_data_dirs,
        ),
        (
            "orphaned companion files",
            &plan.orphan_companions,
            &mut selection.orphan_companions,
        ),
        (
            "orphaned backups",
            &plan.orphan_backups,
            &mut selection.orphan_backups,
        ),
    ];

    for (label, orphans, slot) in sections {
        if orphans.is_empty() {
            continue;
        }
        print_orphans(label, orphans);
        if accept(yes, &format!("  Clean {} {label}?", orphans.len())) {
            *slot = orphans;
        }
    }

    selection
}

fn accept(yes: bool, prompt: &str) -> bool {
    if yes {
        return true;
    }
    if resolve::confirm(prompt) {
        return true;
    }
    println!("  Skipped.");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orphan(name: &str, size: u64) -> OrphanEntry {
        OrphanEntry {
            path: std::path::PathBuf::from(format!("/tmp/{name}")),
            size,
            companion_id: None,
        }
    }

    #[test]
    fn test_options_map_from_args() {
        let args = GcArgs {
            keep_saves: true,
            orphans_only: true,
            ..Default::default()
        };
        let opts = args.options();
        assert!(opts.keep_saves);
        assert!(opts.orphans_only);
        assert!(!opts.keep_data);
    }

    #[test]
    fn test_confirm_sections_with_yes_takes_everything() {
        let plan = GcPlan {
            orphan_data_dirs: vec![orphan("42_gone", 10)],
            orphan_backups: vec![orphan("7_old.zip", 20)],
            ..Default::default()
        };

        let selection = confirm_sections(&plan, true);
        assert_eq!(selection.orphan_data_dirs.len(), 1);
        assert_eq!(selection.orphan_backups.len(), 1);
        assert!(selection.orphan_companions.is_empty());
        assert!(selection.wads.is_empty());
    }

    #[test]
    fn test_empty_plan_selects_nothing() {
        let plan = GcPlan::default();
        let selection = confirm_sections(&plan, true);
        assert!(selection.wads.is_empty());
        assert!(selection.orphan_data_dirs.is_empty());
        assert!(selection.orphan_companions.is_empty());
        assert!(selection.orphan_backups.is_empty());
    }
}

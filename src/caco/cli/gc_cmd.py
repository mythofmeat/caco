"""Garbage collection command: clean finished/abandoned WAD data."""

import re
import shutil
import sys
from pathlib import Path

import click
from rich.table import Table

from caco import db
from caco.config import (
    find_wad_data_dir,
    get_backup_dir,
    get_cache_dir,
    get_data_dir,
)
from caco.saves import list_all_backups
from caco.utils import format_size as _format_size

from caco.cli import cli, console, err_console


# =============================================================================
# Helpers
# =============================================================================


def _dir_size(path: Path) -> int:
    """Total size of all files in a directory tree."""
    if not path.is_dir():
        return 0
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file())


def _find_orphaned_data_dirs() -> list[tuple[Path, int]]:
    """Find data dirs whose WAD ID no longer exists in the database."""
    data_dir = get_data_dir()
    if not data_dir.is_dir():
        return []

    orphans = []
    for entry in data_dir.iterdir():
        if not entry.is_dir():
            continue
        match = re.match(r"^(\d+)_", entry.name)
        if not match:
            continue
        wad_id = int(match.group(1))
        # Check if WAD exists (including deleted — soft-deleted WADs still own their data)
        wad = db.get_wad(wad_id, include_deleted=True)
        if not wad:
            size = _dir_size(entry)
            orphans.append((entry, size))

    return orphans


def _find_orphaned_backups() -> list[tuple[Path, int]]:
    """Find backup zips whose WAD ID no longer exists in the database."""
    backup_dir = get_backup_dir()
    if not backup_dir.is_dir():
        return []

    orphans = []
    for path in backup_dir.iterdir():
        if not path.is_file() or path.suffix != ".zip":
            continue
        match = re.match(r"^(\d+)_", path.name)
        if not match:
            continue
        wad_id = int(match.group(1))
        wad = db.get_wad(wad_id, include_deleted=True)
        if not wad:
            size = path.stat().st_size
            orphans.append((path, size))

    return orphans


def _get_gc_candidates() -> list[dict]:
    """Find finished/abandoned WADs eligible for GC (not gc_ignored)."""
    finished = db.search_wads(query="status:finished")
    abandoned = db.search_wads(query="status:abandoned")
    all_wads = finished + abandoned

    # Dedupe by ID and filter out gc_ignored
    seen = set()
    candidates = []
    for wad in all_wads:
        if wad["id"] not in seen and not wad.get("gc_ignore"):
            seen.add(wad["id"])
            candidates.append(wad)

    return candidates


def _wad_has_data(wad: dict) -> tuple[Path | None, int, Path | None, int]:
    """Check what cleanable data exists for a WAD.

    Returns (data_dir, data_size, cache_path, cache_size).
    """
    data_dir = find_wad_data_dir(wad["id"])
    data_size = _dir_size(data_dir) if data_dir else 0

    cache_path = None
    cache_size = 0
    if wad.get("cached_path"):
        p = Path(wad["cached_path"])
        if p.is_file():
            cache_path = p
            cache_size = p.stat().st_size

    return data_dir, data_size, cache_path, cache_size


def _clean_wad_data(
    wad: dict,
    data_dir: Path | None,
    cache_path: Path | None,
    *,
    keep_data: bool = False,
    keep_cache: bool = False,
    keep_saves: bool = False,
    keep_demos: bool = False,
) -> int:
    """Delete data/cache for a WAD. Returns bytes freed."""
    freed = 0

    if data_dir and data_dir.is_dir() and not keep_data:
        if keep_saves or keep_demos:
            # Selective cleanup within data dir
            from caco.sourceports import ALL_SAVE_EXTENSIONS

            for f in list(data_dir.rglob("*")):
                if not f.is_file():
                    continue
                if keep_saves and f.suffix.lower() in ALL_SAVE_EXTENSIONS:
                    continue
                if keep_demos and f.suffix.lower() == ".lmp":
                    continue
                freed += f.stat().st_size
                f.unlink()

            # Remove empty subdirectories
            for d in sorted(data_dir.rglob("*"), reverse=True):
                if d.is_dir():
                    try:
                        d.rmdir()
                    except OSError:
                        pass  # Not empty

            # Remove data dir itself if empty
            try:
                data_dir.rmdir()
            except OSError:
                pass
        else:
            freed += _dir_size(data_dir)
            shutil.rmtree(data_dir)

    if cache_path and cache_path.is_file() and not keep_cache:
        freed += cache_path.stat().st_size
        cache_path.unlink()
        db.clear_cached_path(wad["id"])

    # Clear live stats snapshot (data is archived in completions)
    if not keep_data:
        db.update_wad(wad["id"], record_completion=False, stats_snapshot=None)

    return freed


# =============================================================================
# GC Command
# =============================================================================


@cli.command(name="gc")
@click.option("--dry-run", is_flag=True, help="Preview what would be cleaned")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompts")
@click.option("--keep-data", is_flag=True, help="Don't delete data directories")
@click.option("--keep-cache", is_flag=True, help="Don't delete cached WAD files")
@click.option("--keep-saves", is_flag=True, help="Preserve save files in data dirs")
@click.option("--keep-demos", is_flag=True, help="Preserve demo files in data dirs")
@click.option("--orphans-only", is_flag=True, help="Only clean orphaned dirs/backups")
@click.option("--ignore", "ignore_query", help="Mark WAD(s) as permanently ignored by GC")
@click.option("--unignore", "unignore_query", help="Remove GC ignore from WAD(s)")
def gc_cmd(
    dry_run: bool,
    yes: bool,
    keep_data: bool,
    keep_cache: bool,
    keep_saves: bool,
    keep_demos: bool,
    orphans_only: bool,
    ignore_query: str | None,
    unignore_query: str | None,
):
    """Garbage collect finished/abandoned WAD data.

    Cleans up data directories, cached WAD files, orphaned data dirs,
    and orphaned backups. WADs from idgames are cleaned automatically
    (re-downloadable). Non-idgames WADs require individual confirmation.

    \b
    Examples:
        caco gc                    # Scan and clean with prompts
        caco gc --dry-run          # Preview what would be cleaned
        caco gc --keep-saves       # Clean but preserve save files
        caco gc --orphans-only     # Only clean orphaned dirs/backups
        caco gc --ignore id:5      # Permanently exclude WAD 5 from GC
        caco gc --unignore id:5    # Re-include WAD 5 in GC
    """
    # Handle --ignore / --unignore modes
    if ignore_query:
        _handle_ignore(ignore_query, ignore=True)
        return

    if unignore_query:
        _handle_ignore(unignore_query, ignore=False)
        return

    console.print("[bold]Scanning for garbage...[/bold]\n")

    total_freed = 0

    # Phase 1: Finished/abandoned WADs
    if not orphans_only:
        total_freed += _gc_finished_wads(
            dry_run=dry_run,
            yes=yes,
            keep_data=keep_data,
            keep_cache=keep_cache,
            keep_saves=keep_saves,
            keep_demos=keep_demos,
        )

    # Phase 2: Orphaned data dirs
    orphan_dirs = _find_orphaned_data_dirs()
    if orphan_dirs:
        total_freed += _gc_orphaned_dirs(orphan_dirs, dry_run=dry_run, yes=yes)

    # Phase 3: Orphaned backups
    orphan_backups = _find_orphaned_backups()
    if orphan_backups:
        total_freed += _gc_orphaned_backups(orphan_backups, dry_run=dry_run, yes=yes)

    # Summary
    if total_freed > 0:
        if dry_run:
            console.print(f"\n[bold]Total reclaimable: {_format_size(total_freed)}[/bold]")
            console.print("[dim]No changes made (dry run)[/dim]")
        else:
            console.print(f"\n[green bold]Total freed: {_format_size(total_freed)}[/green bold]")
    elif not orphan_dirs and not orphan_backups:
        console.print("[dim]Nothing to clean[/dim]")


def _handle_ignore(query: str, *, ignore: bool) -> None:
    """Set or clear gc_ignore flag on matching WADs."""
    from caco.cli import resolve_wad_query

    wads = resolve_wad_query(query, mode="multiple", yes=False)
    if not wads:
        return

    action = "ignored" if ignore else "un-ignored"
    for wad in wads:
        db.update_wad(wad["id"], record_completion=False, gc_ignore=1 if ignore else 0)

    count = len(wads)
    if ignore:
        console.print(f"[green]Marked {count} WAD(s) as GC-ignored[/green]")
    else:
        console.print(f"[green]Removed GC-ignore from {count} WAD(s)[/green]")


def _gc_finished_wads(
    *,
    dry_run: bool,
    yes: bool,
    keep_data: bool,
    keep_cache: bool,
    keep_saves: bool,
    keep_demos: bool,
) -> int:
    """Clean data for finished/abandoned WADs. Returns bytes freed."""
    candidates = _get_gc_candidates()
    if not candidates:
        return 0

    # Categorize and measure
    auto_clean = []  # idgames WADs (re-downloadable)
    interactive = []  # non-idgames WADs (need individual confirmation)

    for wad in candidates:
        data_dir, data_size, cache_path, cache_size = _wad_has_data(wad)
        total_size = 0
        if not keep_data:
            total_size += data_size
        if not keep_cache:
            total_size += cache_size

        if total_size == 0:
            continue  # Nothing to clean

        entry = {
            "wad": wad,
            "data_dir": data_dir,
            "data_size": data_size,
            "cache_path": cache_path,
            "cache_size": cache_size,
            "total_size": total_size,
        }

        if wad.get("source_type") == "idgames" or wad.get("idgames_id"):
            auto_clean.append(entry)
        else:
            interactive.append(entry)

    if not auto_clean and not interactive:
        return 0

    total_freed = 0

    # Auto-clean section (idgames WADs)
    if auto_clean:
        total_freed += _gc_auto_clean(
            auto_clean,
            dry_run=dry_run,
            yes=yes,
            keep_data=keep_data,
            keep_cache=keep_cache,
            keep_saves=keep_saves,
            keep_demos=keep_demos,
        )

    # Interactive section (non-idgames WADs)
    if interactive:
        total_freed += _gc_interactive(
            interactive,
            dry_run=dry_run,
            keep_data=keep_data,
            keep_cache=keep_cache,
            keep_saves=keep_saves,
            keep_demos=keep_demos,
        )

    return total_freed


def _gc_auto_clean(
    entries: list[dict],
    *,
    dry_run: bool,
    yes: bool,
    keep_data: bool,
    keep_cache: bool,
    keep_saves: bool,
    keep_demos: bool,
) -> int:
    """Clean idgames WADs with a single batch confirmation."""
    total_size = sum(e["total_size"] for e in entries)

    table = Table(title=f"Re-downloadable WADs ({len(entries)})")
    table.add_column("ID", style="dim")
    table.add_column("Title", style="cyan")
    table.add_column("Status", style="dim")
    table.add_column("Data", justify="right")
    table.add_column("Cache", justify="right")

    for entry in entries:
        wad = entry["wad"]
        data_str = _format_size(entry["data_size"]) if entry["data_size"] and not keep_data else "-"
        cache_str = _format_size(entry["cache_size"]) if entry["cache_size"] and not keep_cache else "-"
        table.add_row(
            str(wad["id"]),
            wad["title"],
            wad["status"],
            data_str,
            cache_str,
        )

    console.print(table)
    console.print(f"[bold]Subtotal:[/bold] {_format_size(total_size)}")

    if dry_run:
        return total_size

    if not yes:
        if not click.confirm("\nClean all re-downloadable WADs?"):
            console.print("[dim]Skipped[/dim]\n")
            return 0

    freed = 0
    for entry in entries:
        freed += _clean_wad_data(
            entry["wad"],
            entry["data_dir"],
            entry["cache_path"],
            keep_data=keep_data,
            keep_cache=keep_cache,
            keep_saves=keep_saves,
            keep_demos=keep_demos,
        )

    console.print(f"[green]Cleaned {len(entries)} WAD(s), freed {_format_size(freed)}[/green]\n")
    return freed


def _gc_interactive(
    entries: list[dict],
    *,
    dry_run: bool,
    keep_data: bool,
    keep_cache: bool,
    keep_saves: bool,
    keep_demos: bool,
) -> int:
    """Prompt individually for non-idgames WADs. Returns bytes freed."""
    console.print(f"\n[bold]Non-re-downloadable WADs ({len(entries)}):[/bold]")
    console.print("[dim]y = clean, n = skip, i = ignore permanently[/dim]\n")

    freed = 0
    for entry in entries:
        wad = entry["wad"]
        data_str = _format_size(entry["data_size"]) if entry["data_size"] and not keep_data else ""
        cache_str = _format_size(entry["cache_size"]) if entry["cache_size"] and not keep_cache else ""
        size_parts = [s for s in (data_str, cache_str) if s]
        size_display = " + ".join(size_parts) if size_parts else "0 B"

        console.print(
            f"  [dim][{wad['id']}][/dim] [cyan]{wad['title']}[/cyan] "
            f"({wad['status']}) — {size_display}"
        )

        if dry_run:
            freed += entry["total_size"]
            continue

        choice = click.prompt("  Clean?", type=click.Choice(["y", "n", "i"]), default="n")

        if choice == "y":
            freed += _clean_wad_data(
                wad,
                entry["data_dir"],
                entry["cache_path"],
                keep_data=keep_data,
                keep_cache=keep_cache,
                keep_saves=keep_saves,
                keep_demos=keep_demos,
            )
        elif choice == "i":
            db.update_wad(wad["id"], record_completion=False, gc_ignore=1)
            console.print("  [dim]Permanently ignored[/dim]")

    if freed > 0:
        console.print(f"\n[green]Freed {_format_size(freed)} from non-re-downloadable WADs[/green]")

    return freed


def _gc_orphaned_dirs(
    orphans: list[tuple[Path, int]],
    *,
    dry_run: bool,
    yes: bool,
) -> int:
    """Clean orphaned data directories."""
    total_size = sum(size for _, size in orphans)

    console.print(f"\n[bold]Orphaned data dirs ({len(orphans)}):[/bold]\n")
    for path, size in orphans:
        console.print(f"  {path.name} ({_format_size(size)})")

    console.print(f"\n[bold]Subtotal:[/bold] {_format_size(total_size)}")

    if dry_run:
        return total_size

    if not yes:
        if not click.confirm("\nDelete orphaned data dirs?"):
            console.print("[dim]Skipped[/dim]")
            return 0

    freed = 0
    for path, size in orphans:
        try:
            shutil.rmtree(path)
            freed += size
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"[green]Deleted {len(orphans)} orphaned dir(s), freed {_format_size(freed)}[/green]")
    return freed


def _gc_orphaned_backups(
    orphans: list[tuple[Path, int]],
    *,
    dry_run: bool,
    yes: bool,
) -> int:
    """Clean orphaned backup files."""
    total_size = sum(size for _, size in orphans)

    console.print(f"\n[bold]Orphaned backups ({len(orphans)}):[/bold]\n")
    for path, size in orphans:
        console.print(f"  {path.name} ({_format_size(size)})")

    console.print(f"\n[bold]Subtotal:[/bold] {_format_size(total_size)}")

    if dry_run:
        return total_size

    if not yes:
        if not click.confirm("\nDelete orphaned backups?"):
            console.print("[dim]Skipped[/dim]")
            return 0

    freed = 0
    for path, size in orphans:
        try:
            path.unlink()
            freed += size
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"[green]Deleted {len(orphans)} orphaned backup(s), freed {_format_size(freed)}[/green]")
    return freed

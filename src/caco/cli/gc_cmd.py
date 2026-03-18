"""Garbage collection command: clean finished/abandoned WAD data."""

import re
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

import click
from rich.table import Table

from caco import db
from caco.config import get_backup_dir, get_data_dir
from caco.db._connection import get_connection, _SQLITE_MAX_VARS
from caco.demos import DEMO_EXTENSION
from caco.saves import list_all_backups
from caco.sourceports import ALL_SAVE_EXTENSIONS
from caco.utils import format_size as _format_size

from caco.cli import cli, console, err_console


# =============================================================================
# Options dataclass
# =============================================================================


@dataclass(frozen=True, slots=True)
class GcOptions:
    """Cleanup flags threaded through the GC pipeline."""

    keep_data: bool = False
    keep_cache: bool = False
    keep_saves: bool = False
    keep_demos: bool = False
    keep_companions: bool = False


# =============================================================================
# Helpers
# =============================================================================


def _dir_size(path: Path) -> int:
    """Total size of all files in a directory tree."""
    if not path.is_dir():
        return 0
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file())


def _get_existing_wad_ids(candidate_ids: set[int]) -> set[int]:
    """Batch-check which WAD IDs exist in the database (including soft-deleted)."""
    if not candidate_ids:
        return set()

    existing: set[int] = set()
    ids_list = list(candidate_ids)
    with get_connection() as conn:
        for i in range(0, len(ids_list), _SQLITE_MAX_VARS):
            chunk = ids_list[i : i + _SQLITE_MAX_VARS]
            placeholders = ",".join("?" * len(chunk))
            rows = conn.execute(
                f"SELECT id FROM wads WHERE id IN ({placeholders})", chunk
            ).fetchall()
            existing.update(row[0] for row in rows)
    return existing


def _parse_wad_id_prefix(name: str) -> int | None:
    """Extract WAD ID from a '{id}_...' filename/dirname. Returns None if no match."""
    match = re.match(r"^(\d+)_", name)
    return int(match.group(1)) if match else None


def _find_orphaned_data_dirs() -> list[tuple[Path, int]]:
    """Find data dirs whose WAD ID no longer exists in the database."""
    data_dir = get_data_dir()
    if not data_dir.is_dir():
        return []

    # Collect all candidate IDs in one pass
    candidates: dict[int, Path] = {}
    for entry in data_dir.iterdir():
        if not entry.is_dir():
            continue
        wad_id = _parse_wad_id_prefix(entry.name)
        if wad_id is not None:
            candidates[wad_id] = entry

    if not candidates:
        return []

    existing_ids = _get_existing_wad_ids(set(candidates.keys()))

    orphans = []
    for wad_id, path in candidates.items():
        if wad_id not in existing_ids:
            orphans.append((path, _dir_size(path)))

    return orphans


def _find_orphaned_companions() -> list[tuple[Path, int]]:
    """Find companion files with no WAD links."""
    all_companions = db.get_all_companions_with_counts()
    orphans = []
    for comp in all_companions:
        if comp["wad_count"] == 0:
            p = Path(comp["path"])
            size = p.stat().st_size if p.is_file() else comp.get("size", 0)
            orphans.append((p, size))
    return orphans


def _remove_orphaned_companion_records() -> None:
    """Remove DB records for all orphaned companion files."""
    all_companions = db.get_all_companions_with_counts()
    for comp in all_companions:
        if comp["wad_count"] == 0:
            db.remove_companion(comp["id"])


def _find_orphaned_backups() -> list[tuple[Path, int]]:
    """Find backup zips whose WAD ID no longer exists in the database."""
    all_backups = list_all_backups()
    if not all_backups:
        return []

    wad_ids = {b["wad_id"] for b in all_backups}
    existing_ids = _get_existing_wad_ids(wad_ids)

    return [
        (b["path"], b["size"])
        for b in all_backups
        if b["wad_id"] not in existing_ids
    ]


def _get_gc_candidates() -> list[dict]:
    """Find finished/abandoned WADs eligible for GC (not gc_ignored)."""
    wads = db.search_wads(query="status:finished , status:abandoned")
    return [w for w in wads if not w.get("gc_ignore")]


def _build_data_dir_map() -> dict[int, Path]:
    """Scan the data directory once and return a {wad_id: path} mapping."""
    data_dir = get_data_dir()
    if not data_dir.is_dir():
        return {}

    result: dict[int, Path] = {}
    for entry in data_dir.iterdir():
        if entry.is_dir():
            wad_id = _parse_wad_id_prefix(entry.name)
            if wad_id is not None:
                result[wad_id] = entry
    return result


def _compute_data_size(data_dir: Path | None, opts: GcOptions) -> int:
    """Compute cleanable data size, respecting keep_saves/keep_demos."""
    if not data_dir or not data_dir.is_dir():
        return 0
    if not opts.keep_saves and not opts.keep_demos:
        return _dir_size(data_dir)

    # Selective: sum only files that would be deleted
    total = 0
    for f in data_dir.rglob("*"):
        if not f.is_file():
            continue
        suffix = f.suffix.lower()
        if opts.keep_saves and suffix in ALL_SAVE_EXTENSIONS:
            continue
        if opts.keep_demos and suffix == DEMO_EXTENSION:
            continue
        total += f.stat().st_size
    return total


def _get_wad_companion_info(wad_id: int) -> list[dict]:
    """Get companion files linked to a WAD with orphan-on-removal info."""
    companions = db.get_wad_companions(wad_id)
    result = []
    for comp in companions:
        p = Path(comp["path"])
        file_size = p.stat().st_size if p.is_file() else comp.get("size", 0)
        result.append({
            **comp,
            "file_size": file_size,
            "would_orphan": db.would_be_orphan(comp["id"], wad_id),
        })
    return result


def _wad_has_data(
    wad: dict,
    data_dir_map: dict[int, Path],
    opts: GcOptions,
) -> tuple[Path | None, int, Path | None, int, list[dict], int]:
    """Check what cleanable data exists for a WAD.

    Returns (data_dir, data_size, cache_path, cache_size, companions, companion_size).
    """
    data_dir = data_dir_map.get(wad["id"])
    data_size = _compute_data_size(data_dir, opts) if not opts.keep_data else 0

    cache_path = None
    cache_size = 0
    if not opts.keep_cache and wad.get("cached_path"):
        p = Path(wad["cached_path"])
        if p.is_file():
            cache_path = p
            cache_size = p.stat().st_size

    companions: list[dict] = []
    companion_size = 0
    if not opts.keep_companions:
        companions = _get_wad_companion_info(wad["id"])
        # Only count size for companions that would become orphaned (actually freeing disk)
        companion_size = sum(c["file_size"] for c in companions if c["would_orphan"])

    return data_dir, data_size, cache_path, cache_size, companions, companion_size


def _clean_wad_data(
    wad: dict,
    data_dir: Path | None,
    cache_path: Path | None,
    companions: list[dict],
    opts: GcOptions,
) -> int:
    """Delete data/cache/companions for a WAD. Returns bytes freed."""
    freed = 0

    if data_dir and data_dir.is_dir() and not opts.keep_data:
        if opts.keep_saves or opts.keep_demos:
            # Selective cleanup within data dir
            for f in list(data_dir.rglob("*")):
                if not f.is_file():
                    continue
                suffix = f.suffix.lower()
                if opts.keep_saves and suffix in ALL_SAVE_EXTENSIONS:
                    continue
                if opts.keep_demos and suffix == DEMO_EXTENSION:
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

    if cache_path and cache_path.is_file() and not opts.keep_cache:
        freed += cache_path.stat().st_size
        cache_path.unlink()
        db.clear_cached_path(wad["id"])

    # Clean companion files
    if companions and not opts.keep_companions:
        for comp in companions:
            db.unlink_companion(wad["id"], comp["id"])
            if db.is_orphan(comp["id"]):
                managed_path = db.remove_companion_with_path(comp["id"])
                if managed_path:
                    p = Path(managed_path)
                    if p.is_file():
                        freed += p.stat().st_size
                        p.unlink()

    # Clear live stats snapshot (data is archived in completions)
    if not opts.keep_data:
        db.update_wad(wad["id"], stats_snapshot=None)

    return freed


def _gc_orphans(
    orphans: list[tuple[Path, int]],
    *,
    label: str,
    delete_fn: Callable[[Path], None],
    dry_run: bool,
    yes: bool,
    post_delete_fn: Callable[[], None] | None = None,
) -> int:
    """Clean orphaned files or directories with confirmation."""
    total_size = sum(size for _, size in orphans)

    console.print(f"\n[bold]Orphaned {label} ({len(orphans)}):[/bold]\n")
    for path, size in orphans:
        console.print(f"  {path.name} ({_format_size(size)})")

    console.print(f"\n[bold]Subtotal:[/bold] {_format_size(total_size)}")

    if dry_run:
        return total_size

    if not yes:
        if not click.confirm(f"\nDelete orphaned {label}?"):
            console.print("[dim]Skipped[/dim]")
            return 0

    freed = 0
    for path, size in orphans:
        try:
            delete_fn(path)
            freed += size
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    if post_delete_fn:
        post_delete_fn()

    console.print(f"[green]Deleted {len(orphans)} orphaned {label}, freed {_format_size(freed)}[/green]")
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
@click.option("--keep-companions", is_flag=True, help="Don't delete companion files")
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
    keep_companions: bool,
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

    opts = GcOptions(
        keep_data=keep_data,
        keep_cache=keep_cache,
        keep_saves=keep_saves,
        keep_demos=keep_demos,
        keep_companions=keep_companions,
    )

    console.print("[bold]Scanning for garbage...[/bold]\n")

    total_freed = 0

    # Phase 1: Finished/abandoned WADs
    if not orphans_only:
        total_freed += _gc_finished_wads(dry_run=dry_run, yes=yes, opts=opts)

    # Phase 2: Orphaned data dirs
    orphan_dirs = _find_orphaned_data_dirs()
    if orphan_dirs:
        total_freed += _gc_orphans(
            orphan_dirs,
            label="data dirs",
            delete_fn=shutil.rmtree,
            dry_run=dry_run,
            yes=yes,
        )

    # Phase 3: Orphaned companion files
    if not opts.keep_companions:
        orphan_companions = _find_orphaned_companions()
        if orphan_companions:
            total_freed += _gc_orphans(
                orphan_companions,
                label="companion files",
                delete_fn=lambda p: p.unlink(),
                dry_run=dry_run,
                yes=yes,
                post_delete_fn=_remove_orphaned_companion_records,
            )

    # Phase 4: Orphaned backups
    orphan_backups = _find_orphaned_backups()
    if orphan_backups:
        total_freed += _gc_orphans(
            orphan_backups,
            label="backups",
            delete_fn=lambda p: p.unlink(),
            dry_run=dry_run,
            yes=yes,
        )

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

    for wad in wads:
        db.update_wad(wad["id"], gc_ignore=1 if ignore else 0)

    count = len(wads)
    if ignore:
        console.print(f"[green]Marked {count} WAD(s) as GC-ignored[/green]")
    else:
        console.print(f"[green]Removed GC-ignore from {count} WAD(s)[/green]")


def _gc_finished_wads(*, dry_run: bool, yes: bool, opts: GcOptions) -> int:
    """Clean data for finished/abandoned WADs. Returns bytes freed."""
    candidates = _get_gc_candidates()
    if not candidates:
        return 0

    # Single scan of data directory for all candidates
    data_dir_map = _build_data_dir_map()

    # Categorize and measure
    auto_clean = []  # idgames WADs (re-downloadable)
    interactive = []  # non-idgames WADs (need individual confirmation)

    for wad in candidates:
        data_dir, data_size, cache_path, cache_size, companions, companion_size = _wad_has_data(wad, data_dir_map, opts)
        total_size = data_size + cache_size + companion_size

        if total_size == 0:
            continue  # Nothing to clean

        entry = {
            "wad": wad,
            "data_dir": data_dir,
            "data_size": data_size,
            "cache_path": cache_path,
            "cache_size": cache_size,
            "companions": companions,
            "companion_size": companion_size,
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
        total_freed += _gc_auto_clean(auto_clean, dry_run=dry_run, yes=yes, opts=opts)

    # Interactive section (non-idgames WADs)
    if interactive:
        total_freed += _gc_interactive(interactive, dry_run=dry_run, opts=opts)

    return total_freed


def _gc_auto_clean(
    entries: list[dict],
    *,
    dry_run: bool,
    yes: bool,
    opts: GcOptions,
) -> int:
    """Clean idgames WADs with a single batch confirmation."""
    total_size = sum(e["total_size"] for e in entries)

    table = Table(title=f"Re-downloadable WADs ({len(entries)})")
    table.add_column("ID", style="dim")
    table.add_column("Title", style="cyan")
    table.add_column("Status", style="dim")
    table.add_column("Data", justify="right")
    table.add_column("Cache", justify="right")
    table.add_column("Companions", justify="right")

    for entry in entries:
        wad = entry["wad"]
        data_str = _format_size(entry["data_size"]) if entry["data_size"] else "-"
        cache_str = _format_size(entry["cache_size"]) if entry["cache_size"] else "-"
        comp_str = _format_size(entry["companion_size"]) if entry["companion_size"] else "-"
        table.add_row(
            str(wad["id"]),
            wad["title"],
            wad["status"],
            data_str,
            cache_str,
            comp_str,
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
        freed += _clean_wad_data(entry["wad"], entry["data_dir"], entry["cache_path"], entry["companions"], opts)

    console.print(f"[green]Cleaned {len(entries)} WAD(s), freed {_format_size(freed)}[/green]\n")
    return freed


def _gc_interactive(
    entries: list[dict],
    *,
    dry_run: bool,
    opts: GcOptions,
) -> int:
    """Prompt individually for non-idgames WADs. Returns bytes freed."""
    console.print(f"\n[bold]Non-re-downloadable WADs ({len(entries)}):[/bold]")
    console.print("[dim]y = clean, n = skip, i = ignore permanently[/dim]\n")

    freed = 0
    for entry in entries:
        wad = entry["wad"]
        data_str = _format_size(entry["data_size"]) if entry["data_size"] else ""
        cache_str = _format_size(entry["cache_size"]) if entry["cache_size"] else ""
        comp_str = _format_size(entry["companion_size"]) if entry["companion_size"] else ""
        size_parts = [s for s in (data_str, cache_str, comp_str) if s]
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
            freed += _clean_wad_data(wad, entry["data_dir"], entry["cache_path"], entry["companions"], opts)
        elif choice == "i":
            db.update_wad(wad["id"], gc_ignore=1)
            console.print("  [dim]Permanently ignored[/dim]")

    if freed > 0:
        console.print(f"\n[green]Freed {_format_size(freed)} from non-re-downloadable WADs[/green]")

    return freed

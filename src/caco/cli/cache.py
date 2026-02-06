"""Cache management commands: cache list/clear/clean."""

import sys
from pathlib import Path

import click
from rich.table import Table

from caco import db
from caco.config import get_cache_dir

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
)


# =============================================================================
# Internal helpers
# =============================================================================


def _format_size(size_bytes: int) -> str:
    """Format bytes as human-readable size."""
    for unit in ["B", "KB", "MB", "GB"]:
        if size_bytes < 1024:
            if unit == "B":
                return f"{size_bytes} {unit}"
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.1f} TB"


def _list_orphaned_files(cache_dir: Path, plain: bool) -> None:
    """List orphaned files in the cache directory."""
    if not cache_dir.exists():
        if plain:
            click.echo("No orphaned files")
        else:
            console.print("[dim]Cache directory does not exist[/dim]")
        return

    orphans = []
    total_size = 0

    for path in cache_dir.iterdir():
        if path.is_file():
            # Check if any WAD references this file
            wad = db.get_wad_by_cached_filename(path.name)
            if not wad:
                size = path.stat().st_size
                orphans.append((path, size))
                total_size += size

    if plain:
        click.echo("Filename\tSize")
        for path, size in orphans:
            click.echo(f"{path.name}\t{size}")
        click.echo(f"\nTotal: {total_size}")
    else:
        if not orphans:
            console.print("[dim]No orphaned files found[/dim]")
            return

        table = Table(title=f"Orphaned Files ({len(orphans)} files)")
        table.add_column("Filename")
        table.add_column("Size", justify="right")

        for path, size in orphans:
            table.add_row(path.name, _format_size(size))

        console.print(table)
        console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")
        console.print(f"[dim]Use 'caco cache clean' to remove orphaned files[/dim]")


def _clear_all_cache(cache_dir: Path, dry_run: bool, yes: bool) -> None:
    """Clear the entire cache."""
    cached_wads = db.get_cached_wads()
    total_size = 0
    files_to_delete = []

    for wad in cached_wads:
        # Only delete idgames sources - they can always be re-downloaded
        # Local files are the user's originals, URLs may not be re-downloadable
        if wad.get("source_type") != "idgames":
            continue

        path = Path(wad["cached_path"])
        if path.exists():
            size = path.stat().st_size
            total_size += size
            files_to_delete.append((wad, path, size))

    if not files_to_delete:
        console.print("[dim]Cache is empty (no idgames WADs cached)[/dim]")
        return

    # Preview
    console.print(f"\n[bold]Files to delete ({len(files_to_delete)}):[/bold]\n")
    for wad, path, size in files_to_delete[:10]:
        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']} ({_format_size(size)})")
    if len(files_to_delete) > 10:
        console.print(f"  [dim]... and {len(files_to_delete) - 10} more[/dim]")

    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        if not click.confirm("\nDelete all cached files?"):
            console.print("[dim]Cancelled[/dim]")
            return

    # Delete files and update database
    deleted = 0
    freed = 0
    for wad, path, size in files_to_delete:
        try:
            path.unlink()
            freed += size
            deleted += 1
            db.clear_cached_path(wad["id"])
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"\n[green]Deleted {deleted} file(s), freed {_format_size(freed)}[/green]")


def _clear_specific_cache(query: str, cache_dir: Path, dry_run: bool, yes: bool) -> None:
    """Clear cache for specific WADs."""
    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return

    # Filter to only cached WADs (skip local source and non-cached)
    files_to_delete = []
    total_size = 0

    for wad in wads:
        # Only delete idgames sources - they can always be re-downloaded
        if wad.get("source_type") != "idgames":
            continue
        if not wad.get("cached_path"):
            continue
        path = Path(wad["cached_path"])
        if path.exists():
            size = path.stat().st_size
            total_size += size
            files_to_delete.append((wad, path, size))

    if not files_to_delete:
        console.print("[dim]Selected WAD(s) are not cached or not from idgames[/dim]")
        return

    # Preview
    console.print(f"\n[bold]Files to delete ({len(files_to_delete)}):[/bold]\n")
    for wad, path, size in files_to_delete[:10]:
        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']} ({_format_size(size)})")
    if len(files_to_delete) > 10:
        console.print(f"  [dim]... and {len(files_to_delete) - 10} more[/dim]")

    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    # Delete
    deleted = 0
    freed = 0
    for wad, path, size in files_to_delete:
        try:
            path.unlink()
            freed += size
            deleted += 1
            db.clear_cached_path(wad["id"])
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"\n[green]Deleted {deleted} file(s), freed {_format_size(freed)}[/green]")


# =============================================================================
# Cache group
# =============================================================================


@cli.group(name="cache")
def cache_cmd():
    """Manage WAD cache."""
    pass


@cache_cmd.command(name="list")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
@click.option("--orphans", is_flag=True, help="Show orphaned files (not in database)")
def cache_list(plain: bool, orphans: bool):
    """List cached WAD files and total cache size."""
    cache_dir = get_cache_dir()

    if orphans:
        _list_orphaned_files(cache_dir, plain)
        return

    cached_wads = db.get_cached_wads()
    total_size = 0
    entries = []

    for wad in cached_wads:
        path = Path(wad["cached_path"])
        if path.exists():
            size = path.stat().st_size
            total_size += size
            entries.append({
                "id": wad["id"],
                "title": wad["title"],
                "filename": path.name,
                "size": size,
                "path": str(path),
            })

    if plain:
        click.echo("ID\tTitle\tFilename\tSize")
        for e in entries:
            click.echo(f"{e['id']}\t{e['title']}\t{e['filename']}\t{e['size']}")
        click.echo(f"\nTotal: {total_size}")
    else:
        if not entries:
            console.print("[dim]Cache is empty[/dim]")
            console.print(f"[dim]Cache location: {cache_dir}[/dim]")
            return

        table = Table(title=f"Cached WADs ({len(entries)} files)")
        table.add_column("ID", style="dim")
        table.add_column("Title", style="cyan")
        table.add_column("Filename")
        table.add_column("Size", justify="right")

        for e in entries:
            table.add_row(
                str(e["id"]),
                e["title"],
                e["filename"],
                _format_size(e["size"]),
            )

        console.print(table)
        console.print(f"\n[bold]Total cache size:[/bold] {_format_size(total_size)}")
        console.print(f"[dim]Cache location: {cache_dir}[/dim]")


@cache_cmd.command(name="clear")
@click.argument("query", required=False)
@click.option("--all", "clear_all", is_flag=True, help="Clear entire cache")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def cache_clear(query: str | None, clear_all: bool, dry_run: bool, yes: bool):
    """Remove cached WAD files.

    Without arguments, shows help. Use --all to clear entire cache,
    or specify QUERY to clear specific WADs.

    \b
    Examples:
        caco cache clear --all           # Clear entire cache
        caco cache clear 1,3,5           # Clear specific WAD IDs
        caco cache clear status:finished # Clear all finished WADs
    """
    if not query and not clear_all:
        click.echo(click.get_current_context().get_help())
        return

    cache_dir = get_cache_dir()

    if clear_all:
        _clear_all_cache(cache_dir, dry_run, yes)
    else:
        _clear_specific_cache(query, cache_dir, dry_run, yes)


@cache_cmd.command(name="clean")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def cache_clean(dry_run: bool, yes: bool):
    """Remove orphaned files from cache.

    Orphaned files are files in the cache directory that are not
    tracked by any WAD in the database.
    """
    cache_dir = get_cache_dir()
    if not cache_dir.exists():
        console.print("[dim]Cache directory does not exist[/dim]")
        return

    # Find all orphaned files
    orphans = []
    total_size = 0

    for path in cache_dir.iterdir():
        if path.is_file():
            wad = db.get_wad_by_cached_filename(path.name)
            if not wad:
                size = path.stat().st_size
                orphans.append((path, size))
                total_size += size

    if not orphans:
        console.print("[dim]No orphaned files found[/dim]")
        return

    # Preview
    console.print(f"\n[bold]Orphaned files ({len(orphans)}):[/bold]\n")
    for path, size in orphans[:10]:
        console.print(f"  {path.name} ({_format_size(size)})")
    if len(orphans) > 10:
        console.print(f"  [dim]... and {len(orphans) - 10} more[/dim]")

    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        if not click.confirm("\nDelete orphaned files?"):
            console.print("[dim]Cancelled[/dim]")
            return

    # Delete
    deleted = 0
    freed = 0
    for path, size in orphans:
        try:
            path.unlink()
            freed += size
            deleted += 1
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"\n[green]Deleted {deleted} orphaned file(s), freed {_format_size(freed)}[/green]")

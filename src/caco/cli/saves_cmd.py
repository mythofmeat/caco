"""Save game management commands: saves list/backup/restore/clean/backups."""

import sys

import click
from rich.table import Table

from caco.config import find_wad_data_dir, get_wad_data_dir
from caco.utils import format_size as _format_size

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
)


@cli.group(name="saves")
def saves_cmd():
    """Manage WAD save files and backups."""
    pass


@saves_cmd.command(name="list")
@click.argument("query")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match")
def saves_list(query: str, plain: bool, yes: bool):
    """List save files for a WAD.

    \b
    Examples:
        caco saves list 1
        caco saves list "Eviternity"
        caco saves list title:sunlust --plain
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    data_dir = find_wad_data_dir(wad["id"])
    if not data_dir:
        if plain:
            click.echo("No data directory")
        else:
            console.print(f"[dim]No data directory for '{wad['title']}'[/dim]")
        return

    from caco.saves import find_save_files

    saves = find_save_files(data_dir)

    if plain:
        click.echo("Name\tRelPath\tSize\tModified")
        for s in saves:
            click.echo(f"{s['name']}\t{s['rel_path']}\t{s['size']}\t{s['mtime_iso']}")
        return

    if not saves:
        console.print(f"[dim]No save files found for '{wad['title']}'[/dim]")
        console.print(f"[dim]Data directory: {data_dir}[/dim]")
        return

    total_size = sum(s["size"] for s in saves)

    table = Table(title=f"Save Files — {wad['title']} ({len(saves)} files)")
    table.add_column("Name")
    table.add_column("Path", style="dim")
    table.add_column("Size", justify="right")
    table.add_column("Modified", style="dim")

    for s in saves:
        table.add_row(
            s["name"],
            s["rel_path"],
            _format_size(s["size"]),
            s["mtime_iso"][:19].replace("T", " "),
        )

    console.print(table)
    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")
    console.print(f"[dim]Data directory: {data_dir}[/dim]")


@saves_cmd.command(name="backup")
@click.argument("query")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match")
def saves_backup(query: str, yes: bool):
    """Create a backup of a WAD's data directory.

    Backs up the entire data directory (saves, stats, configs) as a zip file.

    \b
    Examples:
        caco saves backup 1
        caco saves backup "Eviternity"
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    data_dir = find_wad_data_dir(wad["id"])
    if not data_dir:
        err_console.print(f"[red]No data directory for '{wad['title']}'[/red]")
        sys.exit(1)

    from caco.saves import create_backup

    backup_path = create_backup(wad["id"], wad["title"], data_dir)
    size = backup_path.stat().st_size
    console.print(f"[green]Backup created:[/green] {backup_path.name} ({_format_size(size)})")


@saves_cmd.command(name="restore")
@click.argument("query")
@click.argument("backup", required=False)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def saves_restore(query: str, backup: str | None, yes: bool):
    """Restore a WAD's data directory from a backup.

    If BACKUP is omitted, restores from the most recent backup.
    BACKUP can be a filename or absolute path.

    \b
    Examples:
        caco saves restore 1                    # Latest backup
        caco saves restore 1 1_eviternity_20240101_120000.zip
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    from caco.saves import resolve_backup_path, restore_backup

    backup_path = resolve_backup_path(wad["id"], backup)
    if not backup_path or not backup_path.is_file():
        err_console.print(f"[red]No backup found for '{wad['title']}'[/red]")
        if not backup:
            err_console.print("[dim]Create one first with: caco saves backup[/dim]")
        sys.exit(1)

    data_dir = find_wad_data_dir(wad["id"]) or get_wad_data_dir(wad["id"], wad["title"])

    # Warn if data dir already has files
    if data_dir.is_dir() and any(data_dir.iterdir()):
        if not yes:
            console.print(f"[yellow]Data directory already exists with files: {data_dir}[/yellow]")
            if not click.confirm("Overwrite existing files?"):
                console.print("[dim]Cancelled[/dim]")
                return

    file_count = restore_backup(backup_path, data_dir)
    console.print(f"[green]Restored {file_count} file(s) from {backup_path.name}[/green]")


@saves_cmd.command(name="clean")
@click.argument("query")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def saves_clean(query: str, dry_run: bool, yes: bool):
    """Delete save files for a WAD, keeping stats and configs.

    Only removes files with known save extensions (.dsg, .zds).

    \b
    Examples:
        caco saves clean 1 --dry-run    # Preview deletions
        caco saves clean 1 -y           # Delete without confirmation
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    data_dir = find_wad_data_dir(wad["id"])
    if not data_dir:
        console.print(f"[dim]No data directory for '{wad['title']}'[/dim]")
        return

    from caco.saves import find_save_files, clean_save_files

    saves = find_save_files(data_dir)
    if not saves:
        console.print(f"[dim]No save files found for '{wad['title']}'[/dim]")
        return

    total_size = sum(s["size"] for s in saves)

    console.print(f"\n[bold]Save files to delete ({len(saves)}):[/bold]\n")
    for s in saves[:10]:
        console.print(f"  {s['rel_path']} ({_format_size(s['size'])})")
    if len(saves) > 10:
        console.print(f"  [dim]... and {len(saves) - 10} more[/dim]")
    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        if not click.confirm("\nDelete these save files?"):
            console.print("[dim]Cancelled[/dim]")
            return

    deleted = clean_save_files(data_dir)
    console.print(f"\n[green]Deleted {len(deleted)} save file(s)[/green]")


@saves_cmd.command(name="backups")
@click.argument("query", required=False)
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match")
def saves_backups(query: str | None, plain: bool, yes: bool):
    """List existing backup files.

    Without QUERY, lists all backups. With QUERY, lists backups for a specific WAD.

    \b
    Examples:
        caco saves backups              # All backups
        caco saves backups 1            # Backups for WAD 1
        caco saves backups --plain      # TSV output
    """
    from caco.saves import list_all_backups, list_backups

    if query:
        wads = resolve_wad_query(query, mode="pick", yes=yes)
        if not wads:
            return
        wad = wads[0]
        backups = list_backups(wad["id"])
        title = f"Backups — {wad['title']}"
    else:
        backups = list_all_backups()
        title = "All Backups"

    if plain:
        click.echo("Name\tSize\tCreated")
        for b in backups:
            click.echo(f"{b['name']}\t{b['size']}\t{b['created_iso']}")
        return

    if not backups:
        console.print("[dim]No backups found[/dim]")
        return

    total_size = sum(b["size"] for b in backups)

    table = Table(title=f"{title} ({len(backups)} files)")
    table.add_column("Name")
    if not query:
        table.add_column("WAD ID", style="dim", justify="right")
    table.add_column("Size", justify="right")
    table.add_column("Created", style="dim")

    for b in backups:
        row = [b["name"]]
        if not query:
            row.append(str(b.get("wad_id", "?")))
        row.append(_format_size(b["size"]))
        row.append(b["created_iso"][:19].replace("T", " "))
        table.add_row(*row)

    console.print(table)
    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

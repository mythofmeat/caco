"""IWAD management commands: iwad list/add/remove/scan."""

import sqlite3
import sys
from pathlib import Path

import click
from rich.table import Table

from caco import db
from caco.config import get_iwad_dirs
from caco.db._iwads import (
    KNOWN_IWAD_FILENAMES,
    KNOWN_IWADS,
    _compute_md5,
    identify_iwad,
)

from caco.cli import cli, console, err_console


@cli.group(name="iwad")
def iwad_cmd():
    """Manage IWAD registry."""
    pass


@iwad_cmd.command(name="list")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
def iwad_list(plain: bool):
    """List registered IWADs."""
    iwads = db.get_all_iwads()

    if plain:
        click.echo("Name\tTitle\tPath\tMD5")
        for iwad in iwads:
            click.echo(
                f"{iwad['name']}\t{iwad.get('title') or ''}\t{iwad['path']}\t{iwad.get('md5') or ''}"
            )
        return

    if not iwads:
        console.print("[dim]No IWADs registered[/dim]")
        console.print("[dim]Use 'caco iwad scan' to discover IWADs or 'caco iwad add' to register one[/dim]")
        return

    table = Table(title=f"Registered IWADs ({len(iwads)})")
    table.add_column("Name", style="cyan")
    table.add_column("Title")
    table.add_column("Path", style="dim")
    table.add_column("MD5", style="dim")

    for iwad in iwads:
        path_str = iwad["path"]
        # Check if file still exists
        exists = Path(path_str).exists()
        if not exists:
            path_str = f"[red]{path_str} (missing)[/red]"

        table.add_row(
            iwad["name"],
            iwad.get("title") or "-",
            path_str,
            (iwad.get("md5") or "-")[:12] + "..." if iwad.get("md5") else "-",
        )

    console.print(table)


@iwad_cmd.command(name="add")
@click.argument("path", type=click.Path(exists=True))
@click.option("--name", "iwad_name", help="Override auto-detected short name")
def iwad_add(path: str, iwad_name: str | None):
    """Register an IWAD file.

    Auto-detects the IWAD by MD5 checksum, falling back to filename.
    Use --name to override the detected name.

    \b
    Examples:
        caco iwad add ~/games/doom2.wad
        caco iwad add ~/wads/custom.wad --name mycustom
    """
    resolved = Path(path).expanduser().resolve()
    abs_path = str(resolved)

    # Check if already registered by path
    existing = db.get_iwad_by_path(abs_path)
    if existing:
        err_console.print(
            f"[yellow]Already registered:[/yellow] {existing['name']} ({abs_path})"
        )
        return

    # Compute MD5 and try to identify
    md5 = _compute_md5(resolved)
    detected = identify_iwad(resolved)

    if detected:
        name, title = detected
    else:
        name = resolved.stem.lower()
        title = None

    # --name overrides auto-detected name
    if iwad_name:
        name = iwad_name

    # Check if name already taken
    existing_name = db.get_iwad(name)
    if existing_name:
        err_console.print(
            f"[red]Name '{name}' already registered[/red] (path: {existing_name['path']})"
        )
        err_console.print("[dim]Use --name to specify a different name[/dim]")
        sys.exit(1)

    try:
        db.add_iwad(name=name, path=abs_path, title=title, md5=md5)
    except sqlite3.IntegrityError:
        err_console.print(f"[red]Failed to register '{name}' — already exists[/red]")
        sys.exit(1)

    if title:
        console.print(f"[green]Registered:[/green] {name} — {title} ({abs_path})")
    else:
        console.print(f"[green]Registered:[/green] {name} ({abs_path})")


@iwad_cmd.command(name="remove")
@click.argument("name")
def iwad_remove(name: str):
    """Unregister an IWAD by name.

    \b
    Examples:
        caco iwad remove doom2
    """
    if db.remove_iwad(name):
        console.print(f"[green]Removed:[/green] {name}")
    else:
        err_console.print(f"[red]IWAD '{name}' not found[/red]")
        sys.exit(1)


@iwad_cmd.command(name="scan")
@click.option("--dir", "scan_dir", type=click.Path(exists=True, file_okay=False), help="Directory to scan (default: iwad_dirs)")
@click.option("--yes", "-y", is_flag=True, help="Register all discovered IWADs without prompting")
def iwad_scan(scan_dir: str | None, yes: bool):
    """Scan directories for known IWADs.

    Without --dir, scans all directories in the iwad_dirs config.
    Identifies IWADs by MD5 checksum, falling back to filename.

    \b
    Examples:
        caco iwad scan
        caco iwad scan --dir ~/games/iwads
        caco iwad scan --yes
    """
    if scan_dir:
        dirs = [Path(scan_dir).expanduser().resolve()]
    else:
        dirs = get_iwad_dirs()
        if not dirs:
            err_console.print("[yellow]No iwad_dirs configured[/yellow]")
            err_console.print("[dim]Set iwad_dirs in config: caco config iwad_dirs '[\"/path/to/iwads\"]'[/dim]")
            err_console.print("[dim]Or use: caco iwad scan --dir /path/to/iwads[/dim]")
            return

    # Collect all .wad files
    discovered: list[tuple[Path, str, str, str]] = []  # (path, name, title, md5)

    for d in dirs:
        if not d.is_dir():
            continue
        for wad_file in sorted(d.iterdir()):
            if not wad_file.is_file():
                continue
            if wad_file.suffix.lower() != ".wad":
                continue

            abs_path = str(wad_file.resolve())

            # Skip already registered
            if db.get_iwad_by_path(abs_path):
                continue

            md5 = _compute_md5(wad_file)

            # Try MD5 lookup
            if md5 in KNOWN_IWADS:
                name, title = KNOWN_IWADS[md5]
            else:
                # Try filename fallback
                fname = wad_file.name.lower()
                if fname in KNOWN_IWAD_FILENAMES:
                    name, title = KNOWN_IWAD_FILENAMES[fname]
                else:
                    continue  # Unknown file, skip

            # Skip if name already registered
            if db.get_iwad(name):
                continue

            discovered.append((wad_file, name, title, md5))

    if not discovered:
        console.print("[dim]No new IWADs found[/dim]")
        return

    console.print(f"\n[bold]Discovered {len(discovered)} IWAD(s):[/bold]\n")
    for wad_path, name, title, md5 in discovered:
        console.print(f"  [cyan]{name}[/cyan] — {title}")
        console.print(f"    [dim]{wad_path}[/dim]")

    if yes:
        # Register all
        registered = 0
        for wad_path, name, title, md5 in discovered:
            try:
                db.add_iwad(name=name, path=str(wad_path.resolve()), title=title, md5=md5)
                registered += 1
            except sqlite3.IntegrityError:
                pass
        console.print(f"\n[green]Registered {registered} IWAD(s)[/green]")
    else:
        # Prompt for each
        console.print()
        registered = 0
        for wad_path, name, title, md5 in discovered:
            if click.confirm(f"  Register {name} ({title})?", default=True):
                try:
                    db.add_iwad(name=name, path=str(wad_path.resolve()), title=title, md5=md5)
                    registered += 1
                except sqlite3.IntegrityError:
                    err_console.print(f"  [yellow]Skipped (already exists): {name}[/yellow]")
        if registered:
            console.print(f"\n[green]Registered {registered} IWAD(s)[/green]")
        else:
            console.print("\n[dim]No IWADs registered[/dim]")

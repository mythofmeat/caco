"""Companion file management commands."""

import sys

import click

from caco import db
from caco.cli import cli, console, err_console, resolve_wad_query
from caco.config import get_companion_orphan_cleanup
from caco.services.companion_service import register_companion, unregister_companion


def _resolve_wad_and_companion(query: str, filename: str):
    """Resolve a WAD query and find a companion by filename, or exit."""
    wads = resolve_wad_query(query, mode="pick")
    if not wads:
        return None, None

    wad = wads[0]
    comp = db.get_wad_companion_by_filename(wad["id"], filename)
    if not comp:
        err_console.print(f"[red]No companion '{filename}' found for {wad['title']}[/red]")
        sys.exit(1)
    return wad, comp


@cli.group()
def companion():
    """Manage companion files (DEH patches, music WADs, etc.)."""
    pass


@companion.command()
@click.argument("query")
@click.argument("file", type=click.Path(exists=True))
def add(query: str, file: str):
    """Add a companion file to a WAD.

    \b
    Examples:
      caco companion add id:1 /path/to/music.wad
      caco companion add "Eviternity" /path/to/patch.deh
    """
    wads = resolve_wad_query(query, mode="pick")
    if not wads:
        return

    wad = wads[0]
    companion_id, filename = register_companion(file, wad["id"])
    console.print(f"[green]Added companion '{filename}' to {wad['title']}[/green]")


@companion.command()
@click.argument("query")
@click.argument("filename")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for orphan deletion")
def rm(query: str, filename: str, yes: bool):
    """Remove a companion file from a WAD.

    FILENAME matches by the companion's original filename (not full path).

    \b
    Examples:
      caco companion rm id:1 music.wad
      caco companion rm "Eviternity" patch.deh
    """
    wad, comp = _resolve_wad_and_companion(query, filename)
    if not wad:
        return

    companion_id = comp["id"]

    # Determine orphan policy using a read-only check
    orphan_policy = get_companion_orphan_cleanup()
    if orphan_policy == "ask" and db.would_be_orphan(companion_id, wad["id"]):
        if yes:
            orphan_policy = "delete"
        elif click.confirm(
            f"'{filename}' is not linked to any other WADs. Delete the managed file?"
        ):
            orphan_policy = "delete"
        else:
            orphan_policy = "keep"

    deleted = unregister_companion(wad["id"], companion_id, orphan_policy=orphan_policy)

    if deleted:
        console.print(f"[green]Removed and deleted companion '{filename}' from {wad['title']}[/green]")
    else:
        console.print(f"[green]Removed companion '{filename}' from {wad['title']}[/green]")


@companion.command()
@click.argument("query")
@click.argument("filename")
def enable(query: str, filename: str):
    """Enable a companion file for a WAD.

    \b
    Examples:
      caco companion enable id:1 music.wad
    """
    wad, comp = _resolve_wad_and_companion(query, filename)
    if not wad:
        return

    db.set_companion_enabled(wad["id"], comp["id"], True)
    console.print(f"[green]Enabled companion '{filename}' for {wad['title']}[/green]")


@companion.command()
@click.argument("query")
@click.argument("filename")
def disable(query: str, filename: str):
    """Disable a companion file for a WAD (keeps it linked but won't load).

    \b
    Examples:
      caco companion disable id:1 music.wad
    """
    wad, comp = _resolve_wad_and_companion(query, filename)
    if not wad:
        return

    db.set_companion_enabled(wad["id"], comp["id"], False)
    console.print(f"[yellow]Disabled companion '{filename}' for {wad['title']}[/yellow]")


@companion.command(name="ls")
@click.argument("query", required=False)
@click.option("--plain", is_flag=True, help="Plain TSV output")
def companion_ls(query: str | None, plain: bool):
    """List companion files for a WAD or all companions.

    \b
    Examples:
      caco companion ls id:1          # List for a specific WAD
      caco companion ls               # List all registered companions
      caco companion ls --plain       # TSV output
    """
    if query:
        wads = resolve_wad_query(query, mode="pick")
        if not wads:
            return

        wad = wads[0]
        companions = db.get_wad_companions(wad["id"])

        if not companions:
            console.print(f"[dim]No companion files for {wad['title']}[/dim]")
            return

        if plain:
            print("Filename\tEnabled\tOrder\tPath")
            for comp in companions:
                enabled_str = "enabled" if comp["enabled"] else "disabled"
                print(f"{comp['filename']}\t{enabled_str}\t{comp['load_order']}\t{comp.get('path') or ''}")
        else:
            from rich.table import Table

            table = Table(title=f"Companion files for {wad['title']}")
            table.add_column("Filename", style="cyan")
            table.add_column("Status")
            table.add_column("Order", justify="right")
            table.add_column("Path", style="dim")

            for comp in companions:
                status = "[green]enabled[/green]" if comp["enabled"] else "[red]disabled[/red]"
                table.add_row(
                    comp["filename"],
                    status,
                    str(comp["load_order"]),
                    comp.get("path") or "[dim]missing[/dim]",
                )

            console.print(table)
    else:
        # List all registered companions (single query with counts)
        companions = db.get_all_companions_with_counts()

        if not companions:
            console.print("[dim]No companion files registered[/dim]")
            return

        if plain:
            print("ID\tFilename\tMD5\tPath")
            for comp in companions:
                print(f"{comp['id']}\t{comp['filename']}\t{comp.get('md5') or ''}\t{comp.get('path') or ''}")
        else:
            from rich.table import Table

            table = Table(title=f"All companion files ({len(companions)})")
            table.add_column("ID", style="dim")
            table.add_column("Filename", style="cyan")
            table.add_column("WADs", justify="right")
            table.add_column("Path", style="dim")

            for comp in companions:
                table.add_row(
                    str(comp["id"]),
                    comp["filename"],
                    str(comp["wad_count"]),
                    comp.get("path") or "[dim]missing[/dim]",
                )

            console.print(table)

"""Demo recording management commands: demos list/play/clean."""

import shutil
import subprocess
import sys

import click
from rich.table import Table

from caco.config import (
    find_wad_data_dir,
    get_default_sourceport,
    get_iwad,
    resolve_iwad,
    resolve_sourceport,
)
from caco.utils import format_size as _format_size

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
)


@cli.group(name="demos")
def demos_cmd():
    """Manage WAD demo recordings."""
    pass


@demos_cmd.command(name="list")
@click.argument("query")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match")
def demos_list(query: str, plain: bool, yes: bool):
    """List demo files for a WAD.

    \b
    Examples:
        caco demos list 1
        caco demos list "Eviternity"
        caco demos list title:sunlust --plain
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

    from caco.demos import find_demo_files

    demos = find_demo_files(data_dir)

    if plain:
        click.echo("Name\tRelPath\tSize\tModified")
        for d in demos:
            click.echo(f"{d['name']}\t{d['rel_path']}\t{d['size']}\t{d['mtime_iso']}")
        return

    if not demos:
        console.print(f"[dim]No demo files found for '{wad['title']}'[/dim]")
        console.print(f"[dim]Data directory: {data_dir}[/dim]")
        return

    total_size = sum(d["size"] for d in demos)

    table = Table(title=f"Demos — {wad['title']} ({len(demos)} files)")
    table.add_column("Name")
    table.add_column("Path", style="dim")
    table.add_column("Size", justify="right")
    table.add_column("Modified", style="dim")

    for d in demos:
        table.add_row(
            d["name"],
            d["rel_path"],
            _format_size(d["size"]),
            d["mtime_iso"][:19].replace("T", " "),
        )

    console.print(table)
    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")
    console.print(f"[dim]Data directory: {data_dir}[/dim]")


@demos_cmd.command(name="play")
@click.argument("query")
@click.argument("demo", required=False)
@click.option("--sourceport", "-p", help="Sourceport to use")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match")
def demos_play(query: str, demo: str | None, sourceport: str | None, yes: bool):
    """Play back a recorded demo.

    If DEMO is omitted, plays the most recent demo.
    DEMO can be a filename or absolute path.

    \b
    Examples:
        caco demos play 1                  # Most recent demo
        caco demos play 1 mydemo.lmp       # Specific demo
        caco demos play 1 -p dsda-doom     # With specific sourceport
    """
    from pathlib import Path

    from caco import db
    from caco.player import get_wad_path

    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    # Resolve the demo file
    data_dir = find_wad_data_dir(wad["id"])

    if demo:
        # Absolute path
        candidate = Path(demo)
        if candidate.is_absolute():
            demo_path = candidate
        elif data_dir:
            # Look in demos dir
            from caco.demos import get_demos_dir

            demos_dir = get_demos_dir(data_dir)
            demo_path = demos_dir / demo
            if not demo_path.exists() and not demo.endswith(".lmp"):
                demo_path = demos_dir / (demo + ".lmp")
        else:
            err_console.print(f"[red]No data directory for '{wad['title']}'[/red]")
            sys.exit(1)
    else:
        # Use most recent demo
        if not data_dir:
            err_console.print(f"[red]No data directory for '{wad['title']}'[/red]")
            sys.exit(1)

        from caco.demos import find_demo_files

        demos = find_demo_files(data_dir)
        if not demos:
            err_console.print(f"[red]No demo files found for '{wad['title']}'[/red]")
            err_console.print("[dim]Record one with: caco play --record[/dim]")
            sys.exit(1)

        # Most recent by mtime
        demos.sort(key=lambda d: d["mtime_iso"], reverse=True)
        demo_path = demos[0]["path"]

    if not demo_path.is_file():
        err_console.print(f"[red]Demo file not found: {demo_path}[/red]")
        sys.exit(1)

    # Resolve sourceport
    port = sourceport or wad.get("custom_sourceport") or get_default_sourceport()
    if not port:
        err_console.print("[red]No sourceport configured[/red]")
        sys.exit(1)

    port = resolve_sourceport(port)
    if not shutil.which(port) and not Path(port).is_file():
        err_console.print(f"[red]Sourceport '{port}' not found[/red]")
        sys.exit(1)

    # Build command
    cmd = [port]

    # Add IWAD
    iwad = wad.get("custom_iwad") or get_iwad()
    if iwad:
        cmd.extend(["-iwad", resolve_iwad(iwad)])

    # Add the WAD file
    wad_path = get_wad_path(wad)
    if wad_path:
        cmd.extend(["-file", str(wad_path)])

    # Add playdemo
    cmd.extend(["-playdemo", str(demo_path)])

    console.print(f"[cyan]Playing demo: {demo_path.name}[/cyan]")

    try:
        proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL)
        proc.wait()
    except FileNotFoundError:
        err_console.print(f"[red]Sourceport '{port}' not found[/red]")
        sys.exit(1)


@demos_cmd.command(name="clean")
@click.argument("query")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def demos_clean(query: str, dry_run: bool, yes: bool):
    """Delete demo files for a WAD.

    \b
    Examples:
        caco demos clean 1 --dry-run    # Preview deletions
        caco demos clean 1 -y           # Delete without confirmation
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    data_dir = find_wad_data_dir(wad["id"])
    if not data_dir:
        console.print(f"[dim]No data directory for '{wad['title']}'[/dim]")
        return

    from caco.demos import find_demo_files, clean_demo_files

    demos = find_demo_files(data_dir)
    if not demos:
        console.print(f"[dim]No demo files found for '{wad['title']}'[/dim]")
        return

    total_size = sum(d["size"] for d in demos)

    console.print(f"\n[bold]Demo files to delete ({len(demos)}):[/bold]\n")
    for d in demos[:10]:
        console.print(f"  {d['rel_path']} ({_format_size(d['size'])})")
    if len(demos) > 10:
        console.print(f"  [dim]... and {len(demos) - 10} more[/dim]")
    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        if not click.confirm("\nDelete these demo files?"):
            console.print("[dim]Cancelled[/dim]")
            return

    deleted = clean_demo_files(data_dir)
    console.print(f"\n[green]Deleted {len(deleted)} demo file(s)[/green]")

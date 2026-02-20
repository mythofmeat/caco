"""Play command."""

import sys

import click
from rich.progress import Progress, BarColumn, DownloadColumn, TransferSpeedColumn

from caco import db
from caco.config import get_default_sourceport
from caco.player import play, format_duration

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
    _complete_query,
)


@cli.command()
@click.argument("query", required=False, shell_complete=_complete_query)
@click.option("--sourceport", "-p", help="Sourceport to use")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
@click.argument("extra_args", nargs=-1)
def play_cmd(query: str | None, sourceport: str | None, yes: bool, extra_args: tuple[str, ...]):
    """Play a WAD by ID or query (e.g., 'caco play 1' or 'caco play filename:tnto').

    With no arguments, plays the most recently played WAD.
    """
    if query:
        wads = resolve_wad_query(query, mode="pick", yes=yes)
        if not wads:
            return  # User cancelled
        wad = wads[0]
    else:
        # No query - play most recently played WAD
        wad = db.get_most_recently_played()
        if not wad:
            err_console.print("[yellow]No play history yet. Specify a WAD to play.[/yellow]")
            return
    wad_id = wad["id"]

    port = sourceport or get_default_sourceport()
    if not port:
        err_console.print("[red]No sourceport specified. Use --sourceport or set default with 'caco config sourceport <path>'[/red]")
        sys.exit(1)

    console.print(f"[cyan]Playing {wad['title']}...[/cyan]")

    # Create a Rich progress callback for download display
    _progress = [None]  # Mutable container for lazy init

    def _progress_callback(downloaded: int, total: int | None, filename: str) -> None:
        if _progress[0] is None:
            _progress[0] = Progress(
                "[progress.description]{task.description}",
                BarColumn(),
                DownloadColumn(),
                TransferSpeedColumn(),
                console=console,
            )
            _progress[0].start()
            _progress[0]._task = _progress[0].add_task(f"Downloading {filename}", total=total)
        _progress[0].update(_progress[0]._task, completed=downloaded, total=total)

    try:
        duration = play(
            wad_id, sourceport=port, extra_args=list(extra_args),
            progress_callback=_progress_callback,
        )
        if duration:
            console.print(f"[green]Session ended:[/green] {format_duration(duration)}")
    except Exception as e:
        err_console.print(f"[red]Error: {e}[/red]")
        sys.exit(1)
    finally:
        if _progress[0] is not None:
            _progress[0].stop()


# Alias 'play' to 'play_cmd'
cli.add_command(play_cmd, name="play")

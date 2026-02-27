"""Play command."""

import sys

import click
from rich.progress import Progress, BarColumn, DownloadColumn, TransferSpeedColumn

from typing import Any

from caco import db
from caco.config import get_default_sourceport
from caco.player import play, play_iwad, format_duration, PlayResult

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
    _complete_query,
)


def _check_sourceport(sourceport: str | None) -> str:
    """Resolve sourceport or exit with a helpful error listing detected ports."""
    port = sourceport or get_default_sourceport()
    if port:
        return port

    from caco.sourceports import detect_sourceports

    msg = "No sourceport configured."
    detected = detect_sourceports()
    if detected:
        names = ", ".join(exe for exe, _path, _fam in detected)
        msg += f" Found on PATH: {names}."
    msg += "\nSet one with: caco config -e"
    err_console.print(f"[red]{msg}[/red]")
    sys.exit(1)


@cli.command(name="play")
@click.argument("query", required=False, shell_complete=_complete_query)
@click.option("--sourceport", "-p", help="Sourceport to use")
@click.option("--first", "-1", is_flag=True, help="Auto-select first match if multiple")
@click.option("--iwad", "iwad_name", type=str, help="Play an IWAD directly (e.g., --iwad doom2)")
@click.option("--complevel", "-c", type=str, help="Override complevel (int or alias: vanilla, boom, mbf, mbf21)")
@click.option("--config", "-C", "config_profile", type=str, help="Sourceport config profile name")
@click.argument("extra_args", nargs=-1)
def play_cmd(query: str | None, sourceport: str | None, first: bool, iwad_name: str | None, complevel: str | None, config_profile: str | None, extra_args: tuple[str, ...]):
    """Play a WAD by ID or query (e.g., 'caco play 1' or 'caco play filename:tnto').

    \b
    Use --iwad to play an IWAD directly: caco play --iwad doom2
    With no arguments, plays the most recently played WAD.
    """
    # Handle --iwad: play an IWAD directly
    if iwad_name:
        port = _check_sourceport(sourceport)
        iwad_extra = list(extra_args)
        if complevel:
            from caco.complevel import parse_complevel
            cl = parse_complevel(complevel)
            if cl is None:
                err_console.print(f"[red]Invalid complevel: '{complevel}' (use integer or alias: vanilla, boom, mbf, mbf21)[/red]")
                sys.exit(1)
            iwad_extra.extend(["-complevel", str(cl)])
        console.print(f"[cyan]Playing IWAD {iwad_name}...[/cyan]")
        try:
            result = play_iwad(iwad_name, sourceport=port, extra_args=iwad_extra, config_profile=config_profile)
            if result.duration:
                console.print(f"[green]Session ended:[/green] {format_duration(result.duration)}")
            if result.crashed:
                console.print(f"[yellow]Warning: sourceport exited with code {result.exit_code}[/yellow]")
        except Exception as e:
            err_console.print(f"[red]Error: {e}[/red]")
            sys.exit(1)
        return

    wad: dict[str, Any] | None
    if query:
        wads = resolve_wad_query(query, mode="pick", yes=first)
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

    port = _check_sourceport(sourceport)

    # Parse --complevel and inject into extra_args
    extra = list(extra_args)
    if complevel:
        from caco.complevel import parse_complevel
        cl = parse_complevel(complevel)
        if cl is None:
            err_console.print(f"[red]Invalid complevel: '{complevel}' (use integer or alias: vanilla, boom, mbf, mbf21)[/red]")
            sys.exit(1)
        extra.extend(["-complevel", str(cl)])

    console.print(f"[cyan]Playing {wad['title']}...[/cyan]")

    # Create a Rich progress callback for download display
    _progress: list[Progress | None] = [None]
    _task_id: list[Any] = [None]

    def _progress_callback(downloaded: int, total: int | None, filename: str) -> None:
        prog = _progress[0]
        if prog is None:
            prog = Progress(
                "[progress.description]{task.description}",
                BarColumn(),
                DownloadColumn(),
                TransferSpeedColumn(),
                console=console,
            )
            _progress[0] = prog
            prog.start()
            _task_id[0] = prog.add_task(f"Downloading {filename}", total=total)
        prog.update(_task_id[0], completed=downloaded, total=total)

    try:
        result = play(
            wad_id, sourceport=port, extra_args=extra,
            progress_callback=_progress_callback,
            config_profile=config_profile,
        )
        if result.duration:
            console.print(f"[green]Session ended:[/green] {format_duration(result.duration)}")
        if result.crashed:
            console.print(f"[yellow]Warning: sourceport exited with code {result.exit_code}[/yellow]")
    except Exception as e:
        err_console.print(f"[red]Error: {e}[/red]")
        sys.exit(1)
    finally:
        if _progress[0] is not None:
            _progress[0].stop()

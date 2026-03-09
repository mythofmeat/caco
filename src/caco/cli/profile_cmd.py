"""Sourceport config profile management commands."""

import os
import shutil
import subprocess
import sys

import click

from caco import db
from caco.config import (
    get_default_sourceport,
    get_profile_path,
    get_sourceport_dir,
    list_profiles,
)

from caco.cli import cli, console, err_console


def _resolve_sourceport(sourceport: str | None) -> str:
    """Resolve sourceport name from option or config. Exit on failure."""
    if sourceport:
        return sourceport
    port = get_default_sourceport()
    if port:
        return port
    err_console.print("[red]No sourceport specified and no default configured[/red]")
    err_console.print("[dim]Use -p/--sourceport or set one with: caco config set sourceport <name>[/dim]")
    sys.exit(1)


@cli.group()
def profile():
    """Manage sourceport config profiles."""
    pass


@profile.command(name="ls")
@click.option("--sourceport", "-p", type=str, help="Sourceport to list profiles for")
def profile_ls(sourceport: str | None):
    """List sourceport config profiles.

    \b
    Without -p, lists all sourceports and their profiles.
    With -p, lists profiles for that sourceport only.
    """
    from rich.table import Table

    profiles = list_profiles(sourceport)

    if not profiles:
        if sourceport:
            console.print(f"[dim]No profiles for {sourceport}[/dim]")
        else:
            console.print("[dim]No profiles found[/dim]")
            console.print("[dim]Profiles are auto-created on first play, or use 'caco profile create <name>'[/dim]")
        return

    table = Table(title="Config Profiles")
    table.add_column("Sourceport", style="cyan")
    table.add_column("Profile")
    table.add_column("Path", style="dim")

    for port_name, profile_names in profiles.items():
        for pname in profile_names:
            path = get_profile_path(port_name, pname)
            table.add_row(port_name, pname, str(path))

    console.print(table)


@profile.command()
@click.argument("name")
@click.option("--sourceport", "-p", type=str, help="Sourceport (defaults to configured)")
@click.option("--from", "from_profile", type=str, help="Copy from existing profile")
def create(name: str, sourceport: str | None, from_profile: str | None):
    """Create a new config profile.

    Creates an empty .cfg file (sourceport populates defaults on first launch).
    Use --from to copy an existing profile.
    """
    port = _resolve_sourceport(sourceport)
    path = get_profile_path(port, name)

    if path.exists():
        err_console.print(f"[red]Profile '{name}' already exists for {port}[/red]")
        sys.exit(1)

    path.parent.mkdir(parents=True, exist_ok=True)

    if from_profile:
        source_path = get_profile_path(port, from_profile)
        if not source_path.exists():
            err_console.print(f"[red]Source profile '{from_profile}' not found for {port}[/red]")
            sys.exit(1)
        shutil.copy2(str(source_path), str(path))
        console.print(f"[green]Created profile '{name}' (copied from '{from_profile}')[/green]")
    else:
        path.touch()
        console.print(f"[green]Created profile '{name}' for {port}[/green]")

    console.print(f"[dim]{path}[/dim]")


@profile.command()
@click.argument("name")
@click.option("--sourceport", "-p", type=str, help="Sourceport (defaults to configured)")
def edit(name: str, sourceport: str | None):
    """Open a config profile in your editor.

    Uses $VISUAL or $EDITOR (falls back to vi).
    """
    port = _resolve_sourceport(sourceport)
    path = get_profile_path(port, name)

    if not path.exists():
        err_console.print(f"[red]Profile '{name}' not found for {port}[/red]")
        err_console.print(f"[dim]Create it with: caco profile create {name}[/dim]")
        sys.exit(1)

    editor = os.environ.get("VISUAL") or os.environ.get("EDITOR") or "vi"
    try:
        subprocess.run([editor, str(path)])
    except FileNotFoundError:
        err_console.print(f"[red]Editor '{editor}' not found[/red]")
        sys.exit(1)


@profile.command()
@click.argument("name")
@click.argument("new_name")
@click.option("--sourceport", "-p", type=str, help="Sourceport (defaults to configured)")
def cp(name: str, new_name: str, sourceport: str | None):
    """Copy a config profile."""
    port = _resolve_sourceport(sourceport)
    src = get_profile_path(port, name)
    dst = get_profile_path(port, new_name)

    if not src.exists():
        err_console.print(f"[red]Profile '{name}' not found for {port}[/red]")
        sys.exit(1)

    if dst.exists():
        err_console.print(f"[red]Profile '{new_name}' already exists for {port}[/red]")
        sys.exit(1)

    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(src), str(dst))
    console.print(f"[green]Copied '{name}' → '{new_name}'[/green]")


@profile.command()
@click.argument("name")
@click.option("--sourceport", "-p", type=str, help="Sourceport (defaults to configured)")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation")
def rm(name: str, sourceport: str | None, yes: bool):
    """Delete a config profile."""
    port = _resolve_sourceport(sourceport)
    path = get_profile_path(port, name)

    if not path.exists():
        err_console.print(f"[red]Profile '{name}' not found for {port}[/red]")
        sys.exit(1)

    # Check if any WADs reference this profile
    referencing = db.search_wads(query=f"config:{name}")
    # Filter to exact matches only (search_wads uses LIKE)
    referencing = [w for w in referencing if w.get("custom_config") == name]
    if referencing:
        console.print(f"[yellow]Warning: {len(referencing)} WAD(s) reference profile '{name}':[/yellow]")
        for wad in referencing[:5]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(referencing) > 5:
            console.print(f"  [dim]... and {len(referencing) - 5} more[/dim]")

    if not yes:
        if not click.confirm(f"Delete profile '{name}'?"):
            console.print("[dim]Cancelled[/dim]")
            return

    path.unlink()
    console.print(f"[green]Deleted profile '{name}'[/green]")


@profile.command()
@click.argument("name")
@click.option("--sourceport", "-p", type=str, help="Sourceport (defaults to configured)")
def path(name: str, sourceport: str | None):
    """Print the full path to a config profile (for scripting)."""
    port = _resolve_sourceport(sourceport)
    print(get_profile_path(port, name))

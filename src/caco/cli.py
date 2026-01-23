"""Command-line interface for caco."""

import sys

import click
from rich.console import Console
from rich.table import Table

from caco import db
from caco.config import (
    get_default_sourceport,
    set_default_sourceport,
    get_cache_dir,
    set_cache_dir,
    set_iwad,
    load_config,
    save_config,
    CONFIG_FILE,
)
from caco.player import play, format_duration

console = Console()
err_console = Console(stderr=True)


@click.group()
def cli():
    """Caco - Personal Doom WAD library manager."""
    db.init_db()


# =============================================================================
# Library Management
# =============================================================================


@cli.command()
@click.argument("query", required=False)
@click.option("--status", "-s", type=click.Choice([s.value for s in db.Status]))
@click.option("--tag", "-t", help="Filter by tag")
@click.option("--source", type=click.Choice([s.value for s in db.SourceType]))
def list(query: str | None, status: str | None, tag: str | None, source: str | None):
    """List WADs in your library."""
    status_enum = db.Status(status) if status else None
    source_enum = db.SourceType(source) if source else None

    wads = db.search_wads(
        query=query,
        status=status_enum,
        source_type=source_enum,
        tag=tag,
    )

    if not wads:
        console.print("[dim]No WADs found[/dim]")
        return

    table = Table(title=f"Library ({len(wads)} WADs)")
    table.add_column("ID", style="dim")
    table.add_column("Title", style="cyan")
    table.add_column("Author")
    table.add_column("Status")
    table.add_column("Playtime", justify="right")
    table.add_column("Tags", style="dim")

    for wad in wads:
        playtime = db.get_total_playtime(wad["id"])
        playtime_str = format_duration(playtime) if playtime else "-"
        tags_str = ", ".join(wad.get("tags", [])) or "-"

        table.add_row(
            str(wad["id"]),
            wad["title"],
            wad["author"] or "-",
            wad["status"],
            playtime_str,
            tags_str,
        )

    console.print(table)


@cli.command()
@click.argument("wad_id", type=int)
def info(wad_id: int):
    """Show details about a WAD."""
    wad = db.get_wad(wad_id)
    if not wad:
        err_console.print(f"[red]WAD {wad_id} not found[/red]")
        sys.exit(1)

    console.print(f"[bold cyan]{wad['title']}[/bold cyan]")
    console.print(f"[dim]ID: {wad['id']}[/dim]")
    console.print()

    if wad["author"]:
        console.print(f"[bold]Author:[/bold] {wad['author']}")
    if wad["year"]:
        console.print(f"[bold]Year:[/bold] {wad['year']}")
    console.print(f"[bold]Status:[/bold] {wad['status']}")

    if wad["rating"]:
        console.print(f"[bold]Rating:[/bold] {'★' * wad['rating']}{'☆' * (5 - wad['rating'])}")

    if wad.get("tags"):
        console.print(f"[bold]Tags:[/bold] {', '.join(wad['tags'])}")

    console.print()
    console.print(f"[bold]Source:[/bold] {wad['source_type']}")
    if wad["source_url"]:
        console.print(f"[bold]URL:[/bold] {wad['source_url']}")

    if wad["description"]:
        console.print()
        console.print("[bold]Description:[/bold]")
        console.print(wad["description"])

    # Playtime stats
    playtime = db.get_total_playtime(wad_id)
    sessions = db.get_sessions(wad_id)
    if sessions:
        console.print()
        console.print(f"[bold]Playtime:[/bold] {format_duration(playtime)} ({len(sessions)} sessions)")

    if wad["notes"]:
        console.print()
        console.print("[bold]Notes:[/bold]")
        console.print(wad["notes"])


@cli.command()
@click.argument("wad_id", type=int)
@click.option("--status", "-s", type=click.Choice([s.value for s in db.Status]))
@click.option("--rating", "-r", type=click.IntRange(1, 5))
@click.option("--notes", "-n")
def update(wad_id: int, status: str | None, rating: int | None, notes: str | None):
    """Update a WAD's metadata."""
    updates = {}
    if status:
        updates["status"] = db.Status(status)
    if rating:
        updates["rating"] = rating
    if notes:
        updates["notes"] = notes

    if not updates:
        err_console.print("[yellow]No updates specified[/yellow]")
        return

    if db.update_wad(wad_id, **updates):
        console.print("[green]Updated[/green]")
    else:
        err_console.print(f"[red]WAD {wad_id} not found[/red]")
        sys.exit(1)


@cli.command()
@click.argument("wad_id", type=int)
@click.confirmation_option(prompt="Are you sure you want to delete this WAD?")
def delete(wad_id: int):
    """Delete a WAD from the library."""
    if db.delete_wad(wad_id):
        console.print("[green]Deleted[/green]")
    else:
        err_console.print(f"[red]WAD {wad_id} not found[/red]")
        sys.exit(1)


# =============================================================================
# Tags
# =============================================================================


@cli.group()
def tag():
    """Manage tags."""
    pass


@tag.command(name="add")
@click.argument("wad_id", type=int)
@click.argument("tags", nargs=-1, required=True)
def tag_add(wad_id: int, tags: tuple[str, ...]):
    """Add tags to a WAD."""
    for t in tags:
        if db.add_tag(wad_id, t):
            console.print(f"[green]Added tag:[/green] {t}")
        else:
            console.print(f"[yellow]Already tagged:[/yellow] {t}")


@tag.command(name="remove")
@click.argument("wad_id", type=int)
@click.argument("tags", nargs=-1, required=True)
def tag_remove(wad_id: int, tags: tuple[str, ...]):
    """Remove tags from a WAD."""
    for t in tags:
        if db.remove_tag(wad_id, t):
            console.print(f"[green]Removed tag:[/green] {t}")
        else:
            console.print(f"[yellow]Tag not found:[/yellow] {t}")


@tag.command(name="list")
def tag_list():
    """List all tags."""
    tags = db.get_all_tags()
    if not tags:
        console.print("[dim]No tags[/dim]")
        return

    for t in tags:
        console.print(t)


# =============================================================================
# Import
# =============================================================================


@cli.group(name="import")
def import_cmd():
    """Import WADs from various sources."""
    pass


@import_cmd.command(name="idgames")
@click.argument("query_or_id")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add")
def import_idgames(query_or_id: str, tags: tuple[str, ...]):
    """Import a WAD from idgames archive."""
    from caco.sources.idgames import IdgamesSource

    with IdgamesSource() as source:
        # Try as ID first
        try:
            file_id = int(query_or_id)
            entry = source.get(file_id)
            wad_id = source.import_wad(entry, tags=list(tags) if tags else None)
            console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
            return
        except ValueError:
            pass

        # Search
        results = source.search(query_or_id)
        if not results:
            console.print("[dim]No results found[/dim]")
            return

        # Show results and let user pick
        table = Table(title="Search Results")
        table.add_column("#", style="dim")
        table.add_column("ID", style="dim")
        table.add_column("Title", style="cyan")
        table.add_column("Author")
        table.add_column("Date")

        for i, entry in enumerate(results[:20], 1):
            table.add_row(str(i), str(entry.id), entry.title, entry.author, entry.date or "-")

        console.print(table)

        choice = click.prompt("Enter number to import (or 0 to cancel)", type=int, default=0)
        if choice == 0 or choice > len(results):
            return

        entry = results[choice - 1]
        wad_id = source.import_wad(entry, tags=list(tags) if tags else None)
        console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")


@import_cmd.command(name="url")
@click.argument("title")
@click.argument("url")
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "-t", "tags", multiple=True)
@click.option("--description", "-d")
def import_url(title: str, url: str, author: str | None, year: int | None,
               tags: tuple[str, ...], description: str | None):
    """Import a WAD from a URL (e.g., Doomworld forums)."""
    wad_id = db.add_wad(
        title=title,
        source_type=db.SourceType.URL,
        source_url=url,
        author=author,
        year=year,
        description=description,
        tags=list(tags) if tags else None,
    )
    console.print(f"[green]Added:[/green] {title} (ID: {wad_id})")


@import_cmd.command(name="local")
@click.argument("title")
@click.argument("path", type=click.Path(exists=True))
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "-t", "tags", multiple=True)
def import_local(title: str, path: str, author: str | None, year: int | None,
                 tags: tuple[str, ...]):
    """Import a local WAD file."""
    from pathlib import Path as P
    p = P(path).resolve()

    wad_id = db.add_wad(
        title=title,
        source_type=db.SourceType.LOCAL,
        source_url=str(p),
        filename=p.name,
        cached_path=str(p),
        author=author,
        year=year,
        tags=list(tags) if tags else None,
    )
    console.print(f"[green]Added:[/green] {title} (ID: {wad_id})")


# =============================================================================
# Play
# =============================================================================


@cli.command()
@click.argument("wad_id", type=int)
@click.option("--sourceport", "-p", help="Sourceport to use")
@click.argument("extra_args", nargs=-1)
def play_cmd(wad_id: int, sourceport: str | None, extra_args: tuple[str, ...]):
    """Play a WAD."""
    wad = db.get_wad(wad_id)
    if not wad:
        err_console.print(f"[red]WAD {wad_id} not found[/red]")
        sys.exit(1)

    port = sourceport or get_default_sourceport()
    if not port:
        err_console.print("[red]No sourceport specified. Use --sourceport or set default with 'caco config sourceport <path>'[/red]")
        sys.exit(1)

    console.print(f"[cyan]Playing {wad['title']}...[/cyan]")

    try:
        duration = play(wad_id, sourceport=port, extra_args=list(extra_args))
        if duration:
            console.print(f"[green]Session ended:[/green] {format_duration(duration)}")
    except Exception as e:
        err_console.print(f"[red]Error: {e}[/red]")
        sys.exit(1)


# Alias 'play' to 'play_cmd'
cli.add_command(play_cmd, name="play")


# =============================================================================
# Config
# =============================================================================


@cli.command()
@click.argument("key", required=False)
@click.argument("value", required=False)
def config(key: str | None, value: str | None):
    """View or set configuration."""
    if key is None:
        # Show all config
        cfg = load_config()
        console.print(f"[dim]Config file: {CONFIG_FILE}[/dim]")
        console.print()
        for k, v in cfg.items():
            if v == "" or v is None:
                display = "[dim]not set[/dim]"
            elif isinstance(v, list):
                display = ", ".join(v) if v else "[dim]not set[/dim]"
            else:
                display = str(v)
            console.print(f"[bold]{k}:[/bold] {display}")
        return

    if value is None:
        # Show single value
        cfg = load_config()
        console.print(cfg.get(key, "[dim]not set[/dim]"))
        return

    # Set value
    if key == "sourceport":
        set_default_sourceport(value)
    elif key == "cache_dir":
        set_cache_dir(value)
    elif key == "iwad":
        set_iwad(value)
    elif key == "download_mirror":
        cfg = load_config()
        cfg["download_mirror"] = int(value)
        save_config(cfg)
    else:
        err_console.print(f"[red]Unknown config key: {key}[/red]")
        err_console.print("[dim]Valid keys: sourceport, iwad, cache_dir, download_mirror[/dim]")
        sys.exit(1)

    console.print(f"[green]Set {key} = {value}[/green]")


if __name__ == "__main__":
    cli()

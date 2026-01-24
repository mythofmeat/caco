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


class WadIdRange(click.ParamType):
    """Parse WAD ID ranges like '3-6,9,11' into a list of ints."""

    name = "wad_ids"

    def convert(self, value, param, ctx) -> list[int]:
        if isinstance(value, list):
            return value
        try:
            ids = []
            for part in value.split(","):
                part = part.strip()
                if "-" in part:
                    start, end = map(int, part.split("-", 1))
                    if start > end:
                        self.fail(f"Invalid range: {start}-{end}", param, ctx)
                    ids.extend(range(start, end + 1))
                else:
                    ids.append(int(part))
            return list(dict.fromkeys(ids))  # dedupe, preserve order
        except ValueError:
            self.fail(f"Invalid format: {value}. Use '3-6,9,11'", param, ctx)


WAD_IDS = WadIdRange()


@click.group()
def cli():
    """Caco - Personal Doom WAD library manager."""
    db.init_db()


# =============================================================================
# Library Management
# =============================================================================


@cli.command(name="list")
@click.argument("query", required=False)
@click.option("--status", "-s", type=click.Choice([s.value for s in db.Status]))
@click.option("--tag", "-t", help="Filter by tag")
@click.option("--source", type=click.Choice([s.value for s in db.SourceType]))
def list_cmd(query: str | None, status: str | None, tag: str | None, source: str | None):
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
    table.add_column("Last Played", style="dim")

    for wad in wads:
        playtime = db.get_total_playtime(wad["id"])
        playtime_str = format_duration(playtime) if playtime else "-"
        last_played = db.get_last_played(wad["id"])
        last_played_str = last_played[:10] if last_played else "-"

        table.add_row(
            str(wad["id"]),
            wad["title"],
            wad["author"] or "-",
            wad["status"],
            playtime_str,
            last_played_str,
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
    last_played = db.get_last_played(wad_id)
    if sessions:
        console.print()
        console.print(f"[bold]Playtime:[/bold] {format_duration(playtime)} ({len(sessions)} sessions)")
        if last_played:
            console.print(f"[bold]Last played:[/bold] {last_played[:16].replace('T', ' ')}")

    if wad["notes"]:
        console.print()
        console.print("[bold]Notes:[/bold]")
        console.print(wad["notes"])


@cli.command()
@click.argument("wad_ids", type=WAD_IDS)
@click.option("--status", "-s", type=click.Choice([s.value for s in db.Status]))
@click.option("--rating", "-r", type=click.IntRange(1, 5))
@click.option("--notes", "-n")
def update(wad_ids: list[int], status: str | None, rating: int | None, notes: str | None):
    """Update WAD metadata. WAD_IDS: single ID or range (3-6,9,11)."""
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

    success, failed = 0, []
    for wad_id in wad_ids:
        if db.update_wad(wad_id, **updates):
            success += 1
        else:
            failed.append(wad_id)

    if success:
        console.print(f"[green]Updated {success} WAD(s)[/green]")
    if failed:
        err_console.print(f"[red]WAD(s) not found: {', '.join(map(str, failed))}[/red]")
        sys.exit(1)


@cli.command()
@click.argument("wad_ids", type=WAD_IDS)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def delete(wad_ids: list[int], yes: bool):
    """Delete WAD(s) from the library. WAD_IDS: single ID or range (3-6,9,11)."""
    count = len(wad_ids)
    if not yes:
        if not click.confirm(f"Delete {count} WAD(s)?"):
            return

    success, failed = 0, []
    for wad_id in wad_ids:
        if db.delete_wad(wad_id):
            success += 1
        else:
            failed.append(wad_id)

    if success:
        console.print(f"[green]Deleted {success} WAD(s)[/green]")
    if failed:
        err_console.print(f"[red]WAD(s) not found: {', '.join(map(str, failed))}[/red]")
        sys.exit(1)


# =============================================================================
# Tags
# =============================================================================


@cli.group()
def tag():
    """Manage tags."""
    pass


@tag.command(name="add")
@click.argument("wad_ids", type=WAD_IDS)
@click.argument("tags", nargs=-1, required=True)
def tag_add(wad_ids: list[int], tags: tuple[str, ...]):
    """Add tags to WAD(s). WAD_IDS: single ID or range (3-6,9,11)."""
    success, failed = 0, []
    for wad_id in wad_ids:
        wad = db.get_wad(wad_id)
        if not wad:
            failed.append(wad_id)
            continue
        for t in tags:
            db.add_tag(wad_id, t)
        success += 1

    if success:
        console.print(f"[green]Added tag(s) to {success} WAD(s)[/green]")
    if failed:
        err_console.print(f"[red]WAD(s) not found: {', '.join(map(str, failed))}[/red]")
        sys.exit(1)


@tag.command(name="remove")
@click.argument("wad_ids", type=WAD_IDS)
@click.argument("tags", nargs=-1, required=True)
def tag_remove(wad_ids: list[int], tags: tuple[str, ...]):
    """Remove tags from WAD(s). WAD_IDS: single ID or range (3-6,9,11)."""
    success, failed = 0, []
    for wad_id in wad_ids:
        wad = db.get_wad(wad_id)
        if not wad:
            failed.append(wad_id)
            continue
        for t in tags:
            db.remove_tag(wad_id, t)
        success += 1

    if success:
        console.print(f"[green]Removed tag(s) from {success} WAD(s)[/green]")
    if failed:
        err_console.print(f"[red]WAD(s) not found: {', '.join(map(str, failed))}[/red]")
        sys.exit(1)


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

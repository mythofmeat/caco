"""Command-line interface for caco."""

import shutil
import subprocess
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
    set_stats_dir,
    load_config,
    save_config,
    CONFIG_FILE,
)
from caco.player import play, format_duration

console = Console()
err_console = Console(stderr=True)


# =============================================================================
# fzf Integration
# =============================================================================


def _fzf_available() -> bool:
    """Check if fzf is installed."""
    return shutil.which("fzf") is not None


def _fzf_select(
    items: list[str],
    prompt: str = "Select",
    multi: bool = False,
    preview: str | None = None,
) -> list[int] | None:
    """
    Use fzf to select from a list of items.

    Args:
        items: List of strings to select from
        prompt: Prompt to display
        multi: Allow multiple selections
        preview: Optional preview command template ({} is replaced with selection)

    Returns:
        List of selected indices (0-based), or None if cancelled.
    """
    if not _fzf_available():
        return None

    # Build fzf command
    cmd = ["fzf", "--prompt", f"{prompt}> "]
    if multi:
        cmd.append("--multi")
    if preview:
        cmd.extend(["--preview", preview])

    # Add line numbers for tracking
    numbered_items = [f"{i+1}. {item}" for i, item in enumerate(items)]

    try:
        result = subprocess.run(
            cmd,
            input="\n".join(numbered_items),
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            return None  # User cancelled or error

        # Parse selected indices
        selected = []
        for line in result.stdout.strip().split("\n"):
            if line:
                # Extract the number from "N. item"
                try:
                    idx = int(line.split(".", 1)[0]) - 1
                    if 0 <= idx < len(items):
                        selected.append(idx)
                except (ValueError, IndexError):
                    pass

        return selected if selected else None

    except (subprocess.SubprocessError, OSError):
        return None


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


def _parse_id_range(value: str) -> list[int] | None:
    """Try to parse a value as ID range (3-6,9,11). Returns None if not valid."""
    try:
        ids = []
        for part in value.split(","):
            part = part.strip()
            if "-" in part:
                # Must be start-end format with valid ints
                pieces = part.split("-", 1)
                if len(pieces) != 2:
                    return None
                start, end = int(pieces[0]), int(pieces[1])
                if start > end:
                    return None
                ids.extend(range(start, end + 1))
            else:
                ids.append(int(part))
        return list(dict.fromkeys(ids))  # dedupe, preserve order
    except ValueError:
        return None


def resolve_wad_query(
    query: str, allow_multiple: bool = False, yes: bool = False
) -> list[dict] | None:
    """Resolve WAD ID, ID range, or query string to WAD(s).

    Args:
        query: WAD ID, ID range (3-6,9), or query string (filename:tnto)
        allow_multiple: If True, allow multiple matches (with confirmation)
        yes: If True, skip confirmation prompts

    Returns:
        List of WAD dicts, or None if cancelled/no matches.
        Exits with error if single match required but multiple found.
    """
    # Try parsing as ID range first (backward compat)
    ids = _parse_id_range(query)
    if ids is not None:
        wads = []
        missing = []
        for wad_id in ids:
            wad = db.get_wad(wad_id)
            if wad:
                wads.append(wad)
            else:
                missing.append(wad_id)
        if missing:
            err_console.print(f"[red]WAD(s) not found: {', '.join(map(str, missing))}[/red]")
            sys.exit(1)
        return wads

    # Query-based lookup
    results = db.search_wads(query=query)
    if not results:
        err_console.print(f"[red]No WADs matching '{query}'[/red]")
        sys.exit(1)

    if len(results) == 1:
        return results

    # Multiple matches
    if not allow_multiple:
        err_console.print(f"[red]Multiple WADs match '{query}':[/red]")
        for r in results[:10]:
            err_console.print(f"  {r['id']}: {r['title']}")
        if len(results) > 10:
            err_console.print(f"  ... and {len(results) - 10} more")
        sys.exit(1)

    # allow_multiple=True: confirm unless yes
    if not yes:
        console.print(f"[yellow]This will affect {len(results)} WAD(s):[/yellow]")
        for r in results[:10]:
            console.print(f"  {r['id']}: {r['title']}")
        if len(results) > 10:
            console.print(f"  ... and {len(results) - 10} more")
        if not click.confirm("Continue?"):
            return None

    return results


SORT_FIELDS = ["playtime", "rating", "created", "title", "author", "last_played", "year"]


def _parse_sort_option(sort: str | None) -> tuple[str | None, bool]:
    """Parse sort option. Returns (field, descending).

    Examples:
        'playtime' -> ('playtime', True)  # Default desc
        '-title' -> ('title', False)  # Explicit reverse (for title: Z-A)
    """
    if not sort:
        return None, True

    if sort.startswith("-"):
        return sort[1:], False
    return sort, True


def _render_wad_list_plain(wads: list[dict]) -> None:
    """TSV output: ID\tTitle\tAuthor\tStatus\tMaps\tBeaten\tPlaytime\tLastPlayed."""
    # Batch fetch stats for all WADs
    wad_ids = [w["id"] for w in wads]
    maps_completed = db.get_maps_completed_batch(wad_ids)
    times_beaten = db.get_times_beaten_batch(wad_ids)

    # Header
    print("ID\tTitle\tAuthor\tStatus\tMaps\tBeaten\tPlaytime\tLastPlayed")
    for wad in wads:
        playtime = db.get_total_playtime(wad["id"])
        playtime_str = format_duration(playtime) if playtime else ""
        last_played = db.get_last_played(wad["id"])
        last_played_str = last_played[:10] if last_played else ""
        maps_str = str(maps_completed.get(wad["id"], 0))
        beaten_str = str(times_beaten.get(wad["id"], 0))
        print(f"{wad['id']}\t{wad['title']}\t{wad['author'] or ''}\t{wad['status']}\t{maps_str}\t{beaten_str}\t{playtime_str}\t{last_played_str}")


def _render_wad_info_plain(wad: dict) -> None:
    """Key=value output for scripting."""
    playtime = db.get_total_playtime(wad["id"])
    sessions = db.get_sessions(wad["id"])
    last_played = db.get_last_played(wad["id"])
    map_stats = db.get_map_completion_stats(wad["id"])
    times_beaten = db.get_times_beaten(wad["id"])

    print(f"id={wad['id']}")
    print(f"title={wad['title']}")
    print(f"author={wad['author'] or ''}")
    print(f"year={wad['year'] or ''}")
    print(f"status={wad['status']}")
    print(f"rating={wad['rating'] or ''}")
    print(f"tags={','.join(wad.get('tags') or [])}")
    print(f"source_type={wad['source_type']}")
    print(f"source_url={wad['source_url'] or ''}")
    print(f"filename={wad.get('filename') or ''}")
    print(f"playtime={format_duration(playtime) if playtime else ''}")
    print(f"sessions={len(sessions)}")
    print(f"last_played={last_played[:10] if last_played else ''}")
    print(f"maps_completed={map_stats['unique_maps']}")
    print(f"times_beaten={times_beaten}")
    if wad.get("custom_iwad"):
        print(f"custom_iwad={wad['custom_iwad']}")
    if wad.get("custom_sourceport"):
        print(f"custom_sourceport={wad['custom_sourceport']}")
    if wad.get("custom_args"):
        print(f"custom_args={wad['custom_args']}")


@click.group()
def cli():
    """Caco - Personal Doom WAD library manager."""
    db.init_db()


# =============================================================================
# Library Management
# =============================================================================


def _render_wad_list(wads: list[dict], title: str | None = None) -> None:
    """Render a list of WADs as a table."""
    if not wads:
        console.print("[dim]No WADs found[/dim]")
        return

    # Batch fetch stats for all WADs
    wad_ids = [w["id"] for w in wads]
    maps_completed = db.get_maps_completed_batch(wad_ids)
    times_beaten = db.get_times_beaten_batch(wad_ids)

    table = Table(title=title or f"Library ({len(wads)} WADs)")
    table.add_column("ID", style="dim")
    table.add_column("Title", style="cyan")
    table.add_column("Author")
    table.add_column("Status")
    table.add_column("Maps", justify="right")
    table.add_column("Beaten", justify="right")
    table.add_column("Playtime", justify="right")
    table.add_column("Last Played", style="dim")

    for wad in wads:
        playtime = db.get_total_playtime(wad["id"])
        playtime_str = format_duration(playtime) if playtime else "-"
        last_played = db.get_last_played(wad["id"])
        last_played_str = last_played[:10] if last_played else "-"
        maps_str = str(maps_completed.get(wad["id"], 0)) if maps_completed.get(wad["id"]) else "-"
        beaten_str = str(times_beaten.get(wad["id"], 0)) if times_beaten.get(wad["id"]) else "-"

        table.add_row(
            str(wad["id"]),
            wad["title"],
            wad["author"] or "-",
            wad["status"],
            maps_str,
            beaten_str,
            playtime_str,
            last_played_str,
        )

    console.print(table)


@cli.command(name="list")
@click.argument("query", required=False)
@click.option("--status", "-s", type=click.Choice([s.value for s in db.Status]))
@click.option("--tag", "-t", help="Filter by tag")
@click.option("--source", type=click.Choice([s.value for s in db.SourceType]))
@click.option("--sort", "-S", help="Sort by: playtime, rating, created, title, author, last_played, year (prefix - to reverse)")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
def list_cmd(query: str | None, status: str | None, tag: str | None, source: str | None, sort: str | None, plain: bool):
    """List WADs in your library."""
    status_enum = db.Status(status) if status else None
    source_enum = db.SourceType(source) if source else None

    sort_field, sort_desc = _parse_sort_option(sort)
    if sort_field and sort_field not in SORT_FIELDS:
        err_console.print(f"[red]Invalid sort field: {sort_field}[/red]")
        err_console.print(f"[dim]Valid fields: {', '.join(SORT_FIELDS)}[/dim]")
        sys.exit(1)

    wads = db.search_wads(
        query=query,
        status=status_enum,
        source_type=source_enum,
        tag=tag,
        sort_by=sort_field,
        sort_desc=sort_desc,
    )
    if plain:
        _render_wad_list_plain(wads)
    else:
        _render_wad_list(wads)


@cli.command()
@click.argument("query")
@click.option("--plain", is_flag=True, help="Output as key=value pairs (for scripting)")
def info(query: str, plain: bool):
    """Show details about a WAD. QUERY: WAD ID or query (e.g., filename:tnto)."""
    wads = resolve_wad_query(query, allow_multiple=False)
    wad = wads[0]
    wad_id = wad["id"]

    if plain:
        _render_wad_info_plain(wad)
        return

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

    # Map completion stats
    map_stats = db.get_map_completion_stats(wad_id)
    times_beaten = db.get_times_beaten(wad_id)
    if map_stats["unique_maps"] > 0 or times_beaten > 0:
        console.print()
        if map_stats["unique_maps"] > 0:
            console.print(f"[bold]Maps completed:[/bold] {map_stats['unique_maps']}")
            # Show sample of completed maps
            completed_maps = list(map_stats["by_map"].keys())[:5]
            if len(completed_maps) < len(map_stats["by_map"]):
                console.print(f"  [dim]{', '.join(completed_maps)}, ...[/dim]")
            else:
                console.print(f"  [dim]{', '.join(completed_maps)}[/dim]")
        if times_beaten > 0:
            console.print(f"[bold]Times beaten:[/bold] {times_beaten}")

    # Per-WAD play config
    if wad.get("custom_iwad") or wad.get("custom_sourceport") or wad.get("custom_args"):
        import json
        console.print()
        console.print("[bold]Custom play config:[/bold]")
        if wad.get("custom_iwad"):
            console.print(f"  IWAD: {wad['custom_iwad']}")
        if wad.get("custom_sourceport"):
            console.print(f"  Sourceport: {wad['custom_sourceport']}")
        if wad.get("custom_args"):
            try:
                args = json.loads(wad["custom_args"])
                console.print(f"  Args: {' '.join(args)}")
            except json.JSONDecodeError:
                console.print(f"  Args: {wad['custom_args']}")


@cli.command()
@click.argument("query")
@click.option("--status", "-s", type=click.Choice([s.value for s in db.Status]))
@click.option("--rating", "-r", type=click.IntRange(1, 5))
@click.option("--notes", "-n")
@click.option("--iwad", help="Custom IWAD path for this WAD")
@click.option("--clear-iwad", is_flag=True, help="Clear custom IWAD")
@click.option("--sourceport", help="Custom sourceport for this WAD")
@click.option("--clear-sourceport", is_flag=True, help="Clear custom sourceport")
@click.option("--args", "custom_args", help="Custom arguments (JSON array or space-separated)")
@click.option("--clear-args", is_flag=True, help="Clear custom arguments")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
def update(
    query: str,
    status: str | None,
    rating: int | None,
    notes: str | None,
    iwad: str | None,
    clear_iwad: bool,
    sourceport: str | None,
    clear_sourceport: bool,
    custom_args: str | None,
    clear_args: bool,
    yes: bool,
):
    """Update WAD metadata. QUERY: ID, ID range (3-6,9), or query (tag:megawad)."""
    import json

    updates = {}
    if status:
        updates["status"] = db.Status(status)
    if rating:
        updates["rating"] = rating
    if notes:
        updates["notes"] = notes

    # Per-WAD play config
    if iwad:
        updates["custom_iwad"] = iwad
    elif clear_iwad:
        updates["custom_iwad"] = None
    if sourceport:
        updates["custom_sourceport"] = sourceport
    elif clear_sourceport:
        updates["custom_sourceport"] = None
    if custom_args:
        # Accept JSON array or space-separated string
        try:
            args_list = json.loads(custom_args)
        except json.JSONDecodeError:
            args_list = custom_args.split()
        updates["custom_args"] = json.dumps(args_list)
    elif clear_args:
        updates["custom_args"] = None

    if not updates:
        err_console.print("[yellow]No updates specified[/yellow]")
        return

    wads = resolve_wad_query(query, allow_multiple=True, yes=yes)
    if not wads:
        return  # User cancelled

    for wad in wads:
        db.update_wad(wad["id"], **updates)

    console.print(f"[green]Updated {len(wads)} WAD(s)[/green]")


@cli.command()
@click.argument("query")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def delete(query: str, yes: bool):
    """Delete WAD(s) from the library. QUERY: ID, ID range (3-6,9), or query (status:abandoned)."""
    wads = resolve_wad_query(query, allow_multiple=True, yes=yes)
    if not wads:
        return  # User cancelled

    for wad in wads:
        db.delete_wad(wad["id"])

    console.print(f"[green]Deleted {len(wads)} WAD(s)[/green]")


# =============================================================================
# Tags
# =============================================================================


@cli.group()
def tag():
    """Manage tags."""
    pass


@tag.command(name="add")
@click.argument("query")
@click.argument("tags", nargs=-1, required=True)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
def tag_add(query: str, tags: tuple[str, ...], yes: bool):
    """Add tags to WAD(s). QUERY: ID, ID range (3-6,9), or query (author:romero)."""
    wads = resolve_wad_query(query, allow_multiple=True, yes=yes)
    if not wads:
        return  # User cancelled

    for wad in wads:
        for t in tags:
            db.add_tag(wad["id"], t)

    console.print(f"[green]Added tag(s) to {len(wads)} WAD(s)[/green]")


@tag.command(name="remove")
@click.argument("query")
@click.argument("tags", nargs=-1, required=True)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
def tag_remove(query: str, tags: tuple[str, ...], yes: bool):
    """Remove tags from WAD(s). QUERY: ID, ID range (3-6,9), or query (author:romero)."""
    wads = resolve_wad_query(query, allow_multiple=True, yes=yes)
    if not wads:
        return  # User cancelled

    for wad in wads:
        for t in tags:
            db.remove_tag(wad["id"], t)

    console.print(f"[green]Removed tag(s) from {len(wads)} WAD(s)[/green]")


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
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow selecting multiple WADs (requires fzf)")
def import_idgames(query_or_id: str, tags: tuple[str, ...], force: bool, multi: bool):
    """Import a WAD from idgames archive.

    Use fzf for interactive selection (if installed). Use --multi for batch import.
    """
    from caco.sources.idgames import IdgamesSource

    def _check_and_import(entry, tags_list):
        """Check for duplicates before importing. Returns wad_id or None if skipped."""
        # Check for duplicate
        existing = db.find_duplicate(
            db.SourceType.IDGAMES,
            source_id=str(entry.id),
            filename=entry.filename,
            author=entry.author,
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return None

        wad_id = source.import_wad(entry, tags=tags_list)
        return wad_id

    with IdgamesSource() as source:
        # Try as ID first
        try:
            file_id = int(query_or_id)
            entry = source.get(file_id)
            wad_id = _check_and_import(entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
            return
        except ValueError:
            pass

        # Search
        results = source.search(query_or_id)
        if not results:
            console.print("[dim]No results found[/dim]")
            return

        # Multi-select requires fzf
        if multi and not _fzf_available():
            err_console.print("[red]--multi requires fzf to be installed[/red]")
            err_console.print("[dim]Install fzf: https://github.com/junegunn/fzf[/dim]")
            sys.exit(1)

        # Try fzf for selection
        if _fzf_available():
            # Format items for fzf: "Title by Author (Year)"
            fzf_items = []
            for entry in results[:50]:  # Allow more results with fzf
                year = entry.date[:4] if entry.date else "????"
                fzf_items.append(f"{entry.title} by {entry.author or 'Unknown'} ({year})")

            selected_indices = _fzf_select(
                fzf_items,
                prompt="Select WAD(s)" if multi else "Select WAD",
                multi=multi,
            )

            if selected_indices is None:
                return  # User cancelled

            # Import selected WADs
            imported = 0
            for idx in selected_indices:
                entry = results[idx]
                wad_id = _check_and_import(entry, list(tags) if tags else None)
                if wad_id:
                    console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
                    imported += 1

            if multi and imported > 1:
                console.print(f"[green]Imported {imported} WAD(s)[/green]")

        else:
            # Fallback to numbered prompt
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
            wad_id = _check_and_import(entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")


@import_cmd.command(name="url")
@click.argument("title")
@click.argument("url")
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "-t", "tags", multiple=True)
@click.option("--description", "-d")
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
def import_url(title: str, url: str, author: str | None, year: int | None,
               tags: tuple[str, ...], description: str | None, force: bool):
    """Import a WAD from a URL (e.g., Doomworld forums)."""
    # Check for duplicate
    existing = db.find_duplicate(
        db.SourceType.URL,
        source_url=url,
        filename=title,
        author=author,
    )
    if existing and not force:
        console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
        console.print("[dim]Use --force to import anyway[/dim]")
        return

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
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
def import_local(title: str, path: str, author: str | None, year: int | None,
                 tags: tuple[str, ...], force: bool):
    """Import a local WAD file."""
    from pathlib import Path as P
    p = P(path).resolve()

    # Check for duplicate
    existing = db.find_duplicate(
        db.SourceType.LOCAL,
        source_url=str(p),
        filename=p.name,
        author=author,
    )
    if existing and not force:
        console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
        console.print("[dim]Use --force to import anyway[/dim]")
        return

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
@click.argument("query")
@click.option("--sourceport", "-p", help="Sourceport to use")
@click.argument("extra_args", nargs=-1)
def play_cmd(query: str, sourceport: str | None, extra_args: tuple[str, ...]):
    """Play a WAD by ID or query (e.g., 'caco play 1' or 'caco play filename:tnto')."""
    # Try parsing as int first (WAD ID)
    try:
        wad_id = int(query)
        wad = db.get_wad(wad_id)
        if not wad:
            err_console.print(f"[red]WAD {wad_id} not found[/red]")
            sys.exit(1)
    except ValueError:
        # Query-based lookup
        results = db.search_wads(query=query)
        if not results:
            err_console.print(f"[red]No WADs matching '{query}'[/red]")
            sys.exit(1)
        if len(results) > 1:
            err_console.print(f"[red]Multiple WADs match '{query}':[/red]")
            for r in results[:10]:
                err_console.print(f"  {r['id']}: {r['title']}")
            if len(results) > 10:
                err_console.print(f"  ... and {len(results) - 10} more")
            sys.exit(1)
        wad = results[0]
        wad_id = wad["id"]

    port = sourceport or get_default_sourceport()
    if not port:
        err_console.print("[red]No sourceport specified. Use --sourceport or set default with 'caco config sourceport <path>'[/red]")
        sys.exit(1)

    console.print(f"[cyan]Playing {wad['title']}...[/cyan]")

    try:
        duration = play(wad_id, sourceport=port, extra_args=list(extra_args), console=console)
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
    elif key == "stats_dir":
        set_stats_dir(value)
    elif key == "download_mirror":
        cfg = load_config()
        cfg["download_mirror"] = int(value)
        save_config(cfg)
    else:
        err_console.print(f"[red]Unknown config key: {key}[/red]")
        err_console.print("[dim]Valid keys: sourceport, iwad, cache_dir, stats_dir, download_mirror[/dim]")
        sys.exit(1)

    console.print(f"[green]Set {key} = {value}[/green]")


# =============================================================================
# Completions
# =============================================================================


@cli.command()
@click.argument("shell", required=False, type=click.Choice(["bash", "fish", "zsh"]))
@click.option("--install", is_flag=True, help="Install completions to shell config")
def completions(shell: str | None, install: bool):
    """Generate or install shell completions."""
    import os
    from pathlib import Path

    # Auto-detect shell if not specified
    if not shell:
        shell_path = os.environ.get("SHELL", "")
        if "fish" in shell_path:
            shell = "fish"
        elif "zsh" in shell_path:
            shell = "zsh"
        elif "bash" in shell_path:
            shell = "bash"
        else:
            err_console.print("[red]Could not detect shell. Specify: bash, fish, or zsh[/red]")
            sys.exit(1)

    # Get completion script
    from click.shell_completion import get_completion_class

    comp_cls = get_completion_class(shell)
    if not comp_cls:
        err_console.print(f"[red]Unsupported shell: {shell}[/red]")
        sys.exit(1)

    comp = comp_cls(cli, {}, "caco", "_CACO_COMPLETE")
    script = comp.source()

    if not install:
        # Just print the script
        console.print(script)
        return

    # Install to appropriate location
    home = Path.home()
    if shell == "fish":
        dest = home / ".config" / "fish" / "completions" / "caco.fish"
    elif shell == "zsh":
        dest = home / ".zfunc" / "_caco"
    elif shell == "bash":
        dest = home / ".local" / "share" / "bash-completion" / "completions" / "caco"
    else:
        err_console.print(f"[red]Unknown shell: {shell}[/red]")
        sys.exit(1)

    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(script)
    console.print(f"[green]Installed completions to {dest}[/green]")

    if shell == "zsh":
        console.print("[dim]Add 'fpath=(~/.zfunc $fpath)' to ~/.zshrc and run 'compinit'[/dim]")
    elif shell == "bash":
        console.print("[dim]Add 'source ~/.local/share/bash-completion/completions/caco' to ~/.bashrc[/dim]")


# =============================================================================
# Map Completions
# =============================================================================


SKILL_NAMES = {1: "ITYTD", 2: "HNTR", 3: "HMP", 4: "UV", 5: "NM"}


def _parse_map_range(maps_str: str) -> list[str]:
    """Parse map range like 'MAP01-MAP05' or 'E1M1-E1M9' into list of maps."""
    maps = []
    for part in maps_str.split(","):
        part = part.strip().upper()
        if "-" in part and not part.startswith("E"):
            # Handle MAP01-MAP05 range
            start, end = part.split("-", 1)
            if start.startswith("MAP") and end.startswith("MAP"):
                try:
                    start_num = int(start[3:])
                    end_num = int(end[3:])
                    for i in range(start_num, end_num + 1):
                        maps.append(f"MAP{i:02d}")
                except ValueError:
                    maps.append(part)
            elif start.startswith("E") and "M" in start:
                # Handle E1M1-E1M9 range
                try:
                    ep = start[1]
                    start_map = int(start.split("M")[1])
                    end_map = int(end.split("M")[1])
                    for i in range(start_map, end_map + 1):
                        maps.append(f"E{ep}M{i}")
                except (ValueError, IndexError):
                    maps.append(part)
            else:
                maps.append(part)
        else:
            maps.append(part)
    return maps


@cli.group(name="map")
def map_cmd():
    """Manage map completions."""
    pass


@map_cmd.command(name="sync")
@click.argument("query", required=False)
@click.option("--all", "sync_all", is_flag=True, help="Sync all WADs with stats files")
def map_sync(query: str | None, sync_all: bool):
    """Sync map completions from stats.txt files."""
    from caco.player import get_stats_path, parse_stats_file

    if sync_all:
        wads = db.search_wads()
    elif query:
        wads = resolve_wad_query(query, allow_multiple=True, yes=True)
        if not wads:
            return
    else:
        err_console.print("[red]Specify a WAD query or use --all[/red]")
        sys.exit(1)

    total_synced = 0
    wads_synced = 0

    for wad in wads:
        stats_path = get_stats_path(wad)
        if not stats_path or not stats_path.exists():
            continue

        completions = parse_stats_file(stats_path)
        if completions:
            added = db.sync_map_completions(wad["id"], completions)
            if added > 0:
                total_synced += added
                wads_synced += 1
                console.print(f"[dim]{wad['title']}: +{added} completion(s)[/dim]")

    if total_synced > 0:
        console.print(f"[green]Synced {total_synced} completion(s) across {wads_synced} WAD(s)[/green]")
    else:
        console.print("[dim]No new completions found[/dim]")


@map_cmd.command(name="complete")
@click.argument("query")
@click.argument("maps", nargs=-1, required=True)
@click.option("--skill", "-s", type=click.IntRange(1, 5), help="Skill level (1-5: ITYTD to NM)")
@click.option("--notes", "-n", help="Notes for this completion")
def map_complete(query: str, maps: tuple[str, ...], skill: int | None, notes: str | None):
    """Manually mark maps as completed."""
    wads = resolve_wad_query(query, allow_multiple=False)
    wad = wads[0]

    # Parse all map arguments (support ranges)
    all_maps = []
    for m in maps:
        all_maps.extend(_parse_map_range(m))

    for map_name in all_maps:
        db.add_map_completion(wad["id"], map_name, skill=skill, notes=notes)

    skill_str = f" (skill {SKILL_NAMES.get(skill, skill)})" if skill else ""
    console.print(f"[green]Marked {len(all_maps)} map(s) as completed{skill_str}[/green]")


@map_cmd.command(name="uncomplete")
@click.argument("query")
@click.argument("maps", nargs=-1, required=True)
@click.option("--skill", "-s", type=click.IntRange(1, 5), help="Only remove specific skill (default: all)")
def map_uncomplete(query: str, maps: tuple[str, ...], skill: int | None):
    """Remove map completion records."""
    wads = resolve_wad_query(query, allow_multiple=False)
    wad = wads[0]

    # Parse all map arguments
    all_maps = []
    for m in maps:
        all_maps.extend(_parse_map_range(m))

    removed = 0
    for map_name in all_maps:
        if db.remove_map_completion(wad["id"], map_name, skill=skill):
            removed += 1

    console.print(f"[green]Removed {removed} completion record(s)[/green]")


@map_cmd.command(name="list")
@click.argument("query")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
def map_list(query: str, plain: bool):
    """List completed maps for a WAD."""
    wads = resolve_wad_query(query, allow_multiple=False)
    wad = wads[0]

    completions = db.get_map_completions(wad["id"])

    if not completions:
        if plain:
            pass  # No output for scripting
        else:
            console.print("[dim]No completed maps[/dim]")
        return

    if plain:
        print("Map\tSkill\tDate")
        for c in completions:
            skill_name = SKILL_NAMES.get(c["skill"], str(c["skill"]) if c["skill"] else "")
            date = c["completed_at"][:10] if c["completed_at"] else ""
            print(f"{c['map_name']}\t{skill_name}\t{date}")
    else:
        table = Table(title=f"Completed Maps - {wad['title']}")
        table.add_column("Map", style="cyan")
        table.add_column("Skill")
        table.add_column("Date", style="dim")
        table.add_column("Notes")

        for c in completions:
            skill_name = SKILL_NAMES.get(c["skill"], str(c["skill"]) if c["skill"] else "-")
            date = c["completed_at"][:10] if c["completed_at"] else "-"
            table.add_row(c["map_name"], skill_name, date, c["notes"] or "-")

        console.print(table)


@map_cmd.command(name="progress")
@click.argument("query")
@click.option("--total", "-t", type=int, help="Total number of maps (for percentage)")
@click.option("--plain", is_flag=True, help="Output as key=value pairs (for scripting)")
def map_progress(query: str, total: int | None, plain: bool):
    """Show map completion progress for a WAD."""
    wads = resolve_wad_query(query, allow_multiple=False)
    wad = wads[0]

    stats = db.get_map_completion_stats(wad["id"])
    completed = stats["unique_maps"]

    if plain:
        print(f"completed={completed}")
        if total:
            print(f"total={total}")
            pct = (completed / total * 100) if total > 0 else 0
            print(f"percentage={pct:.1f}")
    else:
        if total:
            pct = (completed / total * 100) if total > 0 else 0
            console.print(f"[bold]Progress:[/bold] {completed}/{total} ({pct:.1f}%)")

            # Progress bar
            bar_width = 30
            filled = int(bar_width * completed / total) if total > 0 else 0
            bar = "█" * filled + "░" * (bar_width - filled)
            console.print(f"[cyan]{bar}[/cyan]")
        else:
            console.print(f"[bold]Completed maps:[/bold] {completed}")
            console.print("[dim]Use --total N to show percentage[/dim]")

        # Show by skill level
        if stats["by_map"]:
            by_skill = {1: 0, 2: 0, 3: 0, 4: 0, 5: 0}
            for map_name, max_skill in stats["by_map"].items():
                if max_skill and max_skill in by_skill:
                    by_skill[max_skill] += 1

            skill_summary = []
            for s in [5, 4, 3, 2, 1]:  # NM down to ITYTD
                if by_skill[s] > 0:
                    skill_summary.append(f"{SKILL_NAMES[s]}: {by_skill[s]}")
            if skill_summary:
                console.print(f"[dim]By highest skill: {', '.join(skill_summary)}[/dim]")


if __name__ == "__main__":
    cli()

"""Library management commands: list, info, update, delete, restore, random, link."""

import shutil
import sys
from pathlib import Path

import click

from caco import db
from caco.config import get_cache_dir, get_list_config
from caco.player import format_duration

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
    StatusChoice,
    SORT_FIELDS,
    _complete_query,
    _complete_sort,
    _parse_sort_option,
    _render_wad_list,
    _render_wad_list_plain,
    _render_wad_info_plain,
)


@cli.command(name="list")
@click.argument("query", nargs=-1, shell_complete=_complete_query)
@click.option("--sort", "-S", shell_complete=_complete_sort, help="Sort by: playtime, rating, created, title, author, last_played, year (suffix + for asc, - for desc)")
@click.option("--deleted", is_flag=True, help="Show deleted WADs (trash)")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
def list_cmd(query: tuple[str, ...], sort: str | None, deleted: bool, plain: bool):
    """List WADs in your library.

    Uses beets-style query syntax:

    \b
      caco list status:playing              # Filter by status
      caco list author:romero year:1994     # Multiple filters (AND)
      caco list "status:playing , status:to-play"  # OR queries
      caco list ^status:finished            # Negation (use ^ to avoid CLI issues)
      caco list tag:megawad ^tag:slaughter  # Combined filters
      caco list "ancient aliens"            # Free text search

    Query fields: id:, title:, author:, year:, filename:, tag:, status:, source:

    Negation: Use ^ prefix (e.g., ^status:finished). The - prefix also works but
    may conflict with CLI options.

    Customize display via config file: columns, colors, default sort.
    """
    # Load list config for defaults
    list_config = get_list_config()

    # Join query arguments
    query_str = " ".join(query) if query else None

    # Handle config default_status (convert list to OR query)
    if not query_str and list_config.get("default_status"):
        default_statuses = list_config["default_status"]
        if default_statuses:
            # Convert ["playing", "to-play"] -> "status:playing , status:to-play"
            status_queries = [f"status:{s}" for s in default_statuses]
            query_str = " , ".join(status_queries)

    # Use config sort if not specified via CLI
    if sort is None and list_config.get("sort"):
        sort = list_config["sort"]

    sort_field, sort_desc = _parse_sort_option(sort)
    if sort_field and sort_field not in SORT_FIELDS:
        err_console.print(f"[red]Invalid sort field: {sort_field}[/red]")
        err_console.print(f"[dim]Valid fields: {', '.join(SORT_FIELDS)}[/dim]")
        sys.exit(1)

    wads = db.search_wads(
        query=query_str,
        sort_by=sort_field,
        sort_desc=sort_desc,
        include_deleted=deleted,
    )

    # Adjust title for deleted view
    title = "Trash" if deleted else None

    if plain:
        _render_wad_list_plain(wads)
    else:
        _render_wad_list(wads, title=title, list_config=list_config)


@cli.command()
@click.argument("query")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
@click.option("--plain", is_flag=True, help="Output as key=value pairs (for scripting)")
def info(query: str, yes: bool, plain: bool):
    """Show details about a WAD. QUERY: WAD ID or query (e.g., filename:tnto)."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return  # User cancelled
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
    if wad.get("version"):
        console.print(f"[bold]Version:[/bold] {wad['version']}")

    if wad["rating"]:
        console.print(f"[bold]Rating:[/bold] {'\u2605' * wad['rating']}{'\u2606' * (5 - wad['rating'])}")

    if wad.get("tags"):
        console.print(f"[bold]Tags:[/bold] {', '.join(wad['tags'])}")

    console.print()
    console.print(f"[bold]Source:[/bold] {wad['source_type']}")
    if wad["source_url"]:
        console.print(f"[bold]URL:[/bold] {wad['source_url']}")
    if wad.get("idgames_id"):
        console.print(f"[bold]idgames ID:[/bold] {wad['idgames_id']}")

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

    # Completion stats
    times_beaten = db.get_times_beaten(wad_id)
    if times_beaten > 0:
        console.print()
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
@click.option("--title", "-t", help="Set WAD title")
@click.option("--author", "-a", help="Set WAD author")
@click.option("--clear-author", is_flag=True, help="Clear author")
@click.option("--year", type=int, help="Set release year")
@click.option("--clear-year", is_flag=True, help="Clear release year")
@click.option("--description", "-d", help="Set WAD description")
@click.option("--clear-description", is_flag=True, help="Clear description")
@click.option("--version", "-V", "version_str", help="Set version string (e.g., 'v1.0', 'RC2')")
@click.option("--clear-version", is_flag=True, help="Clear version")
@click.option("--status", "-s", type=StatusChoice())
@click.option("--rating", "-r", type=click.IntRange(1, 5))
@click.option("--notes", "-n")
@click.option("--iwad", help="Custom IWAD path for this WAD")
@click.option("--clear-iwad", is_flag=True, help="Clear custom IWAD")
@click.option("--sourceport", help="Custom sourceport for this WAD")
@click.option("--clear-sourceport", is_flag=True, help="Clear custom sourceport")
@click.option("--args", "custom_args", help="Custom arguments (JSON array or space-separated)")
@click.option("--clear-args", is_flag=True, help="Clear custom arguments")
@click.option("--idgames-id", help="Set idgames file ID for downloading")
@click.option("--clear-idgames-id", is_flag=True, help="Clear idgames file ID")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
def update(
    query: str,
    title: str | None,
    author: str | None,
    clear_author: bool,
    year: int | None,
    clear_year: bool,
    description: str | None,
    clear_description: bool,
    version_str: str | None,
    clear_version: bool,
    status: str | None,
    rating: int | None,
    notes: str | None,
    iwad: str | None,
    clear_iwad: bool,
    sourceport: str | None,
    clear_sourceport: bool,
    custom_args: str | None,
    clear_args: bool,
    idgames_id: str | None,
    clear_idgames_id: bool,
    yes: bool,
    dry_run: bool,
):
    """Update WAD metadata. QUERY: ID, ID range (3-6,9), or query (tag:megawad)."""
    import json

    updates = {}
    update_descriptions = []

    # Core metadata fields
    if title:
        updates["title"] = title
        update_descriptions.append(f"title \u2192 \"{title}\"")
    if author:
        updates["author"] = author
        update_descriptions.append(f"author \u2192 \"{author}\"")
    elif clear_author:
        updates["author"] = None
        update_descriptions.append("author \u2192 (cleared)")
    if year:
        updates["year"] = year
        update_descriptions.append(f"year \u2192 {year}")
    elif clear_year:
        updates["year"] = None
        update_descriptions.append("year \u2192 (cleared)")
    if description:
        updates["description"] = description
        desc_preview = description[:30] + "..." if len(description) > 30 else description
        update_descriptions.append(f"description \u2192 \"{desc_preview}\"")
    elif clear_description:
        updates["description"] = None
        update_descriptions.append("description \u2192 (cleared)")
    if version_str:
        updates["version"] = version_str
        update_descriptions.append(f"version \u2192 \"{version_str}\"")
    elif clear_version:
        updates["version"] = None
        update_descriptions.append("version \u2192 (cleared)")

    # Status and user fields
    if status:
        updates["status"] = db.Status(status)
        update_descriptions.append(f"status \u2192 {status}")
    if rating:
        updates["rating"] = rating
        update_descriptions.append(f"rating \u2192 {'\u2605' * rating}")
    if notes:
        updates["notes"] = notes
        update_descriptions.append(f"notes \u2192 \"{notes[:30]}{'...' if len(notes) > 30 else ''}\"")

    # Per-WAD play config
    if iwad:
        updates["custom_iwad"] = iwad
        update_descriptions.append(f"custom_iwad \u2192 {iwad}")
    elif clear_iwad:
        updates["custom_iwad"] = None
        update_descriptions.append("custom_iwad \u2192 (cleared)")
    if sourceport:
        updates["custom_sourceport"] = sourceport
        update_descriptions.append(f"custom_sourceport \u2192 {sourceport}")
    elif clear_sourceport:
        updates["custom_sourceport"] = None
        update_descriptions.append("custom_sourceport \u2192 (cleared)")
    if custom_args:
        # Accept JSON array or space-separated string
        try:
            args_list = json.loads(custom_args)
        except json.JSONDecodeError:
            args_list = custom_args.split()
        updates["custom_args"] = json.dumps(args_list)
        update_descriptions.append(f"custom_args \u2192 {args_list}")
    elif clear_args:
        updates["custom_args"] = None
        update_descriptions.append("custom_args \u2192 (cleared)")

    # Cross-source idgames download ID
    if idgames_id:
        updates["idgames_id"] = idgames_id
        update_descriptions.append(f"idgames_id \u2192 {idgames_id}")
    elif clear_idgames_id:
        updates["idgames_id"] = None
        update_descriptions.append("idgames_id \u2192 (cleared)")

    if not updates:
        err_console.print("[yellow]No updates specified[/yellow]")
        return

    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return  # User cancelled

    if dry_run:
        console.print(f"\n[bold]Would update {len(wads)} WAD(s):[/bold]\n")
        for wad in wads[:10]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(wads) > 10:
            console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
        console.print(f"\n[bold]Changes:[/bold]")
        for desc in update_descriptions:
            console.print(f"  \u2022 {desc}")
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    for wad in wads:
        db.update_wad(wad["id"], **updates)

    console.print(f"[green]Updated {len(wads)} WAD(s)[/green]")


@cli.command()
@click.argument("query", required=False)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted without deleting")
@click.option("--purge", is_flag=True, help="Permanently delete (skip trash)")
@click.option("--purge-all", is_flag=True, help="Permanently delete all items in trash")
def delete(query: str | None, yes: bool, dry_run: bool, purge: bool, purge_all: bool):
    """Delete WAD(s) from the library.

    By default, WADs are moved to trash and can be restored with 'caco restore'.
    Use --purge to permanently delete, or --purge-all to empty the trash.

    QUERY: ID, ID range (3-6,9), or query (status:abandoned).
    """
    # Handle --purge-all: empty the trash
    if purge_all:
        if dry_run:
            trash = db.search_wads(include_deleted=True)
            console.print(f"\n[bold]Would permanently delete {len(trash)} WAD(s) from trash[/bold]")
            console.print("\n[dim]No changes made (dry run)[/dim]")
            return

        if not yes:
            trash = db.search_wads(include_deleted=True)
            if not trash:
                console.print("[dim]Trash is empty[/dim]")
                return
            console.print(f"[yellow]This will permanently delete {len(trash)} WAD(s) from trash[/yellow]")
            if not click.confirm("Proceed?"):
                console.print("[dim]Cancelled[/dim]")
                return

        count = db.purge_all_deleted()
        console.print(f"[green]Permanently deleted {count} WAD(s) from trash[/green]")
        return

    if not query:
        err_console.print("[red]Query required (or use --purge-all to empty trash)[/red]")
        sys.exit(1)

    # For dry-run and preview, we want to resolve without the normal confirmation
    # since we'll show our own detailed preview
    wads = resolve_wad_query(query, mode="multiple", yes=True)
    if not wads:
        return

    # Gather stats for all WADs to be deleted
    total_sessions = 0
    total_playtime = 0

    action = "permanently deleted" if purge else "moved to trash"
    console.print(f"\n[bold]The following WADs will be {action}:[/bold]\n")
    for wad in wads:
        stats = db.get_wad_stats(wad["id"])
        total_sessions += stats["session_count"]
        total_playtime += stats["total_playtime"]

        # Format WAD info
        author_year = []
        if wad.get("author"):
            author_year.append(wad["author"])
        if wad.get("year"):
            author_year.append(str(wad["year"]))
        info_str = f" ({', '.join(author_year)})" if author_year else ""

        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}{info_str}")

    # Show associated data that will be deleted (only for purge)
    if purge and total_sessions:
        console.print(f"\n[dim]This will also delete:[/dim]")
        playtime_str = format_duration(total_playtime) if total_playtime else "0s"
        console.print(f"  \u2022 {total_sessions} play session(s) ({playtime_str})")

    if not purge:
        console.print(f"\n[dim]Use 'caco restore' to recover, or 'caco delete --purge' to permanently delete[/dim]")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    # Ask for confirmation unless --yes
    if not yes:
        console.print()
        if not click.confirm(f"Proceed?"):
            console.print("[dim]Cancelled[/dim]")
            return

    # Perform deletion
    for wad in wads:
        db.delete_wad(wad["id"], purge=purge)

    if purge:
        console.print(f"\n[green]Permanently deleted {len(wads)} WAD(s)[/green]")
    else:
        console.print(f"\n[green]Moved {len(wads)} WAD(s) to trash[/green]")


@cli.command()
@click.argument("query")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def restore(query: str, yes: bool):
    """Restore deleted WAD(s) from trash.

    QUERY: ID, ID range (3-6,9), or query (author:romero).
    Use 'caco list --deleted' to see items in trash.
    """
    # Search only in deleted WADs
    wads = db.search_wads(query=query, include_deleted=True)
    if not wads:
        err_console.print(f"[red]No deleted WADs matching '{query}'[/red]")
        sys.exit(1)

    # Preview
    console.print(f"\n[bold]The following WADs will be restored:[/bold]\n")
    for wad in wads:
        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")

    if not yes and len(wads) > 1:
        console.print()
        if not click.confirm(f"Restore {len(wads)} WAD(s)?"):
            console.print("[dim]Cancelled[/dim]")
            return

    # Restore
    restored = 0
    for wad in wads:
        if db.restore_wad(wad["id"]):
            restored += 1

    console.print(f"\n[green]Restored {restored} WAD(s)[/green]")


@cli.command()
@click.argument("query")
@click.argument("file_path", type=click.Path(exists=True))
@click.option("--move", "-m", is_flag=True, help="Move file instead of copying")
def link(query: str, file_path: str, move: bool):
    """Link a local file to an existing library entry.

    Use this to attach a downloaded WAD file to a Doomwiki import
    or any other metadata-only entry.

    The file is copied (or moved with --move) to the cache directory.

    \b
    Examples:
        caco link 73 ~/Downloads/heartland.wad
        caco link "Heartland" ~/Downloads/heartland.wad --move
    """
    # Resolve the WAD
    wads = resolve_wad_query(query, mode="single")
    if not wads:
        return
    wad = wads[0]

    source_path = Path(file_path).resolve()
    cache_dir = get_cache_dir()
    cache_dir.mkdir(parents=True, exist_ok=True)

    # Generate cache filename: wad_id_original_filename
    dest_filename = f"{wad['id']}_{source_path.name}"
    dest_path = cache_dir / dest_filename

    # Check if already linked
    if wad.get("cached_path"):
        existing = Path(wad["cached_path"])
        if existing.exists():
            console.print(f"[yellow]Already linked:[/yellow] {existing.name}")
            if not click.confirm("Replace with new file?"):
                console.print("[dim]Cancelled[/dim]")
                return
            # Remove old file
            existing.unlink()

    # Copy or move the file
    try:
        if move:
            shutil.move(str(source_path), str(dest_path))
            action = "Moved"
        else:
            shutil.copy2(str(source_path), str(dest_path))
            action = "Copied"
    except OSError as e:
        err_console.print(f"[red]Failed to {('move' if move else 'copy')} file: {e}[/red]")
        sys.exit(1)

    # Update database
    db.update_wad(
        wad["id"],
        cached_path=str(dest_path),
        filename=source_path.name,
    )

    console.print(f"[green]{action}:[/green] {source_path.name}")
    console.print(f"[green]Linked to:[/green] {wad['title']} (ID: {wad['id']})")


@cli.command(name="random")
@click.argument("query", nargs=-1)
def random_cmd(query: tuple[str, ...]):
    """Pick a random WAD. Prints the WAD ID (for scripting).

    Supports the same query syntax as 'caco list' for filtering.

    \b
    Examples:
        caco random                        # Random WAD from entire library
        caco random status:to-play         # Random to-play WAD
        caco play $(caco random)           # Play a random WAD
        caco play $(caco random tag:megawad)  # Play a random megawad
    """
    import random as rand

    query_str = " ".join(query) if query else None
    wads = db.search_wads(query=query_str)
    if not wads:
        err_console.print("[red]No matching WADs[/red]")
        sys.exit(1)
    wad = rand.choice(wads)
    print(wad["id"])

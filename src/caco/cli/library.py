"""Library management commands: ls, info, modify, trash, random."""

import json
import shutil
import sys
from pathlib import Path
from typing import Any

import click
from rich.table import Table

from caco import db
from caco.config import get_cache_dir, get_iwad_dir, get_list_config
from caco.player import format_duration
from caco.utils import format_rating

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
    _complete_query,
    _render_wad_list,
    _render_wad_list_plain,
    _render_wad_list_json,
    _render_wad_info_plain,
    _render_wad_info_json,
)
from caco.cli.parsing import (
    SORT_FIELDS,
    _parse_sort_option,
    extract_sort_from_args,
    parse_modify_args,
)


@cli.command(name="ls")
@click.argument("args", nargs=-1, shell_complete=_complete_query)
@click.option("--output", "-o", type=click.Choice(["json", "plain"]), help="Output format")
@click.option("--deleted", is_flag=True, hidden=True, help="Show deleted WADs (use 'trash --list')")
@click.option("--tags", is_flag=True, help="List all tags with counts")
@click.option("--iwad", "iwad_flag", is_flag=True, help="List registered IWADs")
def ls_cmd(args: tuple[str, ...], output: str | None, deleted: bool, tags: bool, iwad_flag: bool):
    """List WADs in your library.

    \b
    Sort inline with field+ (ascending) or field- (descending):
      caco ls playtime-                    # Sort by playtime descending
      caco ls status:playing title+        # Filter + sort by title ascending

    \b
    Query syntax (beets-style):
      caco ls status:playing               # Filter by status
      caco ls author:romero year:1994      # Multiple filters (implicit AND)
      caco ls "status:playing , status:to-play"  # OR queries
      caco ls ^status:finished             # Negation
      caco ls tag:megawad ^tag:slaughter   # Combined filters
      caco ls "ancient aliens"             # Free text search
      caco ls tag:caco*                    # Glob patterns

    \b
    Special modes:
      caco ls --tags                       # List all tags with counts
      caco ls --iwad                       # List registered IWADs

    \b
    Sort fields: id, playtime, rating, created, title, author, last_played, year
    Query fields: id:, title:, author:, year:, filename:, tag:, status:, source:, iwad:
    """
    # Mutually exclusive modes
    if tags and iwad_flag:
        err_console.print("[red]--tags and --iwad are mutually exclusive[/red]")
        sys.exit(1)

    # --tags mode: show tag counts
    if tags:
        tag_counts = db.get_tag_counts()
        if not tag_counts:
            if output == "plain":
                return
            console.print("[dim]No tags[/dim]")
            return

        if output == "json":
            print(json.dumps([{"tag": t, "count": c} for t, c in tag_counts], indent=2))
            return
        if output == "plain":
            print("Tag\tCount")
            for tag, count in tag_counts:
                print(f"{tag}\t{count}")
            return

        table = Table(title=f"Tags ({len(tag_counts)})")
        table.add_column("Tag", style="cyan")
        table.add_column("WADs", justify="right")
        for tag, count in tag_counts:
            table.add_row(tag, str(count))
        console.print(table)
        return

    # --iwad mode: show registered IWADs
    if iwad_flag:
        iwads = db.get_all_iwads()

        if output == "plain":
            print("Family\tVariant\tTitle\tPath\tMD5")
            for iwad in iwads:
                print(
                    f"{iwad['family']}\t{iwad['variant']}\t{iwad.get('title') or ''}"
                    f"\t{iwad['path']}\t{iwad.get('md5') or ''}"
                )
            return

        if output == "json":
            print(json.dumps([dict(i) for i in iwads], indent=2))
            return

        if not iwads:
            console.print("[dim]No IWADs registered[/dim]")
            console.print("[dim]Use 'caco iwad import <path>' to import an IWAD file or directory[/dim]")
            return

        preferred: set[tuple[str, str]] = set()
        families_seen: set[str] = set()
        for iwad in iwads:
            fam = iwad["family"]
            if fam not in families_seen:
                families_seen.add(fam)
                pref = db.get_iwad(fam)
                if pref:
                    preferred.add((pref["family"], pref["variant"]))

        table = Table(title=f"Registered IWADs ({len(iwads)})")
        table.add_column("Family", style="cyan")
        table.add_column("Variant")
        table.add_column("Title")
        table.add_column("Path", style="dim")

        for iwad in iwads:
            path_str = iwad["path"]
            if not Path(path_str).exists():
                path_str = f"[red]{path_str} (missing)[/red]"
            is_preferred = (iwad["family"], iwad["variant"]) in preferred
            variant_display = iwad["variant"]
            if is_preferred:
                variant_display = f"[bold green]{variant_display} *[/bold green]"
            table.add_row(
                iwad["family"],
                variant_display,
                iwad.get("title") or "-",
                path_str,
            )
        console.print(table)
        return

    # Default mode: list WADs
    list_config = get_list_config()

    # Extract inline sort from args
    remaining, sort_str = extract_sort_from_args(args)
    query_str = " ".join(remaining) if remaining else None

    # Handle config default_status
    if not query_str and list_config.get("default_status"):
        default_statuses = list_config["default_status"]
        if default_statuses:
            status_queries = [f"status:{s}" for s in default_statuses]
            query_str = " , ".join(status_queries)

    # Use config sort if not specified inline
    if sort_str is None and list_config.get("sort"):
        sort_str = list_config["sort"]

    sort_field, sort_desc = _parse_sort_option(sort_str)
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

    title = "Trash" if deleted else None

    if output == "json":
        _render_wad_list_json(wads)
    elif output == "plain":
        _render_wad_list_plain(wads)
    else:
        _render_wad_list(wads, title=title, list_config=list_config)


@cli.command()
@click.argument("query")
@click.option("--output", "-o", type=click.Choice(["json", "plain"]), help="Output format")
def info(query: str, output: str | None):
    """Show details about a WAD.

    Multiple matches are displayed in sequence, separated by a rule.

    \b
    QUERY: WAD ID, ID range (3-6,9), or query (e.g., filename:tnto).
    """
    from caco.cli import _parse_id_range

    # Try ID range first
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
    else:
        wads = db.search_wads(query=query)

    if not wads:
        err_console.print(f"[red]No WADs matching '{query}'[/red]")
        sys.exit(1)

    if output == "json":
        if len(wads) == 1:
            _render_wad_info_json(wads[0])
        else:
            # Multiple: output as JSON array
            results = []
            for wad in wads:
                playtime = db.get_total_playtime(wad["id"])
                sessions = db.get_sessions(wad["id"])
                last_played = db.get_last_played(wad["id"])
                times_beaten = db.get_times_beaten(wad["id"])
                results.append({
                    "id": wad["id"],
                    "title": wad["title"],
                    "author": wad.get("author"),
                    "year": wad.get("year"),
                    "status": wad["status"],
                    "rating": wad.get("rating"),
                    "tags": wad.get("tags", []),
                    "playtime_seconds": playtime,
                    "playtime": format_duration(playtime) if playtime else None,
                    "session_count": len(sessions),
                    "times_beaten": times_beaten,
                    "last_played": last_played,
                })
            print(json.dumps(results, indent=2))
        return

    for i, wad in enumerate(wads):
        if i > 0:
            if output == "plain":
                print("---")
            else:
                console.rule()

        wad_id = wad["id"]

        if output == "plain":
            _render_wad_info_plain(wad)
            continue

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
            console.print(f"[bold]Rating:[/bold] {format_rating(wad['rating'])}")

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

        times_beaten = db.get_times_beaten(wad_id)
        if times_beaten > 0:
            console.print()
            console.print(f"[bold]Times beaten:[/bold] {times_beaten}")

        if wad.get("custom_iwad") or wad.get("custom_sourceport") or wad.get("custom_args") or wad.get("companion_files"):
            console.print()
            console.print("[bold]Custom play config:[/bold]")
            if wad.get("custom_iwad"):
                console.print(f"  IWAD: {wad['custom_iwad']}")
            if wad.get("custom_sourceport"):
                console.print(f"  Sourceport: {wad['custom_sourceport']}")
            if wad.get("custom_args"):
                try:
                    parsed_args = json.loads(wad["custom_args"])
                    console.print(f"  Args: {' '.join(parsed_args)}")
                except json.JSONDecodeError:
                    console.print(f"  Args: {wad['custom_args']}")
            if wad.get("companion_files"):
                try:
                    files = json.loads(wad["companion_files"])
                    console.print(f"  Companion files:")
                    for f in files:
                        console.print(f"    {f}")
                except json.JSONDecodeError:
                    pass


@cli.command()
@click.argument("args", nargs=-1)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
@click.option("--link", "link_path", type=click.Path(exists=True), help="Link a local file to the WAD(s)")
@click.option("--add-file", "add_files", multiple=True, type=click.Path(exists=True), help="Add a companion file (DEH, music WAD, etc.)")
@click.option("--remove-file", "remove_files", multiple=True, help="Remove a companion file (by basename or full path)")
def modify(args: tuple[str, ...], yes: bool, dry_run: bool, link_path: str | None,
           add_files: tuple[str, ...], remove_files: tuple[str, ...]):
    """Modify WAD metadata using beets-style field=value syntax.

    \b
    Set fields:
      caco modify id:1 status=playing          # Set status
      caco modify id:1 rating=4 notes="great"  # Set multiple fields
      caco modify id:1 tag=megawad             # Add a tag
      caco modify id:1 iwad=doom2              # Set custom IWAD

    \b
    Clear fields:
      caco modify id:1 !author                 # Clear author
      caco modify id:1 !tag                    # Remove all tags
      caco modify id:1 !tag:slaughter          # Remove matching tags

    \b
    Companion files (DEH patches, music WADs, etc.):
      caco modify id:1 --add-file /path/to/music.wad
      caco modify id:1 --add-file /path/to/patch.deh
      caco modify id:1 --remove-file music.wad    # Remove by basename
      caco modify id:1 --remove-file /full/path   # Remove by full path

    \b
    Link a file:
      caco modify id:1 --link ~/Downloads/wad.wad

    \b
    Modifiable fields: title, author, year, description, status, rating,
      notes, iwad, sourceport, args, idgames-id, version, tag
    """
    from caco.config import get_link_mode

    query_terms, actions, _sort = parse_modify_args(args)

    if not actions and not link_path and not add_files and not remove_files:
        err_console.print("[yellow]No modifications specified[/yellow]")
        err_console.print("[dim]Use field=value to set, !field to clear, tag=name to add tags[/dim]")
        return

    if not query_terms:
        err_console.print("[red]No query specified — provide a WAD ID or query to match[/red]")
        sys.exit(1)

    query_str = " ".join(query_terms)
    wads = resolve_wad_query(query_str, mode="multiple", yes=yes)
    if not wads:
        return

    # Build descriptions for dry-run / confirmation
    descriptions: list[str] = []
    for action in actions:
        if action.action == "set":
            descriptions.append(f"{action.field} \u2192 \"{action.value}\"")
        elif action.action == "clear":
            descriptions.append(f"{action.field} \u2192 (cleared)")
        elif action.action == "add_tag":
            descriptions.append(f"add tag: {action.value}")
        elif action.action == "remove_all_tags":
            descriptions.append("remove all tags")
        elif action.action == "remove_tag":
            descriptions.append(f"remove tags matching: {action.pattern}")
    if link_path:
        descriptions.append(f"link file: {link_path}")
    for af in add_files:
        descriptions.append(f"add companion file: {af}")
    for rf in remove_files:
        descriptions.append(f"remove companion file: {rf}")

    if dry_run:
        console.print(f"\n[bold]Would modify {len(wads)} WAD(s):[/bold]\n")
        for wad in wads[:10]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(wads) > 10:
            console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
        console.print(f"\n[bold]Changes:[/bold]")
        for desc in descriptions:
            console.print(f"  \u2022 {desc}")
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    # Apply modifications
    for wad in wads:
        updates: dict[str, Any] = {}

        for action in actions:
            if action.action == "set":
                value: Any = action.value
                if action.field == "status":
                    value = db.Status(value)
                elif action.field == "rating":
                    value = int(value)
                elif action.field == "year":
                    value = int(value)
                updates[action.field] = value

            elif action.action == "clear":
                updates[action.field] = None

            elif action.action == "add_tag":
                db.add_tag(wad["id"], action.value)

            elif action.action == "remove_all_tags":
                db.remove_all_tags(wad["id"])

            elif action.action == "remove_tag":
                db.remove_tags_by_pattern(wad["id"], action.pattern)

        if updates:
            db.update_wad(wad["id"], **updates)

    # Handle --link
    if link_path:
        source = Path(link_path).resolve()
        if not source.is_file():
            err_console.print(f"[red]Error: {link_path} is not a regular file[/red]")
            sys.exit(1)

        cache_dir = get_cache_dir()
        cache_dir.mkdir(parents=True, exist_ok=True)
        link_mode = get_link_mode()

        for wad in wads:
            dest_filename = f"{wad['id']}_{source.name}"
            dest = cache_dir / dest_filename

            # Remove old linked file if exists
            if wad.get("cached_path"):
                old = Path(wad["cached_path"])
                if old.exists():
                    old.unlink()

            try:
                if link_mode == "move":
                    shutil.move(str(source), str(dest))
                else:
                    shutil.copy2(str(source), str(dest))
            except OSError as e:
                err_console.print(f"[red]Failed to {link_mode} file: {e}[/red]")
                sys.exit(1)

            db.update_wad(wad["id"], cached_path=str(dest), filename=source.name)

    # Handle --add-file / --remove-file
    if add_files or remove_files:
        for wad in wads:
            existing_raw = wad.get("companion_files")
            try:
                existing: list[str] = json.loads(existing_raw) if existing_raw else []
            except json.JSONDecodeError:
                existing = []

            # Add files (resolve to absolute, deduplicate)
            for af in add_files:
                abs_path = str(Path(af).resolve())
                if abs_path not in existing:
                    existing.append(abs_path)

            # Remove files (match by basename or full path)
            for rf in remove_files:
                rf_basename = Path(rf).name
                existing = [
                    p for p in existing
                    if p != rf and Path(p).name != rf_basename
                ]

            companion_json = json.dumps(existing) if existing else None
            db.update_wad(wad["id"], companion_files=companion_json)

    console.print(f"[green]Modified {len(wads)} WAD(s)[/green]")




@cli.command()
@click.argument("args", nargs=-1)
@click.option("--list", "list_trash", is_flag=True, help="Show trashed WADs")
@click.option("--purge", is_flag=True, help="Permanently delete (no query = purge all trash)")
@click.option("--restore", "restore_flag", is_flag=True, help="Restore from trash")
@click.option("--iwad", "iwad_target", type=str, help="Remove IWAD (FAMILY or FAMILY/VARIANT)")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
@click.option("--dry-run", is_flag=True, help="Show what would happen without making changes")
@click.option("--output", "-o", type=click.Choice(["json", "plain"]), help="Output format (with --list)")
def trash(
    args: tuple[str, ...],
    list_trash: bool,
    purge: bool,
    restore_flag: bool,
    iwad_target: str | None,
    yes: bool,
    dry_run: bool,
    output: str | None,
):
    """Manage trash and removals.

    \b
    Modes:
      caco trash <query>               # Move WAD(s) to trash
      caco trash --list                # Show trashed WADs
      caco trash --purge               # Permanently delete all trash
      caco trash --purge <query>       # Permanently delete matching trash
      caco trash --restore <query>     # Restore from trash
      caco trash --iwad doom2          # Remove IWAD family
      caco trash --iwad doom2/bfg      # Remove IWAD variant
    """
    from caco.db._iwads import remove_iwad_with_paths

    # --iwad mode: remove IWAD
    if iwad_target:
        iwad_dir = get_iwad_dir()
        if "/" in iwad_target:
            family, variant = iwad_target.split("/", 1)
        else:
            family = iwad_target
            variant = None

        if variant:
            paths = remove_iwad_with_paths(family, variant)
            if paths:
                if not dry_run:
                    _delete_managed_files(paths, iwad_dir)
                console.print(f"[green]Removed:[/green] {family}/{variant}")
            else:
                err_console.print(f"[red]IWAD '{family}/{variant}' not found[/red]")
                sys.exit(1)
        else:
            variants = db.get_family_iwads(family)
            if not variants:
                err_console.print(f"[red]No IWADs registered for family '{family}'[/red]")
                sys.exit(1)

            if len(variants) > 1 and not yes:
                variant_names = ", ".join(v["variant"] for v in variants)
                if not click.confirm(
                    f"Remove all {len(variants)} variants of {family} ({variant_names})?",
                    default=False,
                ):
                    return

            if dry_run:
                console.print(f"[bold]Would remove {len(variants)} variant(s) of {family}[/bold]")
                console.print("\n[dim]No changes made (dry run)[/dim]")
                return

            paths = remove_iwad_with_paths(family)
            _delete_managed_files(paths, iwad_dir)
            console.print(f"[green]Removed {len(paths)} variant(s) of {family}[/green]")
        return

    # --list mode: show trashed WADs
    if list_trash:
        wads = db.search_wads(include_deleted=True)
        if output == "json":
            _render_wad_list_json(wads)
        elif output == "plain":
            _render_wad_list_plain(wads)
        else:
            _render_wad_list(wads, title="Trash")
        return

    # --restore mode
    if restore_flag:
        query_str = " ".join(args) if args else None
        if not query_str:
            err_console.print("[red]Query required for --restore[/red]")
            sys.exit(1)

        wads = db.search_wads(query=query_str, include_deleted=True)
        if not wads:
            err_console.print(f"[red]No deleted WADs matching '{query_str}'[/red]")
            sys.exit(1)

        if dry_run:
            console.print(f"\n[bold]Would restore {len(wads)} WAD(s)[/bold]")
            for wad in wads[:10]:
                console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
            console.print("\n[dim]No changes made (dry run)[/dim]")
            return

        console.print(f"\n[bold]The following WADs will be restored:[/bold]\n")
        for wad in wads:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")

        if not yes and len(wads) > 1:
            console.print()
            if not click.confirm(f"Restore {len(wads)} WAD(s)?"):
                console.print("[dim]Cancelled[/dim]")
                return

        restored = 0
        for wad in wads:
            if db.restore_wad(wad["id"]):
                restored += 1
        console.print(f"\n[green]Restored {restored} WAD(s)[/green]")
        return

    # --purge mode
    if purge:
        query_str = " ".join(args) if args else None
        if not query_str:
            # Purge all trash
            if dry_run:
                count = len(db.search_wads(include_deleted=True))
                console.print(f"\n[bold]Would permanently delete {count} WAD(s) from trash[/bold]")
                console.print("\n[dim]No changes made (dry run)[/dim]")
                return

            if not yes:
                trash_wads = db.search_wads(include_deleted=True)
                if not trash_wads:
                    console.print("[dim]Trash is empty[/dim]")
                    return
                console.print(f"[yellow]This will permanently delete {len(trash_wads)} WAD(s) from trash[/yellow]")
                if not click.confirm("Proceed?"):
                    console.print("[dim]Cancelled[/dim]")
                    return

            count = db.purge_all_deleted()
            console.print(f"[green]Permanently deleted {count} WAD(s) from trash[/green]")
        else:
            # Purge matching
            wads = resolve_wad_query(query_str, mode="multiple", yes=True)
            if not wads:
                return

            if dry_run:
                console.print(f"\n[bold]Would permanently delete {len(wads)} WAD(s):[/bold]\n")
                for wad in wads[:10]:
                    console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
                console.print("\n[dim]No changes made (dry run)[/dim]")
                return

            if not yes:
                console.print(f"\n[bold]The following WADs will be permanently deleted:[/bold]\n")
                for wad in wads[:10]:
                    console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
                if len(wads) > 10:
                    console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
                console.print()
                if not click.confirm("Proceed?"):
                    console.print("[dim]Cancelled[/dim]")
                    return

            for wad in wads:
                db.delete_wad(wad["id"], purge=True)
            console.print(f"\n[green]Permanently deleted {len(wads)} WAD(s)[/green]")
        return

    # Default mode: soft-delete (move to trash)
    query_str = " ".join(args) if args else None
    if not query_str:
        err_console.print("[red]Query required (or use --list, --purge, --restore)[/red]")
        sys.exit(1)

    wads = resolve_wad_query(query_str, mode="multiple", yes=True)
    if not wads:
        return

    # Gather stats for preview
    total_sessions = 0
    total_playtime = 0

    console.print(f"\n[bold]The following WADs will be moved to trash:[/bold]\n")
    for wad in wads:
        stats = db.get_wad_stats(wad["id"])
        total_sessions += stats["session_count"]
        total_playtime += stats["total_playtime"]

        author_year = []
        if wad.get("author"):
            author_year.append(wad["author"])
        if wad.get("year"):
            author_year.append(str(wad["year"]))
        info_str = f" ({', '.join(author_year)})" if author_year else ""
        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}{info_str}")

    console.print(f"\n[dim]Use 'caco trash --restore' to recover[/dim]")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        console.print()
        if not click.confirm("Proceed?"):
            console.print("[dim]Cancelled[/dim]")
            return

    for wad in wads:
        db.delete_wad(wad["id"])

    console.print(f"\n[green]Moved {len(wads)} WAD(s) to trash[/green]")


def _delete_managed_files(paths: list[str], iwad_dir: Path) -> None:
    """Delete files that live inside the managed IWAD directory."""
    resolved_iwad_dir = iwad_dir.resolve()
    for path_str in paths:
        p = Path(path_str)
        try:
            resolved = p.resolve()
            if p.exists() and resolved.is_relative_to(resolved_iwad_dir):
                p.unlink()
                if p.parent.resolve() != resolved_iwad_dir:
                    try:
                        p.parent.rmdir()
                    except OSError:
                        pass
        except OSError:
            pass


@cli.command(name="random")
@click.argument("query", nargs=-1)
@click.option("--info", is_flag=True, help="Print ID, title, and author (TSV)")
def random_cmd(query: tuple[str, ...], info: bool):
    """Pick a random WAD. Prints the WAD ID (for scripting).

    Supports the same query syntax as 'caco list' for filtering.

    \b
    Examples:
        caco random                        # Random WAD from entire library
        caco random status:to-play         # Random to-play WAD
        caco random --info                 # Print ID, title, and author
        caco play $(caco random)           # Play a random WAD
        caco play $(caco random tag:megawad)  # Play a random megawad
    """
    query_str = " ".join(query) if query else None
    wads = db.search_wads(query=query_str, sort_by="random", limit=1)
    if not wads:
        err_console.print("[red]No matching WADs[/red]")
        sys.exit(1)
    wad = wads[0]
    if info:
        print(f"{wad['id']}\t{wad['title']}\t{wad['author'] or ''}")
    else:
        print(wad["id"])

"""Library management commands: ls, info, modify, trash, random, enrich."""

import json
import logging
import shutil
import sys
from pathlib import Path
from typing import Any

import click
from rich.table import Table

from caco import db
from caco.config import get_cache_dir, get_id24_dir, get_iwad_dir, get_list_config
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

logger = logging.getLogger(__name__)


@cli.command(name="ls")
@click.argument("args", nargs=-1, shell_complete=_complete_query)
@click.option("--output", "-o", type=click.Choice(["json", "plain"]), help="Output format")
@click.option("--deleted", is_flag=True, hidden=True, help="Show deleted WADs (use 'trash --list')")
@click.option("--tags", is_flag=True, help="List all tags with counts")
@click.option("--iwad", "iwad_flag", is_flag=True, help="List registered IWADs")
@click.option("--id24", "id24_flag", is_flag=True, help="List registered id24 WADs")
def ls_cmd(args: tuple[str, ...], output: str | None, deleted: bool, tags: bool, iwad_flag: bool, id24_flag: bool):
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
      caco ls --id24                       # List registered id24 WADs

    \b
    Sort fields: id, playtime, rating, created, title, author, last_played, year
    Query fields: id:, title:, author:, year:, filename:, tag:, status:, source:, iwad:, complevel:, config:
    """
    # Mutually exclusive modes
    special_flags = sum([tags, iwad_flag, id24_flag])
    if special_flags > 1:
        err_console.print("[red]--tags, --iwad, and --id24 are mutually exclusive[/red]")
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
            console.print("[dim]Use 'caco import <path>' to import an IWAD file or directory[/dim]")
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

    # --id24 mode: show registered id24 WADs
    if id24_flag:
        id24s = db.get_all_id24()

        if output == "plain":
            print("Name\tVersion\tTitle\tPath\tMD5")
            for w in id24s:
                print(
                    f"{w['name']}\t{w.get('version') or ''}\t{w.get('title') or ''}"
                    f"\t{w['path']}\t{w.get('md5') or ''}"
                )
            return

        if output == "json":
            print(json.dumps([dict(w) for w in id24s], indent=2))
            return

        if not id24s:
            console.print("[dim]No id24 WADs registered[/dim]")
            console.print("[dim]Use 'caco import <path>' to import id24 WAD files[/dim]")
            return

        table = Table(title=f"Registered id24 WADs ({len(id24s)})")
        table.add_column("Name", style="cyan")
        table.add_column("Version")
        table.add_column("Title")
        table.add_column("Path", style="dim")

        for w in id24s:
            path_str = w["path"]
            if not Path(path_str).exists():
                path_str = f"[red]{path_str} (missing)[/red]"
            table.add_row(
                w["name"],
                w.get("version") or "-",
                w.get("title") or "-",
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
@click.option("--levelstats", is_flag=True, help="Show per-map statistics")
@click.option("-b", "beaten_target", help="Target completion by timestamp (for --levelstats)")
@click.option("--live", is_flag=True, help="Show only live stats (with --levelstats)")
@click.option("--plain", "stats_plain", is_flag=True, help="TSV output (with --levelstats)")
def info(
    query: str,
    output: str | None,
    levelstats: bool,
    beaten_target: str | None,
    live: bool,
    stats_plain: bool,
):
    """Show details about a WAD.

    Multiple matches are displayed in sequence, separated by a rule.

    \b
    QUERY: WAD ID, ID range (3-6,9), or query (e.g., filename:tnto).

    \b
    Levelstats mode:
      caco info 1 --levelstats                  # All stats entries
      caco info 1 --levelstats --live            # Live stats only
      caco info 1 --levelstats -b 2024-06-15    # Specific completion
      caco info 1 --levelstats --plain           # TSV output
    """
    from caco.cli import _parse_id_range

    # --levelstats mode implied by related flags
    if beaten_target or live or stats_plain:
        levelstats = True

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

    # --levelstats mode: show per-map statistics
    if levelstats:
        from caco.cli.stats import _build_stats_entries, _print_entry, _entry_label

        for wi, wad in enumerate(wads):
            if wi > 0:
                if stats_plain:
                    print()
                else:
                    console.print()

            if live:
                if not wad.get("stats_snapshot"):
                    err_console.print(f"[dim]No live stats for {wad['title']}[/dim]")
                    continue
                entry = {
                    "id": None, "completed_at": None,
                    "stats_snapshot": wad["stats_snapshot"],
                    "notes": None, "_live": True,
                }
                if not stats_plain:
                    console.print(f"\n[bold]{wad['title']}[/bold] — Map Statistics\n")
                _print_entry(entry, plain=stats_plain)
                continue

            if beaten_target:
                comp = db.find_completion_by_timestamp(wad["id"], beaten_target)
                if not comp:
                    err_console.print(f"[red]No completion matching '{beaten_target}' for {wad['title']}[/red]")
                    continue
                if not comp.get("stats_snapshot"):
                    err_console.print(f"[dim]Completion has no stats attached[/dim]")
                    continue
                if not stats_plain:
                    console.print(f"\n[bold]{wad['title']}[/bold] — Map Statistics\n")
                _print_entry(comp, plain=stats_plain)
                continue

            # Default: show all entries
            entries = _build_stats_entries(wad)
            if not entries:
                err_console.print(f"[dim]No stats available for {wad['title']}[/dim]")
                continue

            if not stats_plain:
                console.print(f"\n[bold]{wad['title']}[/bold] — Map Statistics\n")

            for ei, entry in enumerate(entries):
                if ei > 0:
                    if stats_plain:
                        print()
                    else:
                        console.print()
                _print_entry(entry, plain=stats_plain)
        return

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
                completions = db.get_wad_completions(wad["id"])
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
                    "completions": [
                        {
                            "completed_at": c["completed_at"],
                            "notes": c.get("notes"),
                            "has_stats": bool(c.get("stats_snapshot")),
                        }
                        for c in completions
                    ],
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

        # Completions section (replaces simple "Times beaten: N")
        completions = db.get_wad_completions(wad_id)
        if completions:
            console.print()
            console.print(f"[bold]Completions ({len(completions)}):[/bold]")
            for c in completions:
                c_date = c["completed_at"][:16].replace("T", " ") if c.get("completed_at") else "-"
                c_notes = c.get("notes") or ""
                c_stats = " [green]*[/green]" if c.get("stats_snapshot") else ""
                parts = [f"  {c_date}"]
                if c_notes:
                    parts.append(f"  {c_notes}")
                parts.append(c_stats)
                console.print("".join(parts))

        companions = db.get_wad_companions(wad["id"])
        has_play_config = (
            wad.get("custom_iwad") or wad.get("custom_sourceport")
            or wad.get("custom_args") or wad.get("complevel") is not None
            or wad.get("custom_config") or companions
        )
        if has_play_config:
            console.print()
            console.print("[bold]Custom play config:[/bold]")
            if wad.get("custom_iwad"):
                console.print(f"  IWAD: {wad['custom_iwad']}")
            if wad.get("custom_sourceport"):
                console.print(f"  Sourceport: {wad['custom_sourceport']}")
            if wad.get("complevel") is not None:
                from caco.complevel import complevel_name
                console.print(f"  Complevel: {wad['complevel']} ({complevel_name(wad['complevel'])})")
            if wad.get("custom_config"):
                console.print(f"  Config: {wad['custom_config']}")
            if wad.get("custom_args"):
                try:
                    parsed_args = json.loads(wad["custom_args"])
                    console.print(f"  Args: {' '.join(parsed_args)}")
                except json.JSONDecodeError:
                    console.print(f"  Args: {wad['custom_args']}")
            if companions:
                console.print("  Companion files:")
                for comp in companions:
                    status = "" if comp["enabled"] else " [dim](disabled)[/dim]"
                    console.print(f"    {comp['filename']}{status}")


@cli.command()
@click.argument("args", nargs=-1)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
@click.option("--link", "link_path", type=click.Path(exists=True), help="Link a local file to the WAD(s)")
@click.option("--add-file", "add_files", multiple=True, type=click.Path(exists=True), help="Add a companion file (DEH, music WAD, etc.)")
@click.option("--remove-file", "remove_files", multiple=True, help="Remove a companion file (by basename or full path)")
@click.option("--notes", help="Notes for beaten+N completions")
@click.option("--stats-file", "-s", type=click.Path(exists=True),
              help="Stats file for beaten+N or standalone attach")
@click.option("--date", help="Backdate completion (ISO date/datetime, for beaten+N)")
@click.option("-b", "beaten_target", help="Target completion by timestamp (for --stats-file attach)")
def modify(
    args: tuple[str, ...],
    yes: bool,
    dry_run: bool,
    link_path: str | None,
    add_files: tuple[str, ...],
    remove_files: tuple[str, ...],
    notes: str | None,
    stats_file: str | None,
    date: str | None,
    beaten_target: str | None,
):
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
    Completion tracking:
      caco modify id:1 beaten+1                       # Add a completion
      caco modify id:1 beaten+1 --notes "UV max"      # With notes
      caco modify id:1 beaten+1 --date 2024-06-15     # Backdated
      caco modify id:1 beaten+1 -s stats.txt          # With stats file
      caco modify id:1 beaten-1                        # Remove most recent
      caco modify id:1 beaten-2024-06-15T18:30:00      # Remove by timestamp
      caco modify id:1 beaten=5                        # Set exact count
      caco modify id:1 -s stats.txt                    # Attach stats to most recent
      caco modify id:1 -s stats.txt -b 2024-06-15     # Attach to specific

    \b
    Link a file:
      caco modify id:1 --link ~/Downloads/wad.wad

    \b
    Modifiable fields: title, author, year, description, status, rating,
      notes, iwad, sourceport, args, complevel, config, idgames-id, version, tag
    """
    from caco.config import get_link_mode
    from caco.wad_stats import parse_stats_file, stats_to_json, stats_from_json

    query_terms, actions, _sort = parse_modify_args(args)

    # Check for beaten actions in the parsed actions
    beaten_actions = [a for a in actions if a.action.startswith("beaten_")]

    # Validate flag combinations
    if notes and not any(a.action == "beaten_add" for a in beaten_actions):
        err_console.print("[red]--notes requires a beaten+N action[/red]")
        sys.exit(1)
    if date and not any(a.action == "beaten_add" for a in beaten_actions):
        err_console.print("[red]--date requires a beaten+N action[/red]")
        sys.exit(1)
    if beaten_target and beaten_actions:
        err_console.print("[red]-b cannot combine with beaten+/beaten-/beaten= actions[/red]")
        sys.exit(1)
    if beaten_target and not stats_file:
        err_console.print("[red]-b requires --stats-file[/red]")
        sys.exit(1)

    # Standalone --stats-file attach (no beaten action) counts as a modification
    standalone_attach = stats_file and not beaten_actions

    if not actions and not link_path and not add_files and not remove_files and not standalone_attach:
        err_console.print("[yellow]No modifications specified[/yellow]")
        err_console.print("[dim]Use field=value to set, !field to clear, tag=name to add tags, beaten+N to add completions[/dim]")
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
        elif action.action == "beaten_add":
            desc = f"beaten +{action.value}"
            if notes:
                desc += f" (notes: {notes})"
            if date:
                desc += f" (date: {date})"
            if stats_file:
                desc += f" (stats: {stats_file})"
            descriptions.append(desc)
        elif action.action == "beaten_remove":
            descriptions.append(f"beaten -{action.value}")
        elif action.action == "beaten_remove_ts":
            descriptions.append(f"beaten remove @ {action.value}")
        elif action.action == "beaten_set":
            descriptions.append(f"beaten ={action.value}")
    if link_path:
        descriptions.append(f"link file: {link_path}")
    for af in add_files:
        descriptions.append(f"add companion file: {af}")
    for rf in remove_files:
        descriptions.append(f"remove companion file: {rf}")
    if standalone_attach:
        target_desc = f" @ {beaten_target}" if beaten_target else " (most recent)"
        descriptions.append(f"attach stats: {stats_file}{target_desc}")

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

    # Parse stats file once if provided
    snapshot_json: str | None = None
    if stats_file:
        try:
            wad_stats_parsed = parse_stats_file(stats_file)
            snapshot_json = stats_to_json(wad_stats_parsed)
            played = wad_stats_parsed.played_maps
            console.print(
                f"[dim]Parsed {wad_stats_parsed.format}: "
                f"{len(played)} map(s) played, "
                f"total time {wad_stats_parsed.total_time_display}[/dim]"
            )
        except (ValueError, OSError) as e:
            err_console.print(f"[red]Failed to parse stats file: {e}[/red]")
            sys.exit(1)

    # Apply modifications
    for wad in wads:
        updates: dict[str, Any] = {}

        for action in actions:
            if action.action == "set":
                value: Any = action.value
                if action.field == "status":
                    value = db.Status(value)
                elif action.field in ("rating", "year", "complevel"):
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

            elif action.action == "beaten_add":
                n = int(action.value)
                for i in range(n):
                    comp_snapshot = None
                    comp_notes = None
                    comp_date = None
                    if i == 0:
                        comp_notes = notes
                        comp_date = date
                        if snapshot_json:
                            comp_snapshot = snapshot_json
                        elif wad.get("stats_snapshot"):
                            comp_snapshot = wad["stats_snapshot"]
                            ws = stats_from_json(comp_snapshot)
                            console.print(
                                f"[dim]Auto-attaching stats: {len(ws.played_maps)} map(s) played, "
                                f"total time {ws.total_time_display}[/dim]"
                            )
                    else:
                        comp_notes = "Manually added"
                    db.add_wad_completion(
                        wad["id"],
                        stats_snapshot=comp_snapshot,
                        notes=comp_notes,
                        completed_at=comp_date,
                    )
                count = db.get_times_beaten(wad["id"])
                msg = f"[green]Added {n} completion(s) for {wad['title']}[/green] (now beaten {count} time(s))"
                if snapshot_json or (n == 1 and wad.get("stats_snapshot")):
                    msg += " [dim](with stats)[/dim]"
                console.print(msg)

            elif action.action == "beaten_remove":
                n = int(action.value)
                completions = db.get_wad_completions(wad["id"])
                if not completions:
                    console.print(f"[dim]{wad['title']} has no completion records[/dim]")
                    continue
                to_remove = completions[:n]  # Already sorted DESC
                for c in to_remove:
                    db.delete_wad_completion(c["id"])
                count = db.get_times_beaten(wad["id"])
                console.print(
                    f"[green]Removed {len(to_remove)} completion(s) from {wad['title']}[/green] "
                    f"(now beaten {count} time(s))"
                )

            elif action.action == "beaten_remove_ts":
                if db.delete_wad_completion_by_timestamp(wad["id"], action.value):
                    count = db.get_times_beaten(wad["id"])
                    console.print(
                        f"[green]Removed completion @ {action.value} from {wad['title']}[/green] "
                        f"(now beaten {count} time(s))"
                    )
                else:
                    err_console.print(
                        f"[red]No completion at {action.value} for {wad['title']}[/red]"
                    )

            elif action.action == "beaten_set":
                n = int(action.value)
                db.set_wad_completion_count(wad["id"], n)
                console.print(f"[green]Set {wad['title']} to {n} completion(s)[/green]")

        if updates:
            # Skip auto-completion when beaten actions already handle it
            skip_auto = bool(beaten_actions)
            db.update_wad(wad["id"], record_completion=not skip_auto, **updates)

        # Standalone --stats-file attach
        if standalone_attach and snapshot_json:
            if beaten_target:
                comp = db.find_completion_by_timestamp(wad["id"], beaten_target)
                if not comp:
                    err_console.print(f"[red]No completion matching '{beaten_target}' for {wad['title']}[/red]")
                    continue
            else:
                completions = db.get_wad_completions(wad["id"])
                if not completions:
                    err_console.print(f"[dim]{wad['title']} has no completion records[/dim]")
                    continue
                comp = completions[0]

            db.update_wad_completion(comp["id"], stats_snapshot=snapshot_json)
            comp_date = comp["completed_at"][:16].replace("T", " ") if comp.get("completed_at") else "-"
            console.print(f"[green]Attached stats to completion ({comp_date}) for {wad['title']}[/green]")

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
                shutil.copy2(str(source), str(dest))
            except OSError as e:
                err_console.print(f"[red]Failed to copy file: {e}[/red]")
                sys.exit(1)

            db.update_wad(wad["id"], cached_path=str(dest), filename=source.name)

        # Remove original after all copies succeed (move semantics)
        if link_mode == "move":
            try:
                source.unlink()
            except OSError:
                pass

    # Handle --add-file / --remove-file
    if add_files or remove_files:
        from caco.services.companion_service import register_companion, unregister_companion

        for wad in wads:
            for af in add_files:
                register_companion(af, wad["id"])

            for rf in remove_files:
                rf_basename = Path(rf).name
                comp = db.get_wad_companion_by_filename(wad["id"], rf_basename)
                if not comp:
                    # Try exact filename match
                    comp = db.get_wad_companion_by_filename(wad["id"], rf)
                if comp:
                    unregister_companion(wad["id"], comp["id"], orphan_policy="keep")
                else:
                    err_console.print(f"[yellow]Warning: no companion '{rf}' found for {wad['title']}[/yellow]")

    # Print generic "Modified" for non-beaten actions
    non_beaten_actions = [a for a in actions if not a.action.startswith("beaten_")]
    if non_beaten_actions or link_path or add_files or remove_files:
        console.print(f"[green]Modified {len(wads)} WAD(s)[/green]")




@cli.command()
@click.argument("args", nargs=-1)
@click.option("--list", "list_trash", is_flag=True, help="Show trashed WADs")
@click.option("--purge", is_flag=True, help="Permanently delete (no query = purge all trash)")
@click.option("--restore", "restore_flag", is_flag=True, help="Restore from trash")
@click.option("--iwad", "iwad_target", type=str, help="Remove IWAD (FAMILY or FAMILY/VARIANT)")
@click.option("--id24", "id24_target", type=str, help="Remove id24 WAD by name")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
@click.option("--dry-run", is_flag=True, help="Show what would happen without making changes")
@click.option("--output", "-o", type=click.Choice(["json", "plain"]), help="Output format (with --list)")
def trash(
    args: tuple[str, ...],
    list_trash: bool,
    purge: bool,
    restore_flag: bool,
    iwad_target: str | None,
    id24_target: str | None,
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
      caco trash --id24 id1            # Remove id24 WAD
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

    # --id24 mode: remove id24 WAD
    if id24_target:
        from caco.db._id24 import remove_id24_with_paths

        id24_dir = get_id24_dir()
        paths = remove_id24_with_paths(id24_target)
        if paths:
            if not dry_run:
                _delete_managed_files(paths, id24_dir)
            console.print(f"[green]Removed id24:[/green] {id24_target}")
        else:
            err_console.print(f"[red]id24 WAD '{id24_target}' not found[/red]")
            sys.exit(1)
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

    Supports the same query syntax as 'caco ls' for filtering.

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


@cli.command()
@click.argument("query", nargs=-1, shell_complete=_complete_query)
@click.option("--complevel", is_flag=True, help="Only enrich WADs with missing complevel")
@click.option("--dry-run", is_flag=True, help="Preview changes without applying them")
def enrich(query: tuple[str, ...], complevel: bool, dry_run: bool):
    """Re-run enrichment for existing WADs.

    Detects complevel from cached WAD files and Doom Wiki port field.

    \b
    Examples:
        caco enrich                    # Enrich all WADs
        caco enrich --complevel        # Only WADs missing complevel
        caco enrich --dry-run          # Preview what would change
        caco enrich status:playing     # Enrich only playing WADs
    """
    from caco.complevel import complevel_name
    from caco.complevel_detect import detect_complevel
    from caco.services.import_service import ImportService

    query_str = " ".join(query) if query else None
    wads = db.search_wads(query=query_str)
    if not wads:
        err_console.print("[red]No WADs found[/red]")
        sys.exit(1)

    # Filter to WADs missing complevel if --complevel flag is set
    if complevel:
        wads = [w for w in wads if w.get("complevel") is None]
        if not wads:
            console.print("[dim]All matching WADs already have complevel set[/dim]")
            return

    console.print(f"[dim]Enriching {len(wads)} WAD(s)...[/dim]")

    enriched: list[tuple[dict, int]] = []  # (wad, new_complevel)
    wiki_lookups = 0

    for wad in wads:
        # Skip if complevel already set (unless not filtering by --complevel)
        if wad.get("complevel") is not None:
            continue

        detected_cl: int | None = None

        # 1. Try file-based detection if cached file exists
        cached_path = wad.get("cached_path")
        if cached_path and Path(cached_path).exists():
            detected_cl = detect_complevel(cached_path)
            if detected_cl is not None:
                enriched.append((wad, detected_cl))
                if not dry_run:
                    db.update_wad(wad["id"], complevel=detected_cl)
                continue

        # 2. Try Doom Wiki lookup for port field
        title = wad.get("title", "")
        if title:
            try:
                from caco.doomwiki import DoomwikiClient

                client = DoomwikiClient()
                results = client.search_wads(title, limit=5)
                wiki_lookups += 1

                if results:
                    from caco.services.import_service import _titles_match

                    for r in results:
                        if _titles_match(title, r.display_name):
                            if r.port:
                                ImportService._auto_link_complevel(wad["id"] if not dry_run else -1, r.port)
                                # Re-check if it was set (only works when not dry_run)
                                if not dry_run:
                                    updated = db.get_wad(wad["id"])
                                    if updated and updated.get("complevel") is not None:
                                        enriched.append((wad, updated["complevel"]))
                                else:
                                    # For dry run, compute what would be set
                                    detected_cl = _port_to_complevel(r.port)
                                    if detected_cl is not None:
                                        enriched.append((wad, detected_cl))
                            break
            except Exception:
                logger.debug("Wiki lookup failed for %s", title, exc_info=True)

    # Summary
    if not enriched:
        console.print("[dim]No new complevels detected[/dim]")
        if wiki_lookups:
            console.print(f"[dim]({wiki_lookups} Doom Wiki lookup(s) performed)[/dim]")
        return

    prefix = "[bold]Would set[/bold]" if dry_run else "[bold]Set[/bold]"

    for wad, cl in enriched:
        cl_name = complevel_name(cl)
        console.print(f"  {prefix} [cyan]{wad['title']}[/cyan] -> {cl} ({cl_name})")

    suffix = " [dim](dry run)[/dim]" if dry_run else ""
    console.print(
        f"\n[green]{len(enriched)}/{len(wads)} WAD(s) enriched[/green]"
        f"{suffix}"
    )
    if wiki_lookups:
        console.print(f"[dim]({wiki_lookups} Doom Wiki lookup(s) performed)[/dim]")


def _port_to_complevel(port_text: str) -> int | None:
    """Map Doom Wiki port field text to a complevel (same logic as ImportService)."""
    mapping = {
        "boom": 9,
        "mbf21": 21,
        "mbf": 11,
        "vanilla": 2,
        "limit-removing": 2,
        "limit removing": 2,
    }
    text = port_text.lower().strip()
    for key, cl in mapping.items():
        if key in text:
            return cl
    return None

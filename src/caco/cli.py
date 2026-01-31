"""Command-line interface for caco."""

import shutil
import subprocess
import sys
from pathlib import Path

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
    get_list_config,
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


def _interactive_pick(
    wads: list[dict],
    prompt: str = "Select WAD",
    multi: bool = False,
) -> list[dict] | None:
    """
    Interactive picker for selecting WAD(s) from a list.

    Uses fzf if available and stdout is TTY, otherwise falls back to numbered selection.

    Args:
        wads: List of WAD dicts to choose from
        prompt: Prompt to display
        multi: Allow multiple selections

    Returns:
        List of selected WAD dicts, or None if cancelled.
    """
    if not wads:
        return None

    # Format items for display: "[ID] Title (Author, Year)"
    def format_wad(w: dict) -> str:
        author = w.get("author") or "Unknown"
        year = w.get("year") or "????"
        return f"[{w['id']}] {w['title']} ({author}, {year})"

    items = [format_wad(w) for w in wads]

    # Try fzf if available and TTY
    if _fzf_available() and sys.stdout.isatty():
        selected_indices = _fzf_select(items, prompt=prompt, multi=multi)
        if selected_indices is None:
            return None
        return [wads[i] for i in selected_indices]

    # Fallback to numbered selection
    console.print(f"\n[bold]{prompt}:[/bold]")
    for i, item in enumerate(items[:20], 1):
        console.print(f"  [{i}] {item}")
    if len(items) > 20:
        console.print(f"  [dim]... and {len(items) - 20} more[/dim]")

    if multi:
        console.print("[dim]Enter numbers separated by commas (e.g., 1,3,5) or 0 to cancel[/dim]")
        try:
            choice = click.prompt("Select", default="0")
            if choice == "0":
                return None
            indices = [int(x.strip()) - 1 for x in choice.split(",")]
            selected = [wads[i] for i in indices if 0 <= i < len(wads)]
            return selected if selected else None
        except (ValueError, IndexError):
            return None
    else:
        try:
            choice = click.prompt("Select [1-{}]".format(min(len(wads), 20)), type=int, default=0)
            if choice == 0 or choice > len(wads):
                return None
            return [wads[choice - 1]]
        except (ValueError, click.Abort):
            return None


def resolve_wad_query(
    query: str,
    mode: str = "error",
    yes: bool = False,
) -> list[dict] | None:
    """Resolve WAD ID, ID range, or query string to WAD(s).

    Args:
        query: WAD ID, ID range (3-6,9), or query string (filename:tnto)
        mode: How to handle multiple matches:
            - "error": Error if multiple matches (default, backward compat)
            - "single" or "pick": Use interactive picker if multiple
            - "multiple": Allow multiple with confirmation
        yes: If True, skip confirmation prompts (selects first for single mode)

    Returns:
        List of WAD dicts, or None if cancelled/no matches.
        Exits with error if mode="error" and multiple found.
    """
    # Normalize mode aliases
    if mode == "single":
        mode = "pick"

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

    # Multiple matches - handle based on mode
    if mode == "error":
        err_console.print(f"[red]Multiple WADs match '{query}':[/red]")
        for r in results[:10]:
            err_console.print(f"  {r['id']}: {r['title']}")
        if len(results) > 10:
            err_console.print(f"  ... and {len(results) - 10} more")
        sys.exit(1)

    if mode == "pick":
        # Interactive picker for single selection
        if yes:
            # Auto-select first match for scripting
            return [results[0]]
        return _interactive_pick(results, prompt="Select WAD", multi=False)

    # mode == "multiple": confirm unless yes
    if not yes:
        console.print(f"[yellow]This will affect {len(results)} WAD(s):[/yellow]")
        for r in results[:10]:
            console.print(f"  {r['id']}: {r['title']}")
        if len(results) > 10:
            console.print(f"  ... and {len(results) - 10} more")
        if not click.confirm("Continue?"):
            return None

    return results


SORT_FIELDS = ["id", "playtime", "rating", "created", "title", "author", "last_played", "year"]

# Status shortcuts: single letters and common abbreviations
STATUS_SHORTCUTS = {
    "t": "to-play", "toplay": "to-play", "tp": "to-play",
    "b": "backlog", "back": "backlog",
    "p": "playing", "play": "playing",
    "f": "finished", "fin": "finished", "done": "finished",
    "a": "abandoned", "drop": "abandoned", "dropped": "abandoned",
}


def _normalize_status(value: str | None) -> str | None:
    """Normalize status value, expanding shortcuts."""
    if value is None:
        return None
    lower = value.lower()
    # Check if it's a shortcut
    if lower in STATUS_SHORTCUTS:
        return STATUS_SHORTCUTS[lower]
    # Check if it's already a valid status
    try:
        db.Status(lower)
        return lower
    except ValueError:
        # Return as-is, let Click's Choice handle the error
        return value


class StatusChoice(click.Choice):
    """A Click Choice that accepts status shortcuts."""

    def __init__(self):
        super().__init__([s.value for s in db.Status], case_sensitive=False)
        self.shortcuts = STATUS_SHORTCUTS

    def convert(self, value, param, ctx):
        if value is None:
            return None
        lower = value.lower()
        # Expand shortcut if present
        if lower in self.shortcuts:
            value = self.shortcuts[lower]
        return super().convert(value, param, ctx)

    def get_metavar(self, param, ctx=None):
        # Show both full values and shortcuts in help
        return "[STATUS]"


def _complete_tags(ctx, param, incomplete):
    """Shell completion function for tag names."""
    try:
        tags = db.get_all_tags()
        return [t for t in tags if t.lower().startswith(incomplete.lower())]
    except Exception:
        return []


# Query field prefixes for completion
QUERY_FIELDS = ["id:", "title:", "name:", "author:", "year:", "filename:", "tag:", "status:", "source:"]

# Valid status values for completion
QUERY_STATUS_VALUES = ["to-play", "backlog", "playing", "finished", "abandoned"]

# Valid source values for completion
QUERY_SOURCE_VALUES = ["idgames", "doomwiki", "doomworld", "url", "local"]


def _complete_query(ctx, param, incomplete: str) -> list[str]:
    """Shell completion for query arguments.

    Completes:
    - Field prefixes: title:, author:, status:, etc.
    - Status values: status:playing, status:to-play, etc.
    - Source values: source:idgames, source:doomwiki, etc.
    - Tag values: tag:megawad, tag:cacoward, etc.
    - Negated versions: ^status:, ^tag:, etc. (^ avoids CLI option parsing issues)
    """
    completions = []

    # Check if we're completing a negation (- or ^ prefix)
    # ^ is preferred to avoid CLI option parsing issues
    negated = incomplete.startswith("-") or incomplete.startswith("^")
    if negated:
        search_text = incomplete[1:]
        prefix = incomplete[0]  # Preserve the original prefix
    else:
        search_text = incomplete
        prefix = ""

    # If no colon yet, suggest field prefixes
    if ":" not in search_text:
        for field in QUERY_FIELDS:
            if field.startswith(search_text.lower()):
                completions.append(f"{prefix}{field}")
        return completions

    # Field:value completion
    field, _, partial_value = search_text.partition(":")
    field = field.lower()

    if field == "status":
        for status in QUERY_STATUS_VALUES:
            if status.startswith(partial_value.lower()):
                completions.append(f"{prefix}status:{status}")

    elif field == "source":
        for source in QUERY_SOURCE_VALUES:
            if source.startswith(partial_value.lower()):
                completions.append(f"{prefix}source:{source}")

    elif field == "tag":
        try:
            tags = db.get_all_tags()
            for tag in tags:
                if tag.startswith(partial_value.lower()):
                    completions.append(f"{prefix}tag:{tag}")
        except Exception:
            pass

    return completions


def _parse_sort_option(sort: str | None) -> tuple[str | None, bool]:
    """Parse sort option. Returns (field, descending).

    Supports suffix notation (like beets) to avoid CLI flag conflicts:
        'playtime' -> ('playtime', True)   # Default (desc for numeric/date)
        'title+' -> ('title', False)       # Explicit ascending
        'title-' -> ('title', True)        # Explicit descending
        '-title' -> ('title', False)       # Legacy prefix (still works)
    """
    if not sort:
        return None, True

    # Suffix notation (preferred - avoids CLI flag issues)
    if sort.endswith("+"):
        return sort[:-1], False  # Ascending
    if sort.endswith("-"):
        return sort[:-1], True   # Descending

    # Legacy prefix notation (still supported)
    if sort.startswith("-"):
        return sort[1:], False
    if sort.startswith("+"):
        return sort[1:], True

    return sort, True


def _complete_sort(ctx, param, incomplete: str) -> list[str]:
    """Shell completion for sort fields.

    Completes field names and suggests +/- suffix for direction.
    """
    completions = []

    # Check for direction suffix
    if incomplete.endswith("+") or incomplete.endswith("-"):
        # Already has direction, complete the field part
        field_part = incomplete[:-1]
        suffix = incomplete[-1]
        for field in SORT_FIELDS:
            if field.startswith(field_part):
                completions.append(f"{field}{suffix}")
    else:
        # Complete field names, offer with +/- variants
        for field in SORT_FIELDS:
            if field.startswith(incomplete):
                completions.append(field)      # Default direction
                completions.append(f"{field}+")  # Ascending
                completions.append(f"{field}-")  # Descending

    return completions


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


def _render_wad_list(wads: list[dict], title: str | None = None, list_config: dict | None = None) -> None:
    """Render a list of WADs as a table.

    Args:
        wads: List of WAD dicts to display
        title: Optional table title
        list_config: Optional config dict with 'format' and 'colors' keys
    """
    if not wads:
        console.print("[dim]No WADs found[/dim]")
        return

    # Get list config
    if list_config is None:
        list_config = get_list_config()

    columns = list_config.get("format", ["id", "title", "author", "status", "maps", "beaten", "playtime", "last_played"])
    colors = list_config.get("colors", {})

    # Batch fetch stats for all WADs
    wad_ids = [w["id"] for w in wads]
    maps_completed = db.get_maps_completed_batch(wad_ids)
    times_beaten = db.get_times_beaten_batch(wad_ids)

    # Column definitions: name -> (header, style, justify)
    column_defs = {
        "id": ("ID", "dim", None),
        "title": ("Title", "cyan", None),
        "author": ("Author", None, None),
        "year": ("Year", "dim", "right"),
        "status": ("Status", None, None),
        "rating": ("Rating", None, "center"),
        "maps": ("Maps", None, "right"),
        "beaten": ("Beaten", None, "right"),
        "playtime": ("Playtime", None, "right"),
        "last_played": ("Last Played", "dim", None),
        "tags": ("Tags", "dim", None),
        "source": ("Source", "dim", None),
        "filename": ("Filename", "dim", None),
    }

    table = Table(title=title or f"Library ({len(wads)} WADs)")

    # Add columns based on config
    for col in columns:
        if col in column_defs:
            header, style, justify = column_defs[col]
            table.add_column(header, style=style, justify=justify)

    for wad in wads:
        # Pre-compute values that might be needed
        playtime = db.get_total_playtime(wad["id"])
        last_played = db.get_last_played(wad["id"])

        # Build row values based on columns
        row_values = []
        for col in columns:
            if col == "id":
                row_values.append(str(wad["id"]))
            elif col == "title":
                row_values.append(wad["title"])
            elif col == "author":
                row_values.append(wad["author"] or "-")
            elif col == "year":
                row_values.append(str(wad["year"]) if wad.get("year") else "-")
            elif col == "status":
                status = wad["status"]
                status_color = colors.get(status, "")
                if status_color:
                    row_values.append(f"[{status_color}]{status}[/{status_color}]")
                else:
                    row_values.append(status)
            elif col == "rating":
                if wad.get("rating"):
                    row_values.append("★" * wad["rating"] + "☆" * (5 - wad["rating"]))
                else:
                    row_values.append("-")
            elif col == "maps":
                count = maps_completed.get(wad["id"], 0)
                row_values.append(str(count) if count else "-")
            elif col == "beaten":
                count = times_beaten.get(wad["id"], 0)
                row_values.append(str(count) if count else "-")
            elif col == "playtime":
                row_values.append(format_duration(playtime) if playtime else "-")
            elif col == "last_played":
                row_values.append(last_played[:10] if last_played else "-")
            elif col == "tags":
                tags = wad.get("tags", [])
                if tags:
                    if len(tags) > 3:
                        row_values.append(", ".join(tags[:3]) + f" +{len(tags) - 3}")
                    else:
                        row_values.append(", ".join(tags))
                else:
                    row_values.append("-")
            elif col == "source":
                row_values.append(wad.get("source_type", "-"))
            elif col == "filename":
                row_values.append(wad.get("filename", "-") or "-")
            else:
                row_values.append("-")

        table.add_row(*row_values)

    console.print(table)


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
@click.option("--status", "-s", type=StatusChoice())
@click.option("--rating", "-r", type=click.IntRange(1, 5))
@click.option("--notes", "-n")
@click.option("--iwad", help="Custom IWAD path for this WAD")
@click.option("--clear-iwad", is_flag=True, help="Clear custom IWAD")
@click.option("--sourceport", help="Custom sourceport for this WAD")
@click.option("--clear-sourceport", is_flag=True, help="Clear custom sourceport")
@click.option("--args", "custom_args", help="Custom arguments (JSON array or space-separated)")
@click.option("--clear-args", is_flag=True, help="Clear custom arguments")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
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
    dry_run: bool,
):
    """Update WAD metadata. QUERY: ID, ID range (3-6,9), or query (tag:megawad)."""
    import json

    updates = {}
    update_descriptions = []

    if status:
        updates["status"] = db.Status(status)
        update_descriptions.append(f"status → {status}")
    if rating:
        updates["rating"] = rating
        update_descriptions.append(f"rating → {'★' * rating}")
    if notes:
        updates["notes"] = notes
        update_descriptions.append(f"notes → \"{notes[:30]}{'...' if len(notes) > 30 else ''}\"")

    # Per-WAD play config
    if iwad:
        updates["custom_iwad"] = iwad
        update_descriptions.append(f"custom_iwad → {iwad}")
    elif clear_iwad:
        updates["custom_iwad"] = None
        update_descriptions.append("custom_iwad → (cleared)")
    if sourceport:
        updates["custom_sourceport"] = sourceport
        update_descriptions.append(f"custom_sourceport → {sourceport}")
    elif clear_sourceport:
        updates["custom_sourceport"] = None
        update_descriptions.append("custom_sourceport → (cleared)")
    if custom_args:
        # Accept JSON array or space-separated string
        try:
            args_list = json.loads(custom_args)
        except json.JSONDecodeError:
            args_list = custom_args.split()
        updates["custom_args"] = json.dumps(args_list)
        update_descriptions.append(f"custom_args → {args_list}")
    elif clear_args:
        updates["custom_args"] = None
        update_descriptions.append("custom_args → (cleared)")

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
            console.print(f"  • {desc}")
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
    total_completions = 0
    total_sessions = 0
    total_playtime = 0

    action = "permanently deleted" if purge else "moved to trash"
    console.print(f"\n[bold]The following WADs will be {action}:[/bold]\n")
    for wad in wads:
        stats = db.get_wad_stats(wad["id"])
        total_completions += stats["map_completions"]
        total_sessions += stats["session_count"]
        total_playtime += stats["total_playtime"]

        # Format WAD info
        author_year = []
        if wad.get("author"):
            author_year.append(wad["author"])
        if wad.get("year"):
            author_year.append(str(wad["year"]))
        info = f" ({', '.join(author_year)})" if author_year else ""

        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}{info}")

    # Show associated data that will be deleted (only for purge)
    if purge and (total_completions or total_sessions):
        console.print(f"\n[dim]This will also delete:[/dim]")
        if total_completions:
            console.print(f"  • {total_completions} map completion record(s)")
        if total_sessions:
            playtime_str = format_duration(total_playtime) if total_playtime else "0s"
            console.print(f"  • {total_sessions} play session(s) ({playtime_str})")

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
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
def tag_add(query: str, tags: tuple[str, ...], yes: bool, dry_run: bool):
    """Add tags to WAD(s). QUERY: ID, ID range (3-6,9), or query (author:romero)."""
    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return  # User cancelled

    if dry_run:
        console.print(f"\n[bold]Would add tag(s) {', '.join(tags)} to {len(wads)} WAD(s):[/bold]\n")
        for wad in wads[:10]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(wads) > 10:
            console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    for wad in wads:
        for t in tags:
            db.add_tag(wad["id"], t)

    console.print(f"[green]Added tag(s) to {len(wads)} WAD(s)[/green]")


@tag.command(name="remove")
@click.argument("query")
@click.argument("tags", nargs=-1, required=True)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
def tag_remove(query: str, tags: tuple[str, ...], yes: bool, dry_run: bool):
    """Remove tags from WAD(s). QUERY: ID, ID range (3-6,9), or query (author:romero)."""
    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return  # User cancelled

    if dry_run:
        console.print(f"\n[bold]Would remove tag(s) {', '.join(tags)} from {len(wads)} WAD(s):[/bold]\n")
        for wad in wads[:10]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(wads) > 10:
            console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

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


@cli.group(name="import", invoke_without_command=True)
@click.pass_context
def import_cmd(ctx):
    """Import WADs from various sources.

    Without a subcommand, shows help. Use 'caco add <source>' for auto-detection.
    """
    if ctx.invoked_subcommand is None:
        click.echo(ctx.get_help())


def _detect_source_type(source: str) -> str:
    """Detect the type of import source.

    Returns: 'doomwiki_url', 'doomworld_url', 'url', 'local', 'idgames_id', or 'idgames_search'
    """
    from pathlib import Path

    # URL detection - check for specific sites first
    if source.startswith(("http://", "https://")):
        if "doomwiki.org/wiki/" in source:
            return "doomwiki_url"
        if "doomworld.com/forum/topic/" in source:
            return "doomworld_url"
        return "url"

    # Local file detection (check if path exists)
    if Path(source).exists():
        return "local"

    # idgames ID detection (numeric)
    if source.isdigit():
        return "idgames_id"

    # Default to idgames search
    return "idgames_search"


def _infer_title_from_filename(filename: str) -> str:
    """Infer a reasonable title from a filename."""
    from pathlib import Path

    # Get base name without extension
    name = Path(filename).stem

    # Replace underscores and hyphens with spaces
    name = name.replace("_", " ").replace("-", " ")

    # Title case
    return name.title()


def _infer_title_from_url(url: str) -> str:
    """Infer a title from a URL by extracting the filename."""
    from urllib.parse import urlparse, unquote

    parsed = urlparse(url)
    path = unquote(parsed.path)

    # Get the filename part
    if "/" in path:
        filename = path.split("/")[-1]
    else:
        filename = path

    return _infer_title_from_filename(filename)


@import_cmd.command(name="auto")
@click.argument("source")
@click.option("--title", "-t", help="Override title (inferred from filename if not provided)")
@click.option("--author", "-a", help="Author name")
@click.option("--year", "-y", type=int, help="Year released")
@click.option("--tag", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow multi-select for idgames search (requires fzf)")
def import_auto(source: str, title: str | None, author: str | None, year: int | None,
                tags: tuple[str, ...], force: bool, multi: bool):
    """Smart import that auto-detects source type.

    SOURCE can be:
    - A Doomwiki URL (doomwiki.org/wiki/...) - imports from Doom Wiki
    - A Doomworld forum URL (doomworld.com/forum/topic/...) - imports from forum
    - A URL (http/https) - imports from URL
    - A local file path - imports from local filesystem
    - A number - looks up idgames file ID
    - Text - searches idgames archive

    \b
    Examples:
        caco import auto ~/Downloads/mymap.wad
        caco import auto https://doomwiki.org/wiki/Scythe
        caco import auto https://www.doomworld.com/forum/topic/134292-myhousewad/
        caco import auto https://example.com/map.zip
        caco import auto 12345
        caco import auto "scythe 2"
    """
    from pathlib import Path

    source_type = _detect_source_type(source)

    if source_type == "doomwiki_url":
        # Doomwiki URL import
        from caco.sources.doomwiki import DoomwikiSource
        from urllib.parse import urlparse, unquote

        # Extract page title from URL: https://doomwiki.org/wiki/Page_Title
        parsed = urlparse(source)
        path = unquote(parsed.path)  # Handle URL-encoded chars like %3A for :
        if path.startswith("/wiki/"):
            page_title = path[6:].replace("_", " ")  # Remove /wiki/ prefix
        else:
            page_title = path.split("/")[-1].replace("_", " ")

        with DoomwikiSource() as wiki:
            entry = wiki.get(page_title)
            if not entry:
                err_console.print(f"[red]Page not found:[/red] {page_title}")
                return

            existing = db.find_duplicate(
                db.SourceType.DOOMWIKI,
                source_id=str(entry.page_id),
            )
            if existing and not force:
                console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
                console.print("[dim]Use --force to import anyway[/dim]")
                return

            wad_id = wiki.import_wad(entry, tags=list(tags) if tags else None)
            console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")

    elif source_type == "url":
        # URL import - infer title if not provided
        inferred_title = title or _infer_title_from_url(source)

        existing = db.find_duplicate(
            db.SourceType.URL,
            source_url=source,
            filename=inferred_title,
            author=author,
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return

        wad_id = db.add_wad(
            title=inferred_title,
            source_type=db.SourceType.URL,
            source_url=source,
            author=author,
            year=year,
            tags=list(tags) if tags else None,
        )
        console.print(f"[green]Added:[/green] {inferred_title} (ID: {wad_id})")

    elif source_type == "local":
        # Local file import
        p = Path(source).resolve()
        inferred_title = title or _infer_title_from_filename(p.name)

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
            title=inferred_title,
            source_type=db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            cached_path=str(p),
            author=author,
            year=year,
            tags=list(tags) if tags else None,
        )
        console.print(f"[green]Added:[/green] {inferred_title} (ID: {wad_id})")

    elif source_type == "doomworld_url":
        # Doomworld forum URL import
        from caco.sources.doomworld import DoomworldSource
        from caco.doomworld import (
            DoomworldError,
            complevel_name,
            iwad_display_name,
            sourceport_display_name,
        )

        with DoomworldSource() as doomworld:
            try:
                thread = doomworld.get(source)
            except DoomworldError as e:
                err_console.print(f"[red]Error: {e}[/red]")
                return

            if not thread:
                err_console.print(f"[red]Thread not found:[/red] {source}")
                return

            existing = db.find_duplicate(
                db.SourceType.DOOMWORLD,
                source_id=str(thread.thread_id),
                source_url=thread.thread_url,
            )
            if existing and not force:
                console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
                console.print("[dim]Use --force to import anyway[/dim]")
                return

            wad_id = doomworld.import_wad(
                thread,
                tags=list(tags) if tags else None,
                title=title,
                author=author,
                year=year,
            )
            console.print(f"[green]Imported:[/green] {thread.title} (ID: {wad_id})")

            # Show technical metadata (Phase 2)
            if thread.has_technical_info:
                if thread.iwad:
                    console.print(f"  [dim]IWAD:[/dim] {iwad_display_name(thread.iwad)}")
                if thread.sourceport:
                    console.print(f"  [dim]Port:[/dim] {sourceport_display_name(thread.sourceport)}")
                if thread.complevel is not None:
                    console.print(f"  [dim]Complevel:[/dim] {complevel_name(thread.complevel)}")
                if thread.download_links:
                    console.print(f"  [dim]Downloads:[/dim] {len(thread.download_links)} link(s)")

    elif source_type == "idgames_id":
        # idgames ID lookup
        from caco.sources.idgames import IdgamesSource

        with IdgamesSource() as idgames:
            try:
                entry = idgames.get(int(source))
            except Exception as e:
                err_console.print(f"[red]Failed to fetch idgames ID {source}: {e}[/red]")
                return

            existing = db.find_duplicate(
                db.SourceType.IDGAMES,
                source_id=str(entry.id),
                filename=entry.filename,
                author=entry.author,
            )
            if existing and not force:
                console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
                console.print("[dim]Use --force to import anyway[/dim]")
                return

            wad_id = idgames.import_wad(entry, tags=list(tags) if tags else None)
            console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")

    else:
        # idgames search - delegate to existing command's logic
        from caco.sources.idgames import IdgamesSource

        def _check_and_import(entry, tags_list):
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
            return idgames.import_wad(entry, tags=tags_list)

        with IdgamesSource() as idgames:
            results = idgames.search(source)
            if not results:
                console.print("[dim]No results found[/dim]")
                return

            if multi and not _fzf_available():
                err_console.print("[red]--multi requires fzf to be installed[/red]")
                sys.exit(1)

            if _fzf_available():
                fzf_items = []
                for entry in results[:50]:
                    entry_year = entry.date[:4] if entry.date else "????"
                    fzf_items.append(f"{entry.title} by {entry.author or 'Unknown'} ({entry_year})")

                selected_indices = _fzf_select(
                    fzf_items,
                    prompt="Select WAD(s)" if multi else "Select WAD",
                    multi=multi,
                )

                if selected_indices is None:
                    return

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


@import_cmd.command(name="idgames")
@click.argument("query_or_id")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
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


@import_cmd.command(name="doomwiki")
@click.argument("query_or_title")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow selecting multiple WADs (requires fzf)")
def import_doomwiki(query_or_title: str, tags: tuple[str, ...], force: bool, multi: bool):
    """Import a WAD from Doom Wiki.

    Searches the Doom Wiki (doomwiki.org) for WADs matching the query.
    Only pages with a {{Wad}} infobox template are shown.

    Use fzf for interactive selection (if installed). Use --multi for batch import.

    \b
    Examples:
        caco import doomwiki "Scythe"
        caco import doomwiki "Eviternity" --tag megawad
        caco import doomwiki --multi "cacoward"
    """
    from caco.sources.doomwiki import DoomwikiSource

    def _check_and_import(entry, tags_list):
        """Check for duplicates before importing. Returns wad_id or None if skipped."""
        existing = db.find_duplicate(
            db.SourceType.DOOMWIKI,
            source_id=str(entry.page_id),
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return None

        wad_id = source.import_wad(entry, tags=tags_list)
        return wad_id

    with DoomwikiSource() as source:
        # Try exact page title match first
        entry = source.get(query_or_title)
        if entry:
            wad_id = _check_and_import(entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")
            return

        # Fall back to search
        results = source.search(query_or_title)
        if not results:
            console.print("[dim]No WAD pages found (only pages with {{Wad}} infobox are shown)[/dim]")
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
                year = str(entry.year) if entry.year else "????"
                fzf_items.append(f"{entry.display_name} by {entry.author or 'Unknown'} ({year})")

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
                    console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")
                    imported += 1

            if multi and imported > 1:
                console.print(f"[green]Imported {imported} WAD(s)[/green]")

        else:
            # Fallback to numbered prompt
            table = Table(title="Search Results")
            table.add_column("#", style="dim")
            table.add_column("Title", style="cyan")
            table.add_column("Author")
            table.add_column("Year")

            for i, entry in enumerate(results[:20], 1):
                year = str(entry.year) if entry.year else "-"
                table.add_row(str(i), entry.display_name, entry.author or "-", year)

            console.print(table)

            choice = click.prompt("Enter number to import (or 0 to cancel)", type=int, default=0)
            if choice == 0 or choice > len(results):
                return

            entry = results[choice - 1]
            wad_id = _check_and_import(entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")


def _complete_llm_backends(ctx, param, incomplete):
    """Shell completion for LLM backends."""
    backends = ["claude-code", "openrouter", "anthropic", "openai"]
    return [b for b in backends if b.startswith(incomplete.lower())]


@import_cmd.command(name="doomworld")
@click.argument("url")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--title", help="Override parsed title")
@click.option("--author", "-a", help="Override parsed author")
@click.option("--year", "-y", type=int, help="Override parsed year")
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--smart", "-s", is_flag=True, help="Use LLM for intelligent metadata extraction")
@click.option("--llm-backend", type=click.Choice(["claude-code", "openrouter", "anthropic", "openai"]),
              help="LLM backend (auto-detects if not specified)", shell_complete=_complete_llm_backends)
@click.option("--llm-model", help="Model override for API backends (e.g., 'gpt-4' for openai)")
def import_doomworld(url: str, tags: tuple[str, ...], title: str | None,
                     author: str | None, year: int | None, force: bool,
                     smart: bool, llm_backend: str | None, llm_model: str | None):
    """Import a WAD from a Doomworld forum thread.

    Fetches metadata from the forum thread including title, author,
    date, and first post content. Use --smart for LLM-based extraction.

    \b
    Examples:
        caco import doomworld https://www.doomworld.com/forum/topic/134292-myhousewad/
        caco import doomworld URL --tag cacoward --tag megawad
        caco import doomworld URL --smart
        caco import doomworld URL --smart --llm-backend openrouter
    """
    from caco.sources.doomworld import DoomworldSource
    from caco.doomworld import (
        DoomworldError,
        complevel_name,
        iwad_display_name,
        sourceport_display_name,
    )

    # Validate URL
    if "doomworld.com/forum/topic/" not in url:
        err_console.print("[red]Invalid Doomworld forum URL[/red]")
        err_console.print("[dim]Expected: https://www.doomworld.com/forum/topic/{id}-{slug}/[/dim]")
        sys.exit(1)

    with DoomworldSource() as doomworld:
        try:
            thread = doomworld.get(url)
        except DoomworldError as e:
            err_console.print(f"[red]Error: {e}[/red]")
            sys.exit(1)

        if not thread:
            err_console.print(f"[red]Thread not found:[/red] {url}")
            sys.exit(1)

        # LLM-based extraction (Phase 3)
        llm_metadata = None
        if smart:
            from caco.doomworld.llm import get_parser, LLMError, LLMNotAvailableError

            try:
                parser = get_parser(backend=llm_backend, model=llm_model)
                console.print(f"[dim]Using LLM backend: {parser.name}[/dim]")

                with console.status("[bold blue]Extracting metadata with LLM..."):
                    llm_metadata = parser.parse(thread.first_post_text)

                console.print("[green]LLM extraction complete[/green]")

            except LLMNotAvailableError as e:
                err_console.print(f"[yellow]LLM not available:[/yellow] {e}")
                err_console.print("[dim]Falling back to regex extraction[/dim]")
            except LLMError as e:
                err_console.print(f"[yellow]LLM error:[/yellow] {e}")
                err_console.print("[dim]Falling back to regex extraction[/dim]")

        # Check for duplicates
        existing = db.find_duplicate(
            db.SourceType.DOOMWORLD,
            source_id=str(thread.thread_id),
            source_url=thread.thread_url,
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return

        # Merge LLM metadata with regex-extracted data (LLM takes precedence where available)
        final_title = title
        final_author = author
        final_iwad = thread.iwad
        final_sourceport = thread.sourceport
        final_complevel = thread.complevel

        if llm_metadata:
            if not final_title and llm_metadata.title:
                final_title = llm_metadata.title
            if not final_author and llm_metadata.author:
                final_author = llm_metadata.author
            if not final_iwad and llm_metadata.iwad:
                final_iwad = llm_metadata.iwad
            if not final_sourceport and llm_metadata.sourceport:
                final_sourceport = llm_metadata.sourceport
            if final_complevel is None and llm_metadata.complevel is not None:
                final_complevel = llm_metadata.complevel

        # Import with merged metadata
        wad_id = doomworld.import_wad(
            thread,
            tags=list(tags) if tags else None,
            title=final_title,
            author=final_author,
            year=year,
        )
        console.print(f"[green]Imported:[/green] {thread.title} (ID: {wad_id})")

        # Show parsed metadata
        if thread.author:
            console.print(f"  [dim]Author:[/dim] {thread.author}")
        if thread.posted_date:
            console.print(f"  [dim]Posted:[/dim] {thread.posted_date[:10]}")

        # Show technical metadata (Phase 2 regex + Phase 3 LLM)
        display_iwad = final_iwad or thread.iwad
        display_port = final_sourceport or thread.sourceport
        display_complevel = final_complevel if final_complevel is not None else thread.complevel

        if display_iwad or display_port or display_complevel is not None or thread.download_links:
            if display_iwad:
                console.print(f"  [dim]IWAD:[/dim] {iwad_display_name(display_iwad)}")
            if display_port:
                console.print(f"  [dim]Port:[/dim] {sourceport_display_name(display_port)}")
            if display_complevel is not None:
                console.print(f"  [dim]Complevel:[/dim] {complevel_name(display_complevel)}")
            if thread.download_links:
                console.print(f"  [dim]Downloads:[/dim] {len(thread.download_links)} link(s) found")
                for link in thread.download_links[:3]:  # Show first 3
                    console.print(f"    [blue]{link}[/blue]")
                if len(thread.download_links) > 3:
                    console.print(f"    [dim]... and {len(thread.download_links) - 3} more[/dim]")

        # Show LLM-specific metadata
        if llm_metadata:
            if llm_metadata.description:
                console.print(f"  [dim]Description:[/dim] {llm_metadata.description[:100]}...")
            if llm_metadata.map_count:
                console.print(f"  [dim]Maps:[/dim] {llm_metadata.map_count}")
            if llm_metadata.difficulty:
                console.print(f"  [dim]Difficulty:[/dim] {llm_metadata.difficulty}")
            if llm_metadata.themes:
                console.print(f"  [dim]Themes:[/dim] {', '.join(llm_metadata.themes)}")


@import_cmd.command(name="url")
@click.argument("title")
@click.argument("url")
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "-t", "tags", multiple=True, shell_complete=_complete_tags)
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
@click.argument("paths", nargs=-1, required=True, type=click.Path(exists=True))
@click.option("--title", "-t", help="Override title (only for single file imports)")
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
def import_local(paths: tuple[str, ...], title: str | None, author: str | None, year: int | None,
                 tags: tuple[str, ...], force: bool):
    """Import local WAD file(s).

    Supports multiple paths for batch import. Titles are inferred from filenames.

    \b
    Examples:
        caco import local ~/Downloads/mymap.wad
        caco import local *.wad --tag new --author "Me"
        caco import local map1.wad map2.pk3 --tag batch
    """
    from pathlib import Path as P

    if title and len(paths) > 1:
        err_console.print("[yellow]--title only works with single file imports[/yellow]")
        err_console.print("[dim]Titles will be inferred from filenames for batch imports[/dim]")

    imported = 0
    skipped = 0

    for path in paths:
        p = P(path).resolve()

        # Infer title from filename if not provided (or multiple files)
        file_title = title if (title and len(paths) == 1) else _infer_title_from_filename(p.name)

        # Check for duplicate
        existing = db.find_duplicate(
            db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            author=author,
        )
        if existing and not force:
            console.print(f"[yellow]Skipped (duplicate):[/yellow] {p.name} → {existing['title']} (ID: {existing['id']})")
            skipped += 1
            continue

        wad_id = db.add_wad(
            title=file_title,
            source_type=db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            cached_path=str(p),
            author=author,
            year=year,
            tags=list(tags) if tags else None,
        )
        console.print(f"[green]Added:[/green] {file_title} (ID: {wad_id})")
        imported += 1

    if len(paths) > 1:
        summary = f"[green]Imported {imported} WAD(s)[/green]"
        if skipped:
            summary += f" [dim]({skipped} skipped as duplicates)[/dim]"
        console.print(summary)


# =============================================================================
# Play
# =============================================================================


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
@click.option("--path", is_flag=True, help="Print config file path")
@click.option("--edit", "-e", is_flag=True, help="Open config in $EDITOR")
def config(path: bool, edit: bool):
    """View or edit configuration.

    Without options, displays the current config file contents.
    Edit the config file directly for full control over all settings.
    """
    import os

    config_path = CONFIG_FILE

    if path:
        click.echo(config_path)
        return

    if edit:
        editor = os.environ.get("EDITOR", os.environ.get("VISUAL", "nano"))
        # Create config file with defaults if it doesn't exist
        if not config_path.exists():
            config_path.parent.mkdir(parents=True, exist_ok=True)
            default_content = '''# Caco configuration file
# Edit these settings to customize caco behavior

# Path to your sourceport executable (e.g., gzdoom, dsda-doom)
sourceport = ""

# Path to your IWAD file (e.g., doom2.wad)
iwad = ""

# Directory for caching downloaded WADs
cache_dir = "~/.cache/caco/wads"

# Directory for sourceport stats files (dsda-doom stats.txt location)
stats_dir = "~/.local/share/nyan-doom/nyan_doom_data"

# idgames download mirror (0-4, see https://www.doomworld.com/idgames/api/)
download_mirror = 0

# Extra arguments to pass to sourceport
sourceport_args = []

# [list] section for customizing list output (coming soon)
# format = ["id", "title", "author", "status"]
# sort = "id+"
'''
            config_path.write_text(default_content)
            console.print(f"[dim]Created default config at {config_path}[/dim]")

        subprocess.run([editor, str(config_path)])
        return

    # Default: show config contents
    console.print(f"[dim]Config file: {config_path}[/dim]")
    console.print()

    if config_path.exists():
        from rich.panel import Panel
        from rich.syntax import Syntax

        content = config_path.read_text()
        syntax = Syntax(content, "toml", theme="monokai", line_numbers=False)
        console.print(Panel(syntax, title="config.toml", border_style="dim"))
    else:
        console.print("[dim]No config file exists. Run 'caco config --edit' to create one.[/dim]")


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


# =============================================================================
# Beaten (WAD Completions)
# =============================================================================


@cli.group(name="beaten")
def beaten_cmd():
    """Manage WAD completion records (times beaten)."""
    pass


@beaten_cmd.command(name="list")
@click.argument("query")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def beaten_list(query: str, yes: bool):
    """List completion records for a WAD (when it was beaten)."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    completions = db.get_wad_completions(wad["id"])

    if not completions:
        console.print(f"[dim]{wad['title']} has not been marked as beaten[/dim]")
        return

    console.print(f"\n[bold]{wad['title']}[/bold] - Completion History ({len(completions)} time(s))\n")

    table = Table()
    table.add_column("ID", style="dim")
    table.add_column("Date")
    table.add_column("Notes")

    for c in completions:
        date = c["completed_at"][:16].replace("T", " ") if c["completed_at"] else "-"
        table.add_row(str(c["id"]), date, c["notes"] or "-")

    console.print(table)


@beaten_cmd.command(name="add")
@click.argument("query")
@click.option("--notes", "-n", help="Notes for this completion")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def beaten_add(query: str, notes: str | None, yes: bool):
    """Manually add a completion record (mark as beaten)."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    completion_id = db.add_wad_completion(wad["id"], notes=notes)
    count = db.get_times_beaten(wad["id"])
    console.print(f"[green]Added completion for {wad['title']}[/green] (now beaten {count} time(s))")


@beaten_cmd.command(name="remove")
@click.argument("query")
@click.argument("completion_id", type=int, required=False)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation / auto-select")
def beaten_remove(query: str, completion_id: int | None, yes: bool):
    """Remove a completion record.

    If COMPLETION_ID is provided, removes that specific record.
    Otherwise, removes the most recent completion.
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    completions = db.get_wad_completions(wad["id"])
    if not completions:
        console.print(f"[dim]{wad['title']} has no completion records[/dim]")
        return

    if completion_id:
        # Remove specific completion
        if db.delete_wad_completion(completion_id):
            count = db.get_times_beaten(wad["id"])
            console.print(f"[green]Removed completion #{completion_id}[/green] (now beaten {count} time(s))")
        else:
            err_console.print(f"[red]Completion #{completion_id} not found[/red]")
    else:
        # Remove most recent (first in list, since sorted DESC)
        latest = completions[0]
        if not yes:
            date = latest["completed_at"][:16].replace("T", " ") if latest["completed_at"] else "unknown date"
            console.print(f"Remove most recent completion from {date}?")
            if not click.confirm("Proceed?"):
                return

        db.delete_wad_completion(latest["id"])
        count = db.get_times_beaten(wad["id"])
        console.print(f"[green]Removed most recent completion[/green] (now beaten {count} time(s))")


@beaten_cmd.command(name="set")
@click.argument("query")
@click.argument("count", type=int)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation / auto-select")
def beaten_set(query: str, count: int, yes: bool):
    """Set completion count to a specific number."""
    if count < 0:
        err_console.print("[red]Count cannot be negative[/red]")
        return

    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    current = db.get_times_beaten(wad["id"])
    if current == count:
        console.print(f"[dim]{wad['title']} is already set to {count} completion(s)[/dim]")
        return

    if not yes:
        if count > current:
            console.print(f"This will add {count - current} completion record(s)")
        else:
            console.print(f"This will remove {current - count} completion record(s)")
        if not click.confirm("Proceed?"):
            return

    db.set_wad_completion_count(wad["id"], count)
    console.print(f"[green]Set {wad['title']} to {count} completion(s)[/green]")


# =============================================================================
# Cache Management Commands
# =============================================================================


def _format_size(size_bytes: int) -> str:
    """Format bytes as human-readable size."""
    for unit in ["B", "KB", "MB", "GB"]:
        if size_bytes < 1024:
            if unit == "B":
                return f"{size_bytes} {unit}"
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.1f} TB"


@cli.group(name="cache")
def cache_cmd():
    """Manage WAD cache."""
    pass


@cache_cmd.command(name="list")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
@click.option("--orphans", is_flag=True, help="Show orphaned files (not in database)")
def cache_list(plain: bool, orphans: bool):
    """List cached WAD files and total cache size."""
    from caco.config import get_cache_dir

    cache_dir = get_cache_dir()

    if orphans:
        _list_orphaned_files(cache_dir, plain)
        return

    cached_wads = db.get_cached_wads()
    total_size = 0
    entries = []

    for wad in cached_wads:
        path = Path(wad["cached_path"])
        if path.exists():
            size = path.stat().st_size
            total_size += size
            entries.append({
                "id": wad["id"],
                "title": wad["title"],
                "filename": path.name,
                "size": size,
                "path": str(path),
            })

    if plain:
        click.echo("ID\tTitle\tFilename\tSize")
        for e in entries:
            click.echo(f"{e['id']}\t{e['title']}\t{e['filename']}\t{e['size']}")
        click.echo(f"\nTotal: {total_size}")
    else:
        if not entries:
            console.print("[dim]Cache is empty[/dim]")
            console.print(f"[dim]Cache location: {cache_dir}[/dim]")
            return

        table = Table(title=f"Cached WADs ({len(entries)} files)")
        table.add_column("ID", style="dim")
        table.add_column("Title", style="cyan")
        table.add_column("Filename")
        table.add_column("Size", justify="right")

        for e in entries:
            table.add_row(
                str(e["id"]),
                e["title"],
                e["filename"],
                _format_size(e["size"]),
            )

        console.print(table)
        console.print(f"\n[bold]Total cache size:[/bold] {_format_size(total_size)}")
        console.print(f"[dim]Cache location: {cache_dir}[/dim]")


def _list_orphaned_files(cache_dir: Path, plain: bool) -> None:
    """List orphaned files in the cache directory."""
    if not cache_dir.exists():
        if plain:
            click.echo("No orphaned files")
        else:
            console.print("[dim]Cache directory does not exist[/dim]")
        return

    orphans = []
    total_size = 0

    for path in cache_dir.iterdir():
        if path.is_file():
            # Check if any WAD references this file
            wad = db.get_wad_by_cached_filename(path.name)
            if not wad:
                size = path.stat().st_size
                orphans.append((path, size))
                total_size += size

    if plain:
        click.echo("Filename\tSize")
        for path, size in orphans:
            click.echo(f"{path.name}\t{size}")
        click.echo(f"\nTotal: {total_size}")
    else:
        if not orphans:
            console.print("[dim]No orphaned files found[/dim]")
            return

        table = Table(title=f"Orphaned Files ({len(orphans)} files)")
        table.add_column("Filename")
        table.add_column("Size", justify="right")

        for path, size in orphans:
            table.add_row(path.name, _format_size(size))

        console.print(table)
        console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")
        console.print(f"[dim]Use 'caco cache clean' to remove orphaned files[/dim]")


@cache_cmd.command(name="clear")
@click.argument("query", required=False)
@click.option("--all", "clear_all", is_flag=True, help="Clear entire cache")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def cache_clear(query: str | None, clear_all: bool, dry_run: bool, yes: bool):
    """Remove cached WAD files.

    Without arguments, shows help. Use --all to clear entire cache,
    or specify QUERY to clear specific WADs.

    \b
    Examples:
        caco cache clear --all           # Clear entire cache
        caco cache clear 1,3,5           # Clear specific WAD IDs
        caco cache clear status:finished # Clear all finished WADs
    """
    from caco.config import get_cache_dir

    if not query and not clear_all:
        click.echo(click.get_current_context().get_help())
        return

    cache_dir = get_cache_dir()

    if clear_all:
        _clear_all_cache(cache_dir, dry_run, yes)
    else:
        _clear_specific_cache(query, cache_dir, dry_run, yes)


def _clear_all_cache(cache_dir: Path, dry_run: bool, yes: bool) -> None:
    """Clear the entire cache."""
    cached_wads = db.get_cached_wads()
    total_size = 0
    files_to_delete = []

    for wad in cached_wads:
        # Only delete idgames sources - they can always be re-downloaded
        # Local files are the user's originals, URLs may not be re-downloadable
        if wad.get("source_type") != "idgames":
            continue

        path = Path(wad["cached_path"])
        if path.exists():
            size = path.stat().st_size
            total_size += size
            files_to_delete.append((wad, path, size))

    if not files_to_delete:
        console.print("[dim]Cache is empty (no idgames WADs cached)[/dim]")
        return

    # Preview
    console.print(f"\n[bold]Files to delete ({len(files_to_delete)}):[/bold]\n")
    for wad, path, size in files_to_delete[:10]:
        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']} ({_format_size(size)})")
    if len(files_to_delete) > 10:
        console.print(f"  [dim]... and {len(files_to_delete) - 10} more[/dim]")

    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        if not click.confirm("\nDelete all cached files?"):
            console.print("[dim]Cancelled[/dim]")
            return

    # Delete files and update database
    deleted = 0
    freed = 0
    for wad, path, size in files_to_delete:
        try:
            path.unlink()
            freed += size
            deleted += 1
            db.clear_cached_path(wad["id"])
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"\n[green]Deleted {deleted} file(s), freed {_format_size(freed)}[/green]")


def _clear_specific_cache(query: str, cache_dir: Path, dry_run: bool, yes: bool) -> None:
    """Clear cache for specific WADs."""
    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return

    # Filter to only cached WADs (skip local source and non-cached)
    files_to_delete = []
    total_size = 0

    for wad in wads:
        # Only delete idgames sources - they can always be re-downloaded
        if wad.get("source_type") != "idgames":
            continue
        if not wad.get("cached_path"):
            continue
        path = Path(wad["cached_path"])
        if path.exists():
            size = path.stat().st_size
            total_size += size
            files_to_delete.append((wad, path, size))

    if not files_to_delete:
        console.print("[dim]Selected WAD(s) are not cached or not from idgames[/dim]")
        return

    # Preview
    console.print(f"\n[bold]Files to delete ({len(files_to_delete)}):[/bold]\n")
    for wad, path, size in files_to_delete[:10]:
        console.print(f"  [dim][{wad['id']}][/dim] {wad['title']} ({_format_size(size)})")
    if len(files_to_delete) > 10:
        console.print(f"  [dim]... and {len(files_to_delete) - 10} more[/dim]")

    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    # Delete
    deleted = 0
    freed = 0
    for wad, path, size in files_to_delete:
        try:
            path.unlink()
            freed += size
            deleted += 1
            db.clear_cached_path(wad["id"])
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"\n[green]Deleted {deleted} file(s), freed {_format_size(freed)}[/green]")


@cache_cmd.command(name="clean")
@click.option("--dry-run", is_flag=True, help="Show what would be deleted")
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation prompt")
def cache_clean(dry_run: bool, yes: bool):
    """Remove orphaned files from cache.

    Orphaned files are files in the cache directory that are not
    tracked by any WAD in the database.
    """
    from caco.config import get_cache_dir

    cache_dir = get_cache_dir()
    if not cache_dir.exists():
        console.print("[dim]Cache directory does not exist[/dim]")
        return

    # Find all orphaned files
    orphans = []
    total_size = 0

    for path in cache_dir.iterdir():
        if path.is_file():
            wad = db.get_wad_by_cached_filename(path.name)
            if not wad:
                size = path.stat().st_size
                orphans.append((path, size))
                total_size += size

    if not orphans:
        console.print("[dim]No orphaned files found[/dim]")
        return

    # Preview
    console.print(f"\n[bold]Orphaned files ({len(orphans)}):[/bold]\n")
    for path, size in orphans[:10]:
        console.print(f"  {path.name} ({_format_size(size)})")
    if len(orphans) > 10:
        console.print(f"  [dim]... and {len(orphans) - 10} more[/dim]")

    console.print(f"\n[bold]Total:[/bold] {_format_size(total_size)}")

    if dry_run:
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    if not yes:
        if not click.confirm("\nDelete orphaned files?"):
            console.print("[dim]Cancelled[/dim]")
            return

    # Delete
    deleted = 0
    freed = 0
    for path, size in orphans:
        try:
            path.unlink()
            freed += size
            deleted += 1
        except OSError as e:
            err_console.print(f"[red]Failed to delete {path}: {e}[/red]")

    console.print(f"\n[green]Deleted {deleted} orphaned file(s), freed {_format_size(freed)}[/green]")


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
        wads = resolve_wad_query(query, mode="multiple", yes=True)
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
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def map_complete(query: str, maps: tuple[str, ...], skill: int | None, notes: str | None, yes: bool):
    """Manually mark maps as completed."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return  # User cancelled
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
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def map_uncomplete(query: str, maps: tuple[str, ...], skill: int | None, yes: bool):
    """Remove map completion records."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return  # User cancelled
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
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
@click.option("--all-cycles", "-a", is_flag=True, help="Show completions from all playthroughs")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
def map_list(query: str, yes: bool, all_cycles: bool, plain: bool):
    """List completed maps for a WAD (current playthrough by default)."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return  # User cancelled
    wad = wads[0]

    completions = db.get_map_completions(wad["id"], current_cycle_only=not all_cycles)

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
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
@click.option("--plain", is_flag=True, help="Output as key=value pairs (for scripting)")
def map_progress(query: str, total: int | None, yes: bool, plain: bool):
    """Show map completion progress for a WAD."""
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return  # User cancelled
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


# =============================================================================
# Command Aliases
# =============================================================================

# Unix-like aliases for common commands
cli.add_command(import_auto, name="add")  # caco add → caco import auto
cli.add_command(delete, name="rm")        # caco rm → caco delete
cli.add_command(list_cmd, name="ls")      # caco ls → caco list
cli.add_command(info, name="i")           # caco i → caco info


if __name__ == "__main__":
    cli()

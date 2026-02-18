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
    get_iwad_dirs,
    load_config,
    save_config,
    CONFIG_FILE,
    get_list_config,
)
from caco.db import STATUS_SHORTCUTS
from caco.player import play, format_duration

console = Console()
err_console = Console(stderr=True)


# =============================================================================
# Shared Import Helper
# =============================================================================


def _check_and_import_entry(
    source,
    entry,
    source_type: db.SourceType,
    tags_list: list[str] | None,
    force: bool,
    *,
    source_id: str | None = None,
    filename: str | None = None,
    author: str | None = None,
) -> int | None:
    """Check for duplicates and import a WAD entry.

    Returns wad_id if imported, None if skipped due to duplicate.
    """
    existing = db.find_duplicate(
        source_type,
        source_id=source_id,
        filename=filename,
        author=author,
    )
    if existing and not force:
        console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
        console.print("[dim]Use --force to import anyway[/dim]")
        return None

    return source.import_wad(entry, tags=tags_list)


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
) -> list[int] | None:
    """
    Use fzf to select from a list of items.

    Args:
        items: List of strings to select from
        prompt: Prompt to display
        multi: Allow multiple selections

    Returns:
        List of selected indices (0-based), or None if cancelled.
    """
    if not _fzf_available():
        return None

    # Build fzf command
    cmd = ["fzf", "--prompt", f"{prompt}> "]
    if multi:
        cmd.append("--multi")

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
        List of WAD dicts, or None if user cancelled interactive selection.

    Raises:
        SystemExit: In mode="error", if query matches zero WADs or multiple
            WADs. Also raised for missing IDs in ID range lookups. This is
            intentional for CLI convenience.
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


def _render_wad_list_json(wads: list[dict]) -> None:
    """JSON output for list command."""
    import json as _json

    wad_ids = [w["id"] for w in wads]
    times_beaten = db.get_times_beaten_batch(wad_ids)
    playtimes = db.get_total_playtime_batch(wad_ids)
    last_played_map = db.get_last_played_batch(wad_ids)

    result = []
    for wad in wads:
        playtime = playtimes.get(wad["id"], 0)
        last_played = last_played_map.get(wad["id"])
        beaten = times_beaten.get(wad["id"], 0)
        result.append({
            "id": wad["id"],
            "title": wad["title"],
            "author": wad.get("author"),
            "year": wad.get("year"),
            "status": wad["status"],
            "rating": wad.get("rating"),
            "tags": wad.get("tags", []),
            "source_type": wad["source_type"],
            "source_url": wad.get("source_url"),
            "idgames_id": wad.get("idgames_id"),
            "filename": wad.get("filename"),
            "version": wad.get("version"),
            "playtime_seconds": playtime,
            "playtime": format_duration(playtime) if playtime else None,
            "times_beaten": beaten,
            "last_played": last_played,
            "created_at": wad.get("created_at"),
        })

    print(_json.dumps(result, indent=2))


def _render_wad_info_json(wad: dict) -> None:
    """JSON output for info command."""
    import json as _json

    playtime = db.get_total_playtime(wad["id"])
    sessions = db.get_sessions(wad["id"])
    last_played = db.get_last_played(wad["id"])
    times_beaten = db.get_times_beaten(wad["id"])

    result = {
        "id": wad["id"],
        "title": wad["title"],
        "author": wad.get("author"),
        "year": wad.get("year"),
        "description": wad.get("description"),
        "status": wad["status"],
        "rating": wad.get("rating"),
        "notes": wad.get("notes"),
        "tags": wad.get("tags", []),
        "source_type": wad["source_type"],
        "source_id": wad.get("source_id"),
        "source_url": wad.get("source_url"),
        "idgames_id": wad.get("idgames_id"),
        "filename": wad.get("filename"),
        "version": wad.get("version"),
        "custom_iwad": wad.get("custom_iwad"),
        "custom_sourceport": wad.get("custom_sourceport"),
        "custom_args": wad.get("custom_args"),
        "playtime_seconds": playtime,
        "playtime": format_duration(playtime) if playtime else None,
        "session_count": len(sessions),
        "times_beaten": times_beaten,
        "last_played": last_played,
        "created_at": wad.get("created_at"),
        "updated_at": wad.get("updated_at"),
    }

    print(_json.dumps(result, indent=2))


def _render_wad_list_plain(wads: list[dict]) -> None:
    """TSV output: ID\tTitle\tAuthor\tStatus\tBeaten\tPlaytime\tLastPlayed."""
    # Batch fetch stats for all WADs
    wad_ids = [w["id"] for w in wads]
    times_beaten = db.get_times_beaten_batch(wad_ids)
    playtimes = db.get_total_playtime_batch(wad_ids)
    last_played_map = db.get_last_played_batch(wad_ids)

    # Header
    print("ID\tTitle\tAuthor\tStatus\tBeaten\tPlaytime\tLastPlayed")
    for wad in wads:
        playtime = playtimes.get(wad["id"], 0)
        playtime_str = format_duration(playtime) if playtime else ""
        last_played = last_played_map.get(wad["id"])
        last_played_str = last_played[:10] if last_played else ""
        beaten_str = str(times_beaten.get(wad["id"], 0))
        print(f"{wad['id']}\t{wad['title']}\t{wad['author'] or ''}\t{wad['status']}\t{beaten_str}\t{playtime_str}\t{last_played_str}")


def _render_wad_info_plain(wad: dict) -> None:
    """Key=value output for scripting."""
    playtime = db.get_total_playtime(wad["id"])
    sessions = db.get_sessions(wad["id"])
    last_played = db.get_last_played(wad["id"])
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
    print(f"idgames_id={wad.get('idgames_id') or ''}")
    print(f"filename={wad.get('filename') or ''}")
    print(f"playtime={format_duration(playtime) if playtime else ''}")
    print(f"sessions={len(sessions)}")
    print(f"last_played={last_played[:10] if last_played else ''}")
    print(f"times_beaten={times_beaten}")
    if wad.get("custom_iwad"):
        print(f"custom_iwad={wad['custom_iwad']}")
    if wad.get("custom_sourceport"):
        print(f"custom_sourceport={wad['custom_sourceport']}")
    if wad.get("custom_args"):
        print(f"custom_args={wad['custom_args']}")


def _render_wad_list(wads: list[dict], title: str | None = None, list_config: dict | None = None) -> None:
    """Render a list of WADs as a table.

    Args:
        wads: List of WAD dicts to display
        title: Optional table title
        list_config: Optional config dict with 'format' key
    """
    if not wads:
        console.print("[dim]No WADs found[/dim]")
        return

    # Get list config
    if list_config is None:
        list_config = get_list_config()

    columns = list_config.get("format", ["id", "title", "author", "status", "beaten", "playtime", "last_played"])

    # Batch fetch stats for all WADs
    wad_ids = [w["id"] for w in wads]
    times_beaten = db.get_times_beaten_batch(wad_ids)
    playtimes = db.get_total_playtime_batch(wad_ids)
    last_played_map = db.get_last_played_batch(wad_ids)

    # Column definitions: name -> (header, style, justify)
    column_defs = {
        "id": ("ID", "dim", None),
        "title": ("Title", "cyan", None),
        "author": ("Author", None, None),
        "year": ("Year", "dim", "right"),
        "status": ("Status", None, None),
        "rating": ("Rating", None, "center"),
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
        # Pre-computed batch values
        playtime = playtimes.get(wad["id"], 0)
        last_played = last_played_map.get(wad["id"])

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
                row_values.append(wad["status"])
            elif col == "rating":
                if wad.get("rating"):
                    row_values.append("\u2605" * wad["rating"] + "\u2606" * (5 - wad["rating"]))
                else:
                    row_values.append("-")
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


@click.group(invoke_without_command=True)
@click.option("--tui", is_flag=True, help="Launch TUI interface")
@click.option("--gui", is_flag=True, help="Launch GUI interface (requires PySide6)")
@click.pass_context
def cli(ctx, tui: bool, gui: bool):
    """Caco - Personal Doom WAD library manager."""
    db.init_db()

    # Warn about non-existent iwad_dirs
    for iwad_dir in get_iwad_dirs():
        if not iwad_dir.is_dir():
            err_console.print(f"[yellow]Warning: iwad_dirs entry does not exist: {iwad_dir}[/yellow]")

    if tui:
        from caco.tui import CacoApp
        CacoApp().run()
        ctx.exit(0)

    if gui:
        from caco.gui import CacoGuiApp
        code = CacoGuiApp().run()
        ctx.exit(code)

    # If no command given and not TUI/GUI, show help
    if ctx.invoked_subcommand is None and not tui and not gui:
        click.echo(ctx.get_help())


# =============================================================================
# Import all submodules to register commands with the cli group
# =============================================================================

from caco.cli import library  # noqa: E402, F401
from caco.cli import import_cmds  # noqa: E402, F401
from caco.cli import tags  # noqa: E402, F401
from caco.cli import play_cmd as play_mod  # noqa: E402, F401
from caco.cli import cache  # noqa: E402, F401
from caco.cli import config_cmd  # noqa: E402, F401
from caco.cli import stats  # noqa: E402, F401


# =============================================================================
# Command Aliases
# =============================================================================

# Unix-like aliases for common commands
cli.add_command(library.delete, name="rm")             # caco rm -> caco delete
cli.add_command(library.list_cmd, name="ls")           # caco ls -> caco list
cli.add_command(library.info, name="i")                # caco i -> caco info


if __name__ == "__main__":
    cli()

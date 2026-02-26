"""Stats and beaten commands."""

import click
from rich.table import Table

from caco import db
from caco.player import format_duration
from caco.wad_stats import (
    WadStats,
    format_stats,
    format_time_tics,
    format_time_secs,
    parse_stats_file,
    skill_name,
    stats_from_json,
    stats_to_json,
)

from caco.cli import (
    cli,
    console,
    err_console,
    resolve_wad_query,
)


# =============================================================================
# Library Statistics
# =============================================================================


@cli.command()
@click.option("--period", "-p", type=click.Choice(["month", "year"]),
              default="month", help="Group activity by month or year")
@click.option("--limit", "-n", type=int, default=12,
              help="Number of periods to show in activity table")
@click.option("--plain", is_flag=True,
              help="Output as key=value pairs (for scripting)")
def stats(period: str, limit: int, plain: bool):
    """Show library statistics.

    Displays library-wide statistics including total playtime,
    completion rates, WAD counts by status, and activity over time.

    \b
    Examples:
        caco stats                # Default: group by month, show 12 periods
        caco stats --period year  # Group activity by year
        caco stats --limit 6      # Show only last 6 periods
        caco stats --plain        # Output for scripting
    """
    snap = db.get_stats_snapshot(period)

    if plain:
        # Key=value output for scripting
        print(f"total_wads={snap.total_wads}")
        print(f"total_sessions={snap.total_sessions}")
        print(f"total_playtime={snap.total_playtime}")
        print(f"wads_with_sessions={snap.wads_with_sessions}")
        print(f"played_wads={snap.played_wads}")
        print(f"finished_wads={snap.finished_wads}")
        print(f"completion_rate={snap.completion_rate:.3f}")
        print(f"total_completions={snap.total_completions}")

        # Status breakdown
        for status, count in sorted(snap.wads_by_status.items()):
            print(f"status_{status.replace('-', '_')}={count}")

        # Activity periods
        for i, row in enumerate(snap.activity[:limit]):
            print(f"activity_{i}_period={row['period']}")
            print(f"activity_{i}_wads={row['wad_count']}")
            print(f"activity_{i}_sessions={row['session_count']}")
            print(f"activity_{i}_playtime={row['total_playtime']}")
        return

    # Check for empty library
    if snap.total_wads == 0:
        console.print("[dim]No WADs in library[/dim]")
        return

    # Rich formatted output
    console.print("\n[bold]Library Statistics[/bold]\n")

    # Overview section
    console.print("[bold cyan]Overview[/bold cyan]")
    console.print(f"  Total WADs:      {snap.total_wads}")
    total_playtime_str = format_duration(snap.total_playtime) if snap.total_playtime else "0s"
    console.print(f"  Total playtime:  {total_playtime_str}")
    console.print(f"  Sessions:        {snap.total_sessions}")
    console.print(f"  WADs played:     {snap.wads_with_sessions}")

    # Completion section
    console.print("\n[bold cyan]Completion[/bold cyan]")
    if snap.played_wads > 0:
        pct = snap.completion_rate * 100
        console.print(f"  Finished:          {snap.finished_wads} / {snap.played_wads} played ({pct:.1f}%)")
    else:
        console.print(f"  Finished:          {snap.finished_wads} / 0 played")
    if snap.total_completions > 0:
        console.print(f"  Total completions: {snap.total_completions} (including replays)")

    # Status breakdown
    status_counts = snap.wads_by_status
    if status_counts:
        console.print("\n[bold cyan]Status Breakdown[/bold cyan]")
        # Order statuses for consistent display
        status_order = ['to-play', 'playing', 'backlog', 'finished', 'abandoned', 'awaiting-update']
        # Format as two columns
        items = []
        for status in status_order:
            if status in status_counts:
                items.append((status, status_counts[status]))
        # Add any statuses not in our order
        for status, count in sorted(status_counts.items()):
            if status not in status_order:
                items.append((status, count))

        # Display in two columns
        for i in range(0, len(items), 2):
            left = f"  {items[i][0]}:".ljust(18) + str(items[i][1]).rjust(4)
            if i + 1 < len(items):
                right = f"  {items[i+1][0]}:".ljust(18) + str(items[i+1][1]).rjust(4)
                console.print(f"{left}{right}")
            else:
                console.print(left)

    # Activity table
    activity = snap.activity
    if not activity:
        console.print("\n[dim]No play history yet[/dim]")
    else:
        period_label = "Month" if period == "month" else "Year"
        console.print(f"\n[bold cyan]Activity by {period_label}[/bold cyan]")

        table = Table(show_header=True, header_style="bold")
        table.add_column("Period", style="dim")
        table.add_column("WADs", justify="right")
        table.add_column("Sessions", justify="right")
        table.add_column("Playtime", justify="right")

        for row in activity[:limit]:
            playtime_str = format_duration(row['total_playtime']) if row['total_playtime'] else "-"
            table.add_row(
                row['period'],
                str(row['wad_count']),
                str(row['session_count']),
                playtime_str,
            )

        console.print(table)

        if len(activity) > limit:
            console.print(f"[dim]... and {len(activity) - limit} more period(s)[/dim]")

    console.print()


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
    table.add_column("Stats", justify="center")

    for c in completions:
        date = c["completed_at"][:16].replace("T", " ") if c["completed_at"] else "-"
        has_stats = "[green]*[/green]" if c.get("stats_snapshot") else ""
        table.add_row(str(c["id"]), date, c["notes"] or "-", has_stats)

    console.print(table)


@beaten_cmd.command(name="add")
@click.argument("query")
@click.option("--notes", "-n", help="Notes for this completion")
@click.option("--stats-file", "-s", type=click.Path(exists=True),
              help="Import per-map stats from a stats.txt or levelstat.txt file")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def beaten_add(query: str, notes: str | None, stats_file: str | None, yes: bool):
    """Manually add a completion record (mark as beaten).

    \b
    Optionally attach per-map statistics from a sourceport stats file:
        caco beaten add <query> --stats-file path/to/stats.txt

    Supported formats:
    - nyan-doom/dsda-doom stats.txt (persistent per-map tracking)
    - dsda-doom levelstat.txt (-levelstat flag output)
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    snapshot_json: str | None = None
    if stats_file:
        try:
            wad_stats = parse_stats_file(stats_file)
            snapshot_json = stats_to_json(wad_stats)
            played = wad_stats.played_maps
            console.print(
                f"[dim]Parsed {wad_stats.format}: "
                f"{len(played)} map(s) played, "
                f"total time {wad_stats.total_time_display}[/dim]"
            )
        except (ValueError, OSError) as e:
            err_console.print(f"[red]Failed to parse stats file: {e}[/red]")
            return
    elif wad.get("stats_snapshot"):
        snapshot_json = wad["stats_snapshot"]
        wad_stats = stats_from_json(snapshot_json)
        played = wad_stats.played_maps
        console.print(
            f"[dim]Auto-attaching stats: {len(played)} map(s) played, "
            f"total time {wad_stats.total_time_display}[/dim]"
        )

    completion_id = db.add_wad_completion(
        wad["id"], stats_snapshot=snapshot_json, notes=notes
    )
    count = db.get_times_beaten(wad["id"])
    msg = f"[green]Added completion for {wad['title']}[/green] (now beaten {count} time(s))"
    if snapshot_json:
        msg += " [dim](with stats)[/dim]"
    console.print(msg)


@beaten_cmd.command(name="attach")
@click.argument("query")
@click.argument("completion_id", type=int, required=False)
@click.option("--stats-file", "-s", type=click.Path(exists=True), required=True,
              help="Stats file to attach (stats.txt or levelstat.txt)")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def beaten_attach(query: str, completion_id: int | None, stats_file: str, yes: bool):
    """Attach a stats file to an existing completion record.

    If COMPLETION_ID is not given, attaches to the most recent completion.

    \b
    Examples:
        caco beaten attach "Doom 2 In Retrospect" --stats-file stats.txt
        caco beaten attach "Doom 2 In Retrospect" 42 -s stats.txt
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    completions = db.get_wad_completions(wad["id"])
    if not completions:
        err_console.print(f"[dim]{wad['title']} has no completion records[/dim]")
        return

    if completion_id:
        target = None
        for c in completions:
            if c["id"] == completion_id:
                target = c
                break
        if not target:
            err_console.print(f"[red]Completion #{completion_id} not found[/red]")
            return
    else:
        target = completions[0]  # Most recent

    try:
        wad_stats = parse_stats_file(stats_file)
        snapshot_json = stats_to_json(wad_stats)
        played = wad_stats.played_maps
    except (ValueError, OSError) as e:
        err_console.print(f"[red]Failed to parse stats file: {e}[/red]")
        return

    if target.get("stats_snapshot") and not yes:
        console.print(f"Completion #{target['id']} already has stats attached. Overwrite?")
        if not click.confirm("Proceed?"):
            return

    db.update_wad_completion(target["id"], stats_snapshot=snapshot_json)
    date = target["completed_at"][:16].replace("T", " ") if target["completed_at"] else "-"
    console.print(
        f"[green]Attached stats to completion #{target['id']} ({date})[/green] "
        f"[dim]({wad_stats.format}: {len(played)} map(s), "
        f"time {wad_stats.total_time_display})[/dim]"
    )


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


def _build_stats_entries(wad: dict) -> list[dict]:
    """Build a list of stats entries: live first, then completions with stats.

    Each entry has: stats_snapshot, label, _live flag, and completion fields.
    Matches the pattern used by GUI/TUI for consistency.
    """
    entries = []

    # Live stats from wad's stats_snapshot
    if wad.get("stats_snapshot"):
        entries.append({
            "id": None,
            "completed_at": None,
            "stats_snapshot": wad["stats_snapshot"],
            "notes": None,
            "_live": True,
        })

    # Completions with stats
    completions = db.get_wad_completions(wad["id"])
    for c in completions:
        if c.get("stats_snapshot"):
            entries.append(c)

    return entries


def _find_completion_with_stats(
    wad: dict, completion_id: int | None, *, allow_live: bool = False,
) -> dict | None:
    """Find a completion with stats_snapshot, or None."""
    completions = db.get_wad_completions(wad["id"])

    if completion_id:
        for c in completions:
            if c["id"] == completion_id:
                if not c.get("stats_snapshot"):
                    err_console.print(
                        f"[dim]Completion #{completion_id} has no stats attached[/dim]"
                    )
                    return None
                return c
        err_console.print(f"[red]Completion #{completion_id} not found[/red]")
        return None

    # Find most recent completion with stats
    for c in completions:
        if c.get("stats_snapshot"):
            return c

    # Fall back to live stats if allowed
    if allow_live and wad.get("stats_snapshot"):
        return {
            "id": None,
            "completed_at": None,
            "stats_snapshot": wad["stats_snapshot"],
            "notes": None,
            "_live": True,
        }

    if not completions:
        err_console.print(f"[dim]{wad['title']} has no completion records[/dim]")
    else:
        err_console.print(f"[dim]No completions with stats found for {wad['title']}[/dim]")
    return None


def _entry_label(entry: dict) -> str:
    """Build a display label for a stats entry."""
    if entry.get("_live"):
        return "Current (live)"
    date = entry["completed_at"][:16].replace("T", " ") if entry.get("completed_at") else "-"
    return f"Completion #{entry['id']} ({date})"


def _print_entry(entry: dict, *, plain: bool = False) -> None:
    """Print a single stats entry (header + summary + table)."""
    wad_stats = stats_from_json(entry["stats_snapshot"])
    played = wad_stats.played_maps
    label = _entry_label(entry)

    if plain:
        print(f"# {label}")
        _print_stats_plain(wad_stats)
        return

    console.print(f"[bold]── {label} [/bold]" + "─" * max(0, 40 - len(label)))
    console.print(
        f"[dim]Format: {wad_stats.format} | "
        f"Maps: {len(played)} | "
        f"Time: {wad_stats.total_time_display}[/dim]\n"
    )

    if wad_stats.format == "stats_txt":
        _print_stats_txt_table(played)
    else:
        _print_levelstat_table(played)


@beaten_cmd.command(name="stats")
@click.argument("query")
@click.argument("completion_id", type=int, required=False)
@click.option("--live", is_flag=True, help="Show only live stats snapshot")
@click.option("--plain", is_flag=True, help="Output as TSV for scripting")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def beaten_stats(query: str, completion_id: int | None, live: bool, plain: bool, yes: bool):
    """Show per-map statistics for a WAD.

    Without COMPLETION_ID, shows all stats entries — live stats first,
    then each completion with stats. With COMPLETION_ID, shows just that one.
    Use --live to show only the current live stats snapshot.

    \b
    Examples:
        caco beaten stats "Doom 2 In Retrospect"           # all entries
        caco beaten stats "Doom 2 In Retrospect" 42        # specific completion
        caco beaten stats "Doom 2 In Retrospect" --live    # live stats only
        caco beaten stats "Doom 2 In Retrospect" --plain   # all, TSV format
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    # --live: show only the live stats snapshot
    if live:
        if not wad.get("stats_snapshot"):
            err_console.print(f"[dim]No live stats for {wad['title']}[/dim]")
            return
        entry = {
            "id": None, "completed_at": None,
            "stats_snapshot": wad["stats_snapshot"],
            "notes": None, "_live": True,
        }
        if not plain:
            console.print(f"\n[bold]{wad['title']}[/bold] — Map Statistics\n")
        _print_entry(entry, plain=plain)
        return

    # Specific completion ID: show just that one (original behavior)
    if completion_id is not None:
        comp = _find_completion_with_stats(wad, completion_id)
        if not comp:
            return
        if not plain:
            console.print(f"\n[bold]{wad['title']}[/bold] — Map Statistics\n")
        _print_entry(comp, plain=plain)
        return

    # Default: show all entries (live + completions)
    entries = _build_stats_entries(wad)
    if not entries:
        err_console.print(f"[dim]No stats available for {wad['title']}[/dim]")
        return

    if not plain:
        console.print(f"\n[bold]{wad['title']}[/bold] — Map Statistics\n")

    for i, entry in enumerate(entries):
        if i > 0:
            if plain:
                print()
            else:
                console.print()
        _print_entry(entry, plain=plain)


def _print_stats_plain(wad_stats: WadStats) -> None:
    """Print stats as TSV for scripting."""
    played = wad_stats.played_maps
    if wad_stats.format == "stats_txt":
        print("map\tskill\ttime\tbest_time\tmax_time\tnm_time\texits\t"
              "total_k\tbest_k\tbest_i\tbest_s\tmax_k\tmax_i\tmax_s")
        for m in played:
            print(f"{m.lump}\t{m.best_skill}\t{m.best_time}\t"
                  f"{m.best_max_time}\t{m.best_nm_time}\t"
                  f"{m.total_exits}\t{m.cumulative_kills}\t"
                  f"{m.kills}\t{m.items}\t{m.secrets}\t"
                  f"{m.total_kills}\t{m.total_items}\t{m.total_secrets}")
    else:
        print("map\ttime\ttotal_time\tkills\ttotal_kills\titems\t"
              "total_items\tsecrets\ttotal_secrets")
        for m in played:
            print(f"{m.lump}\t{m.time_secs:.2f}\t{m.total_time_secs:.2f}\t"
                  f"{m.kills}\t{m.total_kills}\t{m.items}\t{m.total_items}\t"
                  f"{m.secrets}\t{m.total_secrets}")


def _print_stats_txt_table(maps: list) -> None:
    """Print a Rich table for stats.txt format data."""
    table = Table(show_header=True, header_style="bold")
    table.add_column("Map", style="cyan")
    table.add_column("Skill")
    table.add_column("Time", justify="right")
    table.add_column("Max Time", justify="right")
    table.add_column("NM Time", justify="right")
    table.add_column("Exits", justify="right")
    table.add_column("K", justify="right")
    table.add_column("I", justify="right")
    table.add_column("S", justify="right")

    for m in maps:
        time_str = format_time_tics(m.best_time)
        max_time_str = format_time_tics(m.best_max_time)
        nm_time_str = format_time_tics(m.best_nm_time)

        k_str = f"{m.kills}/{m.total_kills}" if m.total_kills >= 0 else str(m.kills)
        i_str = f"{m.items}/{m.total_items}" if m.total_items >= 0 else str(m.items)
        s_str = f"{m.secrets}/{m.total_secrets}" if m.total_secrets >= 0 else str(m.secrets)

        table.add_row(
            m.lump,
            skill_name(m.best_skill),
            time_str,
            max_time_str,
            nm_time_str,
            str(m.total_exits),
            k_str,
            i_str,
            s_str,
        )

    console.print(table)


def _print_levelstat_table(maps: list) -> None:
    """Print a Rich table for levelstat.txt format data."""
    table = Table(show_header=True, header_style="bold")
    table.add_column("Map", style="cyan")
    table.add_column("Time", justify="right")
    table.add_column("Total Time", justify="right")
    table.add_column("K", justify="right")
    table.add_column("I", justify="right")
    table.add_column("S", justify="right")

    for m in maps:
        time_str = format_time_secs(m.time_secs)
        total_str = format_time_secs(m.total_time_secs)

        k_str = f"{m.kills}/{m.total_kills}"
        i_str = f"{m.items}/{m.total_items}"
        s_str = f"{m.secrets}/{m.total_secrets}"

        table.add_row(m.lump, time_str, total_str, k_str, i_str, s_str)

    console.print(table)


@beaten_cmd.command(name="export")
@click.argument("query")
@click.argument("completion_id", type=int, required=False)
@click.option("--live", is_flag=True, help="Export live stats snapshot instead of completion")
@click.option("--output", "-o", type=click.Path(), help="Write to file instead of stdout")
@click.option("--yes", "-y", is_flag=True, help="Auto-select first match if multiple")
def beaten_export(query: str, completion_id: int | None, live: bool, output: str | None, yes: bool):
    """Export per-map stats back to original text format.

    If COMPLETION_ID is not given, uses the most recent completion with stats.
    Use --live to export the current live stats snapshot instead.

    \b
    Examples:
        caco beaten export "Doom 2 In Retrospect"
        caco beaten export "Doom 2 In Retrospect" --output stats.txt
        caco beaten export "Doom 2 In Retrospect" 42 -o levelstat.txt
        caco beaten export "Doom 2 In Retrospect" --live
    """
    wads = resolve_wad_query(query, mode="pick", yes=yes)
    if not wads:
        return
    wad = wads[0]

    if live:
        if not wad.get("stats_snapshot"):
            err_console.print(f"[dim]No live stats for {wad['title']}[/dim]")
            return
        snapshot = wad["stats_snapshot"]
    else:
        comp = _find_completion_with_stats(wad, completion_id, allow_live=True)
        if not comp:
            return
        snapshot = comp["stats_snapshot"]

    wad_stats = stats_from_json(snapshot)
    text = format_stats(wad_stats)

    if output:
        from pathlib import Path
        Path(output).write_text(text)
        console.print(f"[green]Exported stats to {output}[/green]")
    else:
        print(text, end="")

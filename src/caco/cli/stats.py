"""Stats command and per-map stats helpers (used by info --levelstats)."""

import click
from rich.table import Table

from caco import db
from caco.player import format_duration
from caco.wad_stats import (
    WadStats,
    format_time_tics,
    format_time_secs,
    skill_name,
    stats_from_json,
)

from caco.cli import (
    cli,
    console,
    err_console,
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


def _entry_label(entry: dict) -> str:
    """Build a display label for a stats entry."""
    if entry.get("_live"):
        return "Current (live)"
    date = entry["completed_at"][:16].replace("T", " ") if entry.get("completed_at") else "-"
    return f"Completion ({date})"


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



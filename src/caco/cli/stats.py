"""Stats and beaten commands."""

import click
from rich.table import Table

from caco import db
from caco.player import format_duration

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
    library_stats = db.get_library_stats()
    completion_stats = db.get_completion_rate()
    activity = db.get_wads_played_by_period(period)

    if plain:
        # Key=value output for scripting
        print(f"total_wads={library_stats['total_wads']}")
        print(f"total_sessions={library_stats['total_sessions']}")
        print(f"total_playtime={library_stats['total_playtime']}")
        print(f"wads_with_sessions={library_stats['wads_with_sessions']}")
        print(f"played_wads={completion_stats['played_wads']}")
        print(f"finished_wads={completion_stats['finished_wads']}")
        print(f"completion_rate={completion_stats['completion_rate']:.3f}")
        print(f"total_completions={completion_stats['total_completions']}")

        # Status breakdown
        for status, count in sorted(library_stats['wads_by_status'].items()):
            print(f"status_{status.replace('-', '_')}={count}")

        # Activity periods
        for i, row in enumerate(activity[:limit]):
            print(f"activity_{i}_period={row['period']}")
            print(f"activity_{i}_wads={row['wad_count']}")
            print(f"activity_{i}_sessions={row['session_count']}")
            print(f"activity_{i}_playtime={row['total_playtime']}")
        return

    # Check for empty library
    if library_stats['total_wads'] == 0:
        console.print("[dim]No WADs in library[/dim]")
        return

    # Rich formatted output
    console.print("\n[bold]Library Statistics[/bold]\n")

    # Overview section
    console.print("[bold cyan]Overview[/bold cyan]")
    console.print(f"  Total WADs:      {library_stats['total_wads']}")
    total_playtime_str = format_duration(library_stats['total_playtime']) if library_stats['total_playtime'] else "0s"
    console.print(f"  Total playtime:  {total_playtime_str}")
    console.print(f"  Sessions:        {library_stats['total_sessions']}")
    console.print(f"  WADs played:     {library_stats['wads_with_sessions']}")

    # Completion section
    console.print("\n[bold cyan]Completion[/bold cyan]")
    played = completion_stats['played_wads']
    finished = completion_stats['finished_wads']
    if played > 0:
        pct = completion_stats['completion_rate'] * 100
        console.print(f"  Finished:          {finished} / {played} played ({pct:.1f}%)")
    else:
        console.print(f"  Finished:          {finished} / 0 played")
    if completion_stats['total_completions'] > 0:
        console.print(f"  Total completions: {completion_stats['total_completions']} (including replays)")

    # Status breakdown
    status_counts = library_stats['wads_by_status']
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

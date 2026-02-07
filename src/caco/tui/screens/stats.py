"""Library statistics screen."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Vertical
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Static

from caco import db
from caco.player import format_duration


class StatsScreen(Screen):
    """Full-screen library statistics view."""

    BINDINGS = [
        Binding("q", "back", "Back", show=True),
        Binding("escape", "back", "Back", show=False),
    ]

    DEFAULT_CSS = """
    StatsScreen {
        padding: 1 2;
    }

    StatsScreen #stats-header {
        text-style: bold;
        margin-bottom: 1;
    }

    StatsScreen .stats-section {
        text-style: bold;
        margin-top: 1;
        margin-bottom: 1;
        border-bottom: solid $primary-darken-2;
    }

    StatsScreen #activity-table {
        height: 1fr;
        margin-top: 1;
    }
    """

    def compose(self) -> ComposeResult:
        yield Static("[bold]Library Statistics[/bold]", id="stats-header")
        yield Vertical(id="stats-content")
        yield Footer()

    def on_mount(self) -> None:
        """Load and display statistics."""
        content = self.query_one("#stats-content", Vertical)

        # Overview
        stats = db.get_library_stats()
        content.mount(Static("[bold]Overview[/bold]", classes="stats-section"))
        content.mount(Static(f"  Total WADs: {stats['total_wads']}"))
        content.mount(Static(f"  Total sessions: {stats['total_sessions']}"))
        content.mount(Static(f"  Total playtime: {format_duration(stats['total_playtime'])}"))
        content.mount(Static(f"  WADs played: {stats['wads_with_sessions']}"))

        # Status breakdown
        if stats["wads_by_status"]:
            content.mount(Static(""))
            content.mount(Static("[bold]By Status[/bold]", classes="stats-section"))
            for status, count in sorted(stats["wads_by_status"].items()):
                content.mount(Static(f"  {status}: {count}"))

        # Completion stats
        completion = db.get_completion_rate()
        content.mount(Static(""))
        content.mount(Static("[bold]Completion[/bold]", classes="stats-section"))
        content.mount(Static(f"  Played: {completion['played_wads']}"))
        content.mount(Static(f"  Finished: {completion['finished_wads']}"))
        rate_pct = f"{completion['completion_rate']:.0%}"
        content.mount(Static(f"  Completion rate: {rate_pct}"))
        content.mount(Static(f"  Total completions: {completion['total_completions']}"))

        # Monthly activity
        activity = db.get_wads_played_by_period("month")
        if activity:
            content.mount(Static(""))
            content.mount(Static("[bold]Monthly Activity[/bold]", classes="stats-section"))

            table = DataTable(id="activity-table")
            content.mount(table)
            table.add_column("Period", key="period", width=10)
            table.add_column("WADs", key="wads", width=8)
            table.add_column("Sessions", key="sessions", width=10)
            table.add_column("Playtime", key="playtime", width=12)
            table.cursor_type = "row"

            for row in activity[:12]:  # Show last 12 months
                table.add_row(
                    row["period"],
                    str(row["wad_count"]),
                    str(row["session_count"]),
                    format_duration(row["total_playtime"]),
                )

    def action_back(self) -> None:
        """Go back."""
        self.app.pop_screen()

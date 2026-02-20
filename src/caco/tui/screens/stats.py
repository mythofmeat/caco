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
        snap = db.get_stats_snapshot("month")

        # Overview
        content.mount(Static("[bold]Overview[/bold]", classes="stats-section"))
        content.mount(Static(f"  Total WADs: {snap.total_wads}"))
        content.mount(Static(f"  Total sessions: {snap.total_sessions}"))
        content.mount(Static(f"  Total playtime: {format_duration(snap.total_playtime)}"))
        content.mount(Static(f"  WADs played: {snap.wads_with_sessions}"))

        # Status breakdown
        if snap.wads_by_status:
            content.mount(Static(""))
            content.mount(Static("[bold]By Status[/bold]", classes="stats-section"))
            for status, count in sorted(snap.wads_by_status.items()):
                content.mount(Static(f"  {status}: {count}"))

        # Completion stats
        content.mount(Static(""))
        content.mount(Static("[bold]Completion[/bold]", classes="stats-section"))
        content.mount(Static(f"  Played: {snap.played_wads}"))
        content.mount(Static(f"  Finished: {snap.finished_wads}"))
        rate_pct = f"{snap.completion_rate:.0%}"
        content.mount(Static(f"  Completion rate: {rate_pct}"))
        content.mount(Static(f"  Total completions: {snap.total_completions}"))

        # Monthly activity
        if snap.activity:
            content.mount(Static(""))
            content.mount(Static("[bold]Monthly Activity[/bold]", classes="stats-section"))

            table: DataTable[str] = DataTable(id="activity-table")
            content.mount(table)
            table.add_column("Period", key="period", width=10)
            table.add_column("WADs", key="wads", width=8)
            table.add_column("Sessions", key="sessions", width=10)
            table.add_column("Playtime", key="playtime", width=12)
            table.cursor_type = "row"

            for row in snap.activity[:12]:  # Show last 12 months
                table.add_row(
                    row["period"],
                    str(row["wad_count"]),
                    str(row["session_count"]),
                    format_duration(row["total_playtime"]),
                )

    def action_back(self) -> None:
        """Go back."""
        self.app.pop_screen()

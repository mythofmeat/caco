"""Session history screen."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Static

from caco import db
from caco.player import format_duration


class SessionsScreen(Screen):
    """Session history viewer for a WAD."""

    BINDINGS = [
        Binding("q", "back", "Back", show=True),
        Binding("escape", "back", "Back", show=False),
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
    ]

    def __init__(self, wad_id: int) -> None:
        super().__init__()
        self.wad_id = wad_id

    def compose(self) -> ComposeResult:
        yield Static("", id="sessions-header")
        yield DataTable(id="sessions-table")
        yield Footer()

    def on_mount(self) -> None:
        """Load session data."""
        wad = db.get_wad(self.wad_id)
        header = self.query_one("#sessions-header", Static)

        if not wad:
            header.update("[red]WAD not found[/red]")
            return

        header.update(f"[bold]Session History: {wad['title']}[/bold]")

        # Set up table
        table = self.query_one("#sessions-table", DataTable)
        table.cursor_type = "row"
        table.zebra_stripes = True

        table.add_column("Date", key="date", width=12)
        table.add_column("Started", key="started", width=10)
        table.add_column("Duration", key="duration", width=12)
        table.add_column("Sourceport", key="sourceport", width=15)

        # Load sessions
        sessions = db.get_sessions(self.wad_id)

        if not sessions:
            table.add_row("No sessions", "", "", "")
            return

        for session in sessions:
            # Parse started_at
            started_at = session.get("started_at", "")
            date = started_at[:10] if started_at else "-"
            time = started_at[11:16] if len(started_at) > 11 else "-"

            # Duration
            duration = session.get("duration_seconds")
            duration_str = format_duration(duration) if duration else "-"

            # Sourceport
            sourceport = session.get("sourceport") or "-"

            table.add_row(date, time, duration_str, sourceport)

        table.focus()

    def action_back(self) -> None:
        """Go back."""
        self.app.pop_screen()

    def action_cursor_down(self) -> None:
        """Move cursor down."""
        table = self.query_one("#sessions-table", DataTable)
        if table.cursor_row is not None and table.cursor_row < table.row_count - 1:
            table.move_cursor(row=table.cursor_row + 1)

    def action_cursor_up(self) -> None:
        """Move cursor up."""
        table = self.query_one("#sessions-table", DataTable)
        if table.cursor_row is not None and table.cursor_row > 0:
            table.move_cursor(row=table.cursor_row - 1)

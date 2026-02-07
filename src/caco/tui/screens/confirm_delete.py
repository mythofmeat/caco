"""Confirmation screen for WAD deletion."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Vertical
from textual.screen import ModalScreen
from textual.widgets import Static

from caco import db
from caco.player import format_duration


class ConfirmDeleteScreen(ModalScreen[int | None]):
    """Modal confirmation dialog for deleting a WAD."""

    BINDINGS = [
        Binding("y", "confirm", "Yes", show=True),
        Binding("n", "cancel", "No", show=True),
        Binding("escape", "cancel", "Cancel", show=False),
    ]

    DEFAULT_CSS = """
    ConfirmDeleteScreen {
        align: center middle;
    }

    ConfirmDeleteScreen #delete-dialog {
        width: 60;
        height: auto;
        max-height: 20;
        background: $surface;
        border: thick $error;
        padding: 1 2;
    }

    ConfirmDeleteScreen #delete-title {
        text-style: bold;
        color: $error;
        text-align: center;
        margin-bottom: 1;
    }

    ConfirmDeleteScreen #delete-info {
        margin-bottom: 1;
    }

    ConfirmDeleteScreen #delete-stats {
        color: $text-muted;
        margin-bottom: 1;
    }

    ConfirmDeleteScreen #delete-hint {
        color: $text-muted;
        text-align: center;
    }

    ConfirmDeleteScreen #delete-prompt {
        text-align: center;
        margin-top: 1;
    }
    """

    def __init__(self, wad_id: int) -> None:
        super().__init__()
        self.wad_id = wad_id

    def compose(self) -> ComposeResult:
        with Vertical(id="delete-dialog"):
            yield Static("Delete WAD?", id="delete-title")
            yield Static("", id="delete-info")
            yield Static("", id="delete-stats")
            yield Static("(Moves to trash. Use T to view trash, u to restore.)", id="delete-hint")
            yield Static("[y] Yes  [n] No", id="delete-prompt")

    def on_mount(self) -> None:
        """Load WAD details for the confirmation dialog."""
        wad = db.get_wad(self.wad_id)
        if not wad:
            self.query_one("#delete-info", Static).update("[red]WAD not found[/red]")
            return

        self.query_one("#delete-info", Static).update(
            f"[bold]{wad['title']}[/bold]"
            + (f" by {wad['author']}" if wad.get("author") else "")
        )

        stats = db.get_wad_stats(self.wad_id)
        stats_parts = []
        if stats["session_count"]:
            stats_parts.append(f"{stats['session_count']} sessions")
        if stats["total_playtime"]:
            stats_parts.append(f"{format_duration(stats['total_playtime'])} played")
        if stats_parts:
            self.query_one("#delete-stats", Static).update(" | ".join(stats_parts))

    def action_confirm(self) -> None:
        """Confirm deletion."""
        self.dismiss(self.wad_id)

    def action_cancel(self) -> None:
        """Cancel deletion."""
        self.dismiss(None)

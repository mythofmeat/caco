"""Main TUI application."""

from pathlib import Path

from textual.app import App
from textual.binding import Binding

from caco.tui.screens.library import LibraryScreen


class CacoApp(App):
    """Caco WAD Library Manager TUI."""

    TITLE = "Caco"
    CSS_PATH = Path(__file__).parent / "styles.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit", show=True),
    ]

    def on_mount(self) -> None:
        """Set up the app on mount."""
        self.push_screen(LibraryScreen())

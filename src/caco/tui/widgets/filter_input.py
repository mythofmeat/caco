"""Filter input widget for search."""

from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.message import Message
from textual.widgets import Input, Static


class FilterInput(Horizontal):
    """Search/filter input bar."""

    class QueryChanged(Message):
        """Fired when the query changes."""

        def __init__(self, query: str) -> None:
            super().__init__()
            self.query = query

    class Escaped(Message):
        """Fired when escape is pressed."""

    def compose(self) -> ComposeResult:
        yield Static("/", id="filter-prompt")
        yield Input(placeholder="Filter (beets-style query)", id="filter-input")

    def on_mount(self) -> None:
        """Set up initial state."""
        self.display = False  # Hidden by default

    def show(self) -> None:
        """Show the filter input and focus it."""
        self.display = True
        input_widget = self.query_one("#filter-input", Input)
        input_widget.focus()

    def hide(self) -> None:
        """Hide the filter input."""
        self.display = False

    def clear(self) -> None:
        """Clear the input and hide."""
        input_widget = self.query_one("#filter-input", Input)
        input_widget.value = ""
        self.hide()
        self.post_message(self.QueryChanged(""))

    def get_query(self) -> str:
        """Get the current query string."""
        input_widget = self.query_one("#filter-input", Input)
        return input_widget.value

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle input submission (Enter key)."""
        self.post_message(self.QueryChanged(event.value))
        self.hide()

    def on_key(self, event) -> None:
        """Handle key events."""
        if event.key == "escape":
            self.clear()
            self.post_message(self.Escaped())
            event.stop()

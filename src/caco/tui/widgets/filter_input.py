"""Filter input widget for search."""

import asyncio

from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.message import Message
from textual.widgets import Input, Static


class FilterInput(Horizontal):
    """Search/filter input bar with live filtering.

    Can operate in two modes:
    - Hidden by default (legacy LibraryScreen behavior)
    - Always visible (new LibraryPane behavior in tabs)
    """

    DEBOUNCE_DELAY = 0.15  # 150ms debounce

    class QueryChanged(Message):
        """Fired when the query changes."""

        def __init__(self, query: str) -> None:
            super().__init__()
            self.query = query

    class Escaped(Message):
        """Fired when escape is pressed."""

    class Submitted(Message):
        """Fired when Enter is pressed (filter submitted)."""

        def __init__(self, query: str) -> None:
            super().__init__()
            self.query = query

    def __init__(self, always_visible: bool = True, **kwargs) -> None:
        super().__init__(**kwargs)
        self._debounce_task: asyncio.Task | None = None
        self._wad_count: int = 0
        self._always_visible = always_visible

    def compose(self) -> ComposeResult:
        yield Static("/", id="filter-prompt")
        yield Input(placeholder="Filter (tag:, status:, author:, year:, ^negate)", id="filter-input")
        yield Static("", id="filter-count")

    def on_mount(self) -> None:
        """Set up initial state."""
        # Only hide by default if not always_visible mode
        if not self._always_visible:
            self.display = False

    def show(self) -> None:
        """Show the filter input and focus it."""
        self.display = True
        input_widget = self.query_one("#filter-input", Input)
        input_widget.focus()

    def hide(self) -> None:
        """Hide the filter input (only if not always_visible)."""
        if not self._always_visible:
            self.display = False

    def clear(self) -> None:
        """Clear the input and optionally hide."""
        if self._debounce_task:
            self._debounce_task.cancel()
            self._debounce_task = None
        input_widget = self.query_one("#filter-input", Input)
        input_widget.value = ""
        self.hide()
        self.post_message(self.QueryChanged(""))

    def get_query(self) -> str:
        """Get the current query string."""
        input_widget = self.query_one("#filter-input", Input)
        return input_widget.value

    def set_wad_count(self, count: int, label: str | None = None) -> None:
        """Update the displayed WAD count."""
        self._wad_count = count
        count_widget = self.query_one("#filter-count", Static)
        if label:
            count_widget.update(f" ({count} {label})")
        else:
            count_widget.update(f" ({count})")

    def on_input_changed(self, event: Input.Changed) -> None:
        """Handle input changes for live filtering with debounce."""
        if self._debounce_task:
            self._debounce_task.cancel()
        self._debounce_task = asyncio.create_task(self._debounced_query(event.value))

    async def _debounced_query(self, query: str) -> None:
        """Fire query after debounce delay."""
        await asyncio.sleep(self.DEBOUNCE_DELAY)
        self.post_message(self.QueryChanged(query))

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle input submission (Enter key)."""
        # Cancel any pending debounce since we're submitting now
        if self._debounce_task:
            self._debounce_task.cancel()
            self._debounce_task = None
        self.post_message(self.QueryChanged(event.value))
        self.post_message(self.Submitted(event.value))
        # Only hide if not always_visible
        if not self._always_visible:
            self.hide()

    def on_key(self, event) -> None:
        """Handle key events."""
        if event.key == "escape":
            self.clear()
            self.post_message(self.Escaped())
            event.stop()

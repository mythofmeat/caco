"""Base search pane widget with shared structure for idgames and doomwiki."""

from __future__ import annotations

from abc import abstractmethod
from typing import Any

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Button, DataTable, Input, Static
from textual.worker import Worker, get_current_worker


class BaseSearchPane(Widget):
    """Base search pane with results table, preview panel, and import functionality.

    Subclasses must implement:
        - search_placeholder: str property
        - _configure_columns(table): set up DataTable columns
        - _search_api(query): perform the actual API search
        - _format_row(entry): return (row_values, key) tuple for a result entry
        - _update_preview(entry): update preview panel for selected entry
        - _get_display_name(entry): return display name for an entry
        - _do_import(entry): perform the import and return wad_id or None
    """

    BINDINGS = [
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
        Binding("enter", "import_wad", "Import", show=True),
        Binding("slash", "focus_search", "Search", show=True, key_display="/"),
    ]

    class WadImported(Message):
        """Fired when a WAD is successfully imported."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    DEFAULT_CSS = """
    BaseSearchPane, BaseSearchPane > * {
        height: 100%;
        width: 100%;
    }

    BaseSearchPane #search-header {
        height: 3;
        width: 100%;
        padding: 0 1;
        align: left middle;
    }

    BaseSearchPane #search-input {
        width: 1fr;
    }

    BaseSearchPane #search-btn {
        margin-left: 1;
    }

    BaseSearchPane #search-content {
        height: 1fr;
    }

    BaseSearchPane #results-container {
        width: 2fr;
        height: 100%;
    }

    BaseSearchPane #preview-container {
        width: 1fr;
        height: 100%;
        border-left: solid $primary;
        padding: 1 2;
    }

    BaseSearchPane #preview-title {
        text-style: bold;
        margin-bottom: 1;
    }

    BaseSearchPane #preview-author {
        color: $text-muted;
        margin-bottom: 1;
    }

    BaseSearchPane #preview-extra {
        margin-bottom: 1;
    }

    BaseSearchPane #preview-desc {
        color: $text;
    }

    BaseSearchPane #search-status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }

    BaseSearchPane DataTable {
        height: 100%;
    }
    """

    @property
    @abstractmethod
    def search_placeholder(self) -> str:
        """Placeholder text for the search input."""
        ...

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._results: list[Any] = []
        self._current_worker: Worker | None = None

    def compose(self) -> ComposeResult:
        with Horizontal(id="search-header"):
            yield Input(
                placeholder=self.search_placeholder,
                id="search-input",
            )
            yield Button("Search", id="search-btn", variant="primary")
        with Horizontal(id="search-content"):
            with Vertical(id="results-container"):
                yield DataTable(id="results-table")
            with Vertical(id="preview-container"):
                yield Static("", id="preview-title")
                yield Static("", id="preview-author")
                yield Static("", id="preview-extra")
                yield Static("", id="preview-desc")
        yield Static("/ Search  Enter Import  j/k Navigate  1-5 Source", id="search-status")

    def on_mount(self) -> None:
        """Set up the results table."""
        table = self.query_one("#results-table", DataTable)
        table.cursor_type = "row"
        table.zebra_stripes = True
        self._configure_columns(table)

    @abstractmethod
    def _configure_columns(self, table: DataTable) -> None:
        """Add columns to the results DataTable."""
        ...

    # ---- Search ----

    def action_focus_search(self) -> None:
        self.query_one("#search-input", Input).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "search-input":
            self._do_search(event.value)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "search-btn":
            query = self.query_one("#search-input", Input).value
            self._do_search(query)

    def _do_search(self, query: str) -> None:
        if not query.strip():
            self.notify("Please enter a search term", severity="warning")
            return
        if self._current_worker and self._current_worker.is_running:
            self._current_worker.cancel()
        # Clear results and show loading state
        table = self.query_one("#results-table", DataTable)
        table.clear()
        self._results = []
        self._clear_preview()
        status = self.query_one("#search-status", Static)
        status.update(f"Searching for '{query}'...")
        self._current_worker = self.run_worker(
            self._run_search(query), exclusive=True,
        )

    async def _run_search(self, query: str) -> list:
        worker = get_current_worker()
        try:
            if worker.is_cancelled:
                return []
            return self._search_api(query)
        except Exception as e:
            self.notify(f"Search error: {e}", severity="error")
            return []

    @abstractmethod
    def _search_api(self, query: str) -> list:
        """Perform the actual API search. Called in worker thread."""
        ...

    def on_worker_state_changed(self, event: Worker.StateChanged) -> None:
        if event.state.name == "SUCCESS" and event.worker.result is not None:
            self._display_results(event.worker.result)

    def _display_results(self, results: list) -> None:
        self._results = results
        table = self.query_one("#results-table", DataTable)
        table.clear()
        status = self.query_one("#search-status", Static)

        if not results:
            status.update("No results found")
            self._clear_preview()
            return

        for entry in results:
            row_values, key = self._format_row(entry)
            table.add_row(*row_values, key=key)

        status.update(f"Found {len(results)} results  |  Enter Import  j/k Navigate  1-5 Source")

        if results:
            table.focus()
            self._update_preview(results[0])

    @abstractmethod
    def _format_row(self, entry) -> tuple[tuple, str]:
        """Format a result entry into (row_values_tuple, row_key)."""
        ...

    # ---- Preview ----

    def _clear_preview(self) -> None:
        self.query_one("#preview-title", Static).update("")
        self.query_one("#preview-author", Static).update("")
        self.query_one("#preview-extra", Static).update("")
        self.query_one("#preview-desc", Static).update("")

    @abstractmethod
    def _update_preview(self, entry) -> None:
        """Update the preview panel with entry details."""
        ...

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        if event.cursor_row is not None and 0 <= event.cursor_row < len(self._results):
            self._update_preview(self._results[event.cursor_row])

    # ---- Navigation ----

    def action_cursor_down(self) -> None:
        table = self.query_one("#results-table", DataTable)
        if table.cursor_row is not None and table.cursor_row < len(self._results) - 1:
            table.move_cursor(row=table.cursor_row + 1)

    def action_cursor_up(self) -> None:
        table = self.query_one("#results-table", DataTable)
        if table.cursor_row is not None and table.cursor_row > 0:
            table.move_cursor(row=table.cursor_row - 1)

    # ---- Import ----

    def action_import_wad(self) -> None:
        table = self.query_one("#results-table", DataTable)
        if table.cursor_row is None or table.cursor_row >= len(self._results):
            self.notify("No WAD selected", severity="warning")
            return
        entry = self._results[table.cursor_row]
        self._start_import(entry)

    def _start_import(self, entry) -> None:
        status = self.query_one("#search-status", Static)
        status.update(f"Importing {self._get_display_name(entry)}...")
        self.run_worker(self._run_import(entry), exclusive=False)

    @abstractmethod
    def _get_display_name(self, entry) -> str:
        """Get display name for an entry (for status messages)."""
        ...

    async def _run_import(self, entry) -> None:
        try:
            wad_id = self._do_import(entry)
        except Exception as e:
            self.notify(f"Import failed: {e}", severity="error")
            status = self.query_one("#search-status", Static)
            status.update(f"Import failed: {e}")
            return

        if wad_id is None:
            # Duplicate detected by subclass
            return

        name = self._get_display_name(entry)
        self.notify(f"Imported: {name} (ID: {wad_id})")
        status = self.query_one("#search-status", Static)
        status.update(f"Successfully imported as ID {wad_id}")
        self.post_message(self.WadImported(wad_id))

    @abstractmethod
    def _do_import(self, entry) -> int | None:
        """Perform the import. Return wad_id on success, None if duplicate.

        Should check for duplicates and call the appropriate source adapter.
        """
        ...

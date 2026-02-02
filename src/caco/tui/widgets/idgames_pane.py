"""idgames search pane widget for the TUI."""

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Button, DataTable, Input, Static
from textual.worker import Worker, get_current_worker

from caco.idgames.models import FileEntry
from caco.sources.idgames import IdgamesSource


class IdgamesSearchPane(Widget):
    """Search pane for finding and importing WADs from idgames archive."""

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
    IdgamesSearchPane {
        height: 100%;
        width: 100%;
    }

    IdgamesSearchPane #search-header {
        height: 3;
        width: 100%;
        padding: 0 1;
        align: left middle;
    }

    IdgamesSearchPane #search-input {
        width: 1fr;
    }

    IdgamesSearchPane #search-btn {
        margin-left: 1;
    }

    IdgamesSearchPane #search-content {
        height: 1fr;
    }

    IdgamesSearchPane #results-container {
        width: 2fr;
        height: 100%;
    }

    IdgamesSearchPane #preview-container {
        width: 1fr;
        height: 100%;
        border-left: solid $primary;
        padding: 1 2;
    }

    IdgamesSearchPane #preview-title {
        text-style: bold;
        margin-bottom: 1;
    }

    IdgamesSearchPane #preview-author {
        color: $text-muted;
        margin-bottom: 1;
    }

    IdgamesSearchPane #preview-rating {
        margin-bottom: 1;
    }

    IdgamesSearchPane #preview-desc {
        color: $text;
    }

    IdgamesSearchPane #search-status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }

    IdgamesSearchPane DataTable {
        height: 100%;
    }
    """

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._results: list[FileEntry] = []
        self._result_id_to_row: dict[int, int] = {}
        self._current_worker: Worker | None = None

    def compose(self) -> ComposeResult:
        with Horizontal(id="search-header"):
            yield Input(
                placeholder="Search idgames archive...",
                id="search-input",
            )
            yield Button("Search", id="search-btn", variant="primary")
        with Horizontal(id="search-content"):
            with Vertical(id="results-container"):
                yield DataTable(id="results-table")
            with Vertical(id="preview-container"):
                yield Static("", id="preview-title")
                yield Static("", id="preview-author")
                yield Static("", id="preview-rating")
                yield Static("", id="preview-desc")
        yield Static("Enter a search term to find WADs", id="search-status")

    def on_mount(self) -> None:
        """Set up the results table."""
        table = self.query_one("#results-table", DataTable)
        table.cursor_type = "row"
        table.zebra_stripes = True

        table.add_column("ID", key="id", width=8)
        table.add_column("Title", key="title", width=30)
        table.add_column("Author", key="author", width=20)
        table.add_column("Rating", key="rating", width=8)
        table.add_column("Date", key="date", width=12)

    def action_focus_search(self) -> None:
        """Focus the search input."""
        self.query_one("#search-input", Input).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle search submission."""
        if event.input.id == "search-input":
            self._do_search(event.value)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle search button click."""
        if event.button.id == "search-btn":
            query = self.query_one("#search-input", Input).value
            self._do_search(query)

    def _do_search(self, query: str) -> None:
        """Perform the search."""
        if not query.strip():
            self.notify("Please enter a search term", severity="warning")
            return

        # Cancel any existing search
        if self._current_worker and self._current_worker.is_running:
            self._current_worker.cancel()

        status = self.query_one("#search-status", Static)
        status.update(f"Searching for '{query}'...")

        self._current_worker = self.run_worker(
            self._search_idgames(query),
            exclusive=True,
        )

    async def _search_idgames(self, query: str) -> list[FileEntry]:
        """Search idgames in a worker thread."""
        worker = get_current_worker()

        try:
            with IdgamesSource() as source:
                if worker.is_cancelled:
                    return []
                results = source.search(query)
                return results
        except Exception as e:
            self.notify(f"Search error: {e}", severity="error")
            return []

    def on_worker_state_changed(self, event: Worker.StateChanged) -> None:
        """Handle search completion."""
        if event.state.name == "SUCCESS" and event.worker.result is not None:
            results = event.worker.result
            self._display_results(results)

    def _display_results(self, results: list[FileEntry]) -> None:
        """Display search results in the table."""
        self._results = results
        self._result_id_to_row.clear()

        table = self.query_one("#results-table", DataTable)
        table.clear()

        status = self.query_one("#search-status", Static)

        if not results:
            status.update("No results found")
            self._clear_preview()
            return

        for i, entry in enumerate(results):
            self._result_id_to_row[entry.id] = i

            # Format rating
            if entry.rating > 0:
                rating_text = f"{entry.rating:.1f}"
            else:
                rating_text = "-"

            # Format date (YYYY-MM-DD)
            date_text = entry.date[:10] if entry.date else "-"

            table.add_row(
                str(entry.id),
                entry.title or entry.filename,
                entry.author or "-",
                rating_text,
                date_text,
                key=str(entry.id),
            )

        status.update(f"Found {len(results)} results - Press Enter to import")

        # Select first row and update preview
        if results:
            table.focus()
            self._update_preview(results[0])

    def _clear_preview(self) -> None:
        """Clear the preview panel."""
        self.query_one("#preview-title", Static).update("")
        self.query_one("#preview-author", Static).update("")
        self.query_one("#preview-rating", Static).update("")
        self.query_one("#preview-desc", Static).update("")

    def _update_preview(self, entry: FileEntry) -> None:
        """Update the preview panel with entry details."""
        title = self.query_one("#preview-title", Static)
        title.update(entry.title or entry.filename)

        author = self.query_one("#preview-author", Static)
        author_parts = []
        if entry.author:
            author_parts.append(entry.author)
        if entry.date:
            year = entry.date.split("-")[0] if "-" in entry.date else entry.date[:4]
            author_parts.append(f"({year})")
        author.update(" ".join(author_parts) if author_parts else "Unknown author")

        rating = self.query_one("#preview-rating", Static)
        if entry.rating > 0:
            stars_full = int(entry.rating)
            stars = "★" * stars_full + "☆" * (5 - stars_full)
            rating.update(f"Rating: {stars} ({entry.rating:.1f}, {entry.votes} votes)")
        else:
            rating.update("Rating: Not rated")

        desc = self.query_one("#preview-desc", Static)
        description = entry.description or "No description available"
        # Truncate long descriptions
        if len(description) > 500:
            description = description[:500] + "..."
        desc.update(description)

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        """Update preview when row selection changes."""
        if event.cursor_row is not None and 0 <= event.cursor_row < len(self._results):
            self._update_preview(self._results[event.cursor_row])

    def action_cursor_down(self) -> None:
        """Move cursor down."""
        table = self.query_one("#results-table", DataTable)
        if table.cursor_row is not None and table.cursor_row < len(self._results) - 1:
            table.move_cursor(row=table.cursor_row + 1)

    def action_cursor_up(self) -> None:
        """Move cursor up."""
        table = self.query_one("#results-table", DataTable)
        if table.cursor_row is not None and table.cursor_row > 0:
            table.move_cursor(row=table.cursor_row - 1)

    def action_import_wad(self) -> None:
        """Import the selected WAD."""
        table = self.query_one("#results-table", DataTable)

        if table.cursor_row is None or table.cursor_row >= len(self._results):
            self.notify("No WAD selected", severity="warning")
            return

        entry = self._results[table.cursor_row]
        self._import_entry(entry)

    def _import_entry(self, entry: FileEntry) -> None:
        """Import a WAD entry into the library."""
        status = self.query_one("#search-status", Static)
        status.update(f"Importing {entry.title or entry.filename}...")

        self.run_worker(self._do_import(entry), exclusive=False)

    async def _do_import(self, entry: FileEntry) -> None:
        """Perform the import in a worker."""
        from caco import db

        # Check for duplicates
        existing = db.find_duplicate(
            source_type=db.SourceType.IDGAMES,
            source_id=str(entry.id),
            filename=entry.filename,
            author=entry.author,
        )

        if existing:
            self.notify(
                f"Already in library: {existing['title']} (ID: {existing['id']})",
                severity="warning",
            )
            status = self.query_one("#search-status", Static)
            status.update("WAD already exists in library")
            return

        try:
            with IdgamesSource() as source:
                wad_id = source.import_wad(entry)

            self.notify(f"Imported: {entry.title or entry.filename} (ID: {wad_id})")
            status = self.query_one("#search-status", Static)
            status.update(f"Successfully imported as ID {wad_id}")

            # Notify parent to refresh library panes
            self.post_message(self.WadImported(wad_id))

        except Exception as e:
            self.notify(f"Import failed: {e}", severity="error")
            status = self.query_one("#search-status", Static)
            status.update(f"Import failed: {e}")

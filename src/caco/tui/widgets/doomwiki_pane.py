"""Doomwiki search pane widget for the TUI."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Button, DataTable, Input, Static
from textual.worker import Worker, get_current_worker

from caco.doomwiki.models import WikiEntry
from caco.sources.doomwiki import DoomwikiSource


class DoomwikiSearchPane(Widget):
    """Search pane for finding and importing WADs from Doom Wiki."""

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
    DoomwikiSearchPane {
        height: 100%;
        width: 100%;
    }

    DoomwikiSearchPane #search-header {
        height: 3;
        width: 100%;
        padding: 0 1;
        align: left middle;
    }

    DoomwikiSearchPane #search-input {
        width: 1fr;
    }

    DoomwikiSearchPane #search-btn {
        margin-left: 1;
    }

    DoomwikiSearchPane #search-content {
        height: 1fr;
    }

    DoomwikiSearchPane #results-container {
        width: 2fr;
        height: 100%;
    }

    DoomwikiSearchPane #preview-container {
        width: 1fr;
        height: 100%;
        border-left: solid $primary;
        padding: 1 2;
    }

    DoomwikiSearchPane #preview-title {
        text-style: bold;
        margin-bottom: 1;
    }

    DoomwikiSearchPane #preview-author {
        color: $text-muted;
        margin-bottom: 1;
    }

    DoomwikiSearchPane #preview-tech {
        margin-bottom: 1;
    }

    DoomwikiSearchPane #preview-desc {
        color: $text;
    }

    DoomwikiSearchPane #search-status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }

    DoomwikiSearchPane DataTable {
        height: 100%;
    }
    """

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._results: list[WikiEntry] = []
        self._current_worker: Worker | None = None

    def compose(self) -> ComposeResult:
        with Horizontal(id="search-header"):
            yield Input(
                placeholder="Search Doom Wiki...",
                id="search-input",
            )
            yield Button("Search", id="search-btn", variant="primary")
        with Horizontal(id="search-content"):
            with Vertical(id="results-container"):
                yield DataTable(id="results-table")
            with Vertical(id="preview-container"):
                yield Static("", id="preview-title")
                yield Static("", id="preview-author")
                yield Static("", id="preview-tech")
                yield Static("", id="preview-desc")
        yield Static("/ Search  Enter Import  j/k Navigate  1-5 Source", id="search-status")

    def on_mount(self) -> None:
        """Set up the results table."""
        table = self.query_one("#results-table", DataTable)
        table.cursor_type = "row"
        table.zebra_stripes = True

        table.add_column("Title", key="title", width=30)
        table.add_column("Author", key="author", width=20)
        table.add_column("Year", key="year", width=6)
        table.add_column("IWAD", key="iwad", width=12)
        table.add_column("Port", key="port", width=15)

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
        status.update(f"Searching Doom Wiki for '{query}'...")

        self._current_worker = self.run_worker(
            self._search_doomwiki(query),
            exclusive=True,
        )

    async def _search_doomwiki(self, query: str) -> list[WikiEntry]:
        """Search Doom Wiki in a worker thread."""
        worker = get_current_worker()

        try:
            with DoomwikiSource() as source:
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

    def _display_results(self, results: list[WikiEntry]) -> None:
        """Display search results in the table."""
        self._results = results

        table = self.query_one("#results-table", DataTable)
        table.clear()

        status = self.query_one("#search-status", Static)

        if not results:
            status.update("No results found")
            self._clear_preview()
            return

        for entry in results:
            # Format year
            year_text = str(entry.year) if entry.year else "-"

            # Truncate long values
            iwad = entry.iwad[:12] if entry.iwad else "-"
            port = entry.port[:15] if entry.port else "-"

            table.add_row(
                entry.display_name[:30] if len(entry.display_name) > 30 else entry.display_name,
                entry.author[:20] if entry.author and len(entry.author) > 20 else (entry.author or "-"),
                year_text,
                iwad,
                port,
                key=str(entry.page_id),
            )

        status.update(f"Found {len(results)} results  |  Enter Import  j/k Navigate  1-5 Source")

        # Select first row and update preview
        if results:
            table.focus()
            self._update_preview(results[0])

    def _clear_preview(self) -> None:
        """Clear the preview panel."""
        self.query_one("#preview-title", Static).update("")
        self.query_one("#preview-author", Static).update("")
        self.query_one("#preview-tech", Static).update("")
        self.query_one("#preview-desc", Static).update("")

    def _update_preview(self, entry: WikiEntry) -> None:
        """Update the preview panel with entry details."""
        title = self.query_one("#preview-title", Static)
        title.update(entry.display_name)

        author = self.query_one("#preview-author", Static)
        author_parts = []
        if entry.author:
            author_parts.append(entry.author)
        if entry.year:
            author_parts.append(f"({entry.year})")
        author.update(" ".join(author_parts) if author_parts else "Unknown author")

        # Technical info (IWAD + Port)
        tech = self.query_one("#preview-tech", Static)
        tech_parts = []
        if entry.iwad:
            tech_parts.append(f"IWAD: {entry.iwad}")
        if entry.port:
            tech_parts.append(f"Port: {entry.port}")
        tech.update(" | ".join(tech_parts) if tech_parts else "")

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

    def _import_entry(self, entry: WikiEntry) -> None:
        """Import a WAD entry into the library."""
        status = self.query_one("#search-status", Static)
        status.update(f"Importing {entry.display_name}...")

        self.run_worker(self._do_import(entry), exclusive=False)

    async def _do_import(self, entry: WikiEntry) -> None:
        """Perform the import in a worker."""
        from caco import db

        # Check for duplicates
        existing = db.find_duplicate(
            source_type=db.SourceType.DOOMWIKI,
            source_id=str(entry.page_id),
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
            with DoomwikiSource() as source:
                wad_id = source.import_wad(entry)

            self.notify(f"Imported: {entry.display_name} (ID: {wad_id})")
            status = self.query_one("#search-status", Static)
            status.update(f"Successfully imported as ID {wad_id}")

            # Notify parent to refresh library panes
            self.post_message(self.WadImported(wad_id))

        except Exception as e:
            self.notify(f"Import failed: {e}", severity="error")
            status = self.query_one("#search-status", Static)
            status.update(f"Import failed: {e}")

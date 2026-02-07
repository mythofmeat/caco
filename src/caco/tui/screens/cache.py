"""Cache management screen."""

from pathlib import Path

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Static

from caco import db


def _format_size(size_bytes: int) -> str:
    """Format bytes into human-readable size."""
    if size_bytes < 1024:
        return f"{size_bytes} B"
    elif size_bytes < 1024 * 1024:
        return f"{size_bytes / 1024:.1f} KB"
    elif size_bytes < 1024 * 1024 * 1024:
        return f"{size_bytes / (1024 * 1024):.1f} MB"
    else:
        return f"{size_bytes / (1024 * 1024 * 1024):.2f} GB"


class CacheScreen(Screen):
    """Cache management screen showing cached WAD files."""

    BINDINGS = [
        Binding("q", "back", "Back", show=True),
        Binding("escape", "back", "Back", show=False),
        Binding("d", "clear_selected", "Clear", show=True),
        Binding("D", "clear_all", "Clear All", show=True, key_display="D"),
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
    ]

    DEFAULT_CSS = """
    CacheScreen {
        padding: 1 2;
    }

    CacheScreen #cache-header {
        text-style: bold;
        margin-bottom: 1;
    }

    CacheScreen #cache-table {
        height: 1fr;
    }

    CacheScreen #cache-status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }
    """

    def compose(self) -> ComposeResult:
        yield Static("[bold]Cache Management[/bold]", id="cache-header")
        yield DataTable(id="cache-table")
        yield Static("", id="cache-status")
        yield Footer()

    def on_mount(self) -> None:
        """Set up table and load cached WADs."""
        table = self.query_one("#cache-table", DataTable)
        table.cursor_type = "row"
        table.zebra_stripes = True
        table.add_column("ID", key="id", width=6)
        table.add_column("Title", key="title", width=30)
        table.add_column("Path", key="path", width=40)
        table.add_column("Size", key="size", width=10)
        self._load_cache()

    def _load_cache(self) -> None:
        """Load cached WADs into table."""
        table = self.query_one("#cache-table", DataTable)
        table.clear()

        self._cached = db.get_cached_wads()
        total_size = 0

        for wad in self._cached:
            path = Path(wad["cached_path"]) if wad.get("cached_path") else None
            if path and path.exists():
                size = path.stat().st_size
                total_size += size
                size_str = _format_size(size)
            else:
                size_str = "missing"

            table.add_row(
                str(wad["id"]),
                wad["title"],
                wad.get("cached_path", "-"),
                size_str,
                key=str(wad["id"]),
            )

        status = self.query_one("#cache-status", Static)
        status.update(f"{len(self._cached)} cached files | Total: {_format_size(total_size)} | d=Clear  D=Clear All")

    def action_clear_selected(self) -> None:
        """Clear cache for the selected WAD."""
        table = self.query_one("#cache-table", DataTable)
        if table.cursor_row is None or table.cursor_row >= len(self._cached):
            self.notify("No WAD selected", severity="warning")
            return

        wad = self._cached[table.cursor_row]
        wad_id = wad["id"]

        # Delete the file
        path = Path(wad["cached_path"]) if wad.get("cached_path") else None
        if path and path.exists():
            path.unlink()

        db.clear_cached_path(wad_id)
        self.notify(f"Cleared cache for {wad['title']}")
        self._load_cache()

    def action_clear_all(self) -> None:
        """Clear all cached WAD files."""
        for wad in self._cached:
            path = Path(wad["cached_path"]) if wad.get("cached_path") else None
            if path and path.exists():
                path.unlink()

        count = db.clear_all_cached_paths()
        self.notify(f"Cleared {count} cached files")
        self._load_cache()

    def action_cursor_down(self) -> None:
        """Move cursor down."""
        table = self.query_one("#cache-table", DataTable)
        if table.cursor_row is not None and table.cursor_row < len(self._cached) - 1:
            table.move_cursor(row=table.cursor_row + 1)

    def action_cursor_up(self) -> None:
        """Move cursor up."""
        table = self.query_one("#cache-table", DataTable)
        if table.cursor_row is not None and table.cursor_row > 0:
            table.move_cursor(row=table.cursor_row - 1)

    def action_back(self) -> None:
        """Go back."""
        self.app.pop_screen()

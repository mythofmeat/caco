"""IWAD and id24 WAD resource management screen."""

from pathlib import Path

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Input, Static, TabbedContent, TabPane

from caco import db


class ResourcesScreen(Screen):
    """Combined management screen for IWAD and id24 WAD registries."""

    BINDINGS = [
        Binding("q", "back", "Back", show=True),
        Binding("escape", "back", "Back", show=False),
        Binding("d", "remove_selected", "Remove", show=True),
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
    ]

    DEFAULT_CSS = """
    ResourcesScreen {
        padding: 1 2;
    }

    ResourcesScreen #resources-header {
        text-style: bold;
        margin-bottom: 1;
    }

    ResourcesScreen TabbedContent {
        height: 1fr;
    }

    ResourcesScreen DataTable {
        height: 1fr;
    }

    ResourcesScreen #import-path {
        margin-top: 1;
    }

    ResourcesScreen #resources-status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }
    """

    def __init__(self) -> None:
        super().__init__()
        self._iwads: list[dict] = []
        self._id24s: list[dict] = []

    def compose(self) -> ComposeResult:
        yield Static("[bold]Resources[/bold]", id="resources-header")
        with TabbedContent(id="resources-tabs"):
            with TabPane("IWADs", id="tab-iwads"):
                yield DataTable(id="iwad-table")
            with TabPane("id24 WADs", id="tab-id24"):
                yield DataTable(id="id24-table")
        yield Input(
            placeholder="Path to IWAD or id24 WAD file to import...",
            id="import-path",
        )
        yield Static("", id="resources-status")
        yield Footer()

    def on_mount(self) -> None:
        """Set up tables and load data."""
        # IWAD table
        iwad_table = self.query_one("#iwad-table", DataTable)
        iwad_table.cursor_type = "row"
        iwad_table.zebra_stripes = True
        iwad_table.add_column("Family", key="family", width=12)
        iwad_table.add_column("Variant", key="variant", width=12)
        iwad_table.add_column("Title", key="title", width=30)
        iwad_table.add_column("Path", key="path", width=40)

        # id24 table
        id24_table = self.query_one("#id24-table", DataTable)
        id24_table.cursor_type = "row"
        id24_table.zebra_stripes = True
        id24_table.add_column("Name", key="name", width=12)
        id24_table.add_column("Version", key="version", width=12)
        id24_table.add_column("Title", key="title", width=30)
        id24_table.add_column("Path", key="path", width=40)

        self._load_iwads()
        self._load_id24()

    def _load_iwads(self) -> None:
        """Load IWAD registry into table."""
        table = self.query_one("#iwad-table", DataTable)
        table.clear()

        self._iwads = db.get_all_iwads()

        # Determine preferred variant per family
        preferred: dict[str, str | None] = {}
        families = {row["family"] for row in self._iwads}
        for family in families:
            pref = db.get_iwad(family)
            if pref:
                preferred[family] = pref.get("variant")

        for row in self._iwads:
            family = row["family"]
            variant = row["variant"]
            is_preferred = preferred.get(family) == variant
            marker = " *" if is_preferred else ""
            table.add_row(
                family,
                f"{variant}{marker}",
                row.get("title") or "",
                row.get("path") or "",
            )

        self._update_status()

    def _load_id24(self) -> None:
        """Load id24 registry into table."""
        table = self.query_one("#id24-table", DataTable)
        table.clear()

        self._id24s = db.get_all_id24()

        for row in self._id24s:
            table.add_row(
                row.get("name") or "",
                row.get("version") or "",
                row.get("title") or "",
                row.get("path") or "",
            )

        self._update_status()

    def _update_status(self) -> None:
        """Update status bar text."""
        status = self.query_one("#resources-status", Static)
        status.update(
            f"{len(self._iwads)} IWAD(s) | {len(self._id24s)} id24 WAD(s) | d=Remove"
        )

    def _active_tab_is_iwad(self) -> bool:
        """Check if the IWAD tab is currently active."""
        tabs = self.query_one("#resources-tabs", TabbedContent)
        return tabs.active == "tab-iwads"

    def action_remove_selected(self) -> None:
        """Remove the selected IWAD or id24 entry."""
        if self._active_tab_is_iwad():
            table = self.query_one("#iwad-table", DataTable)
            if table.cursor_row is None or table.cursor_row >= len(self._iwads):
                self.notify("No IWAD selected", severity="warning")
                return

            row = self._iwads[table.cursor_row]
            family = row["family"]
            variant = row["variant"]

            removed_paths = db.remove_iwad_with_paths(family, variant)
            for p in removed_paths:
                path = Path(p)
                if path.exists():
                    path.unlink()

            self.notify(f"Removed IWAD: {family}/{variant}")
            self._load_iwads()
        else:
            table = self.query_one("#id24-table", DataTable)
            if table.cursor_row is None or table.cursor_row >= len(self._id24s):
                self.notify("No id24 WAD selected", severity="warning")
                return

            row = self._id24s[table.cursor_row]
            name = row["name"]

            removed_paths = db.remove_id24_with_paths(name)
            for p in removed_paths:
                path = Path(p)
                if path.exists():
                    path.unlink()

            self.notify(f"Removed id24: {name}")
            self._load_id24()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle file path submission for import."""
        if event.input.id != "import-path":
            return

        path_str = event.value.strip()
        if not path_str:
            return

        path = Path(path_str).expanduser().resolve()
        if not path.exists():
            self.notify(f"File not found: {path_str}", severity="error")
            return

        from caco.services.resource_service import register_iwad, register_id24

        result = register_iwad(path)
        if result:
            family, variant, title = result
            self.notify(f"Registered IWAD: {title} ({family}/{variant})")
            self._load_iwads()
            event.input.value = ""
            return

        result = register_id24(path)
        if result:
            name, version, title = result
            self.notify(f"Registered id24: {title} ({version})")
            self._load_id24()
            event.input.value = ""
            return

        self.notify("Not a recognized IWAD or id24 WAD", severity="warning")

    def action_cursor_down(self) -> None:
        """Move cursor down in the active table."""
        table = self._get_active_table()
        items = self._iwads if self._active_tab_is_iwad() else self._id24s
        if table.cursor_row is not None and table.cursor_row < len(items) - 1:
            table.move_cursor(row=table.cursor_row + 1)

    def action_cursor_up(self) -> None:
        """Move cursor up in the active table."""
        table = self._get_active_table()
        if table.cursor_row is not None and table.cursor_row > 0:
            table.move_cursor(row=table.cursor_row - 1)

    def _get_active_table(self) -> DataTable:
        """Get the DataTable for the currently active tab."""
        if self._active_tab_is_iwad():
            return self.query_one("#iwad-table", DataTable)
        return self.query_one("#id24-table", DataTable)

    def action_back(self) -> None:
        """Go back to the library."""
        self.app.pop_screen()

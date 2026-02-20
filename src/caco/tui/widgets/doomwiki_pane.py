"""Doomwiki search pane widget for the TUI."""

from textual.widgets import DataTable, Static

from caco.doomwiki.models import WikiEntry
from caco.services import ImportService
from caco.sources.doomwiki import DoomwikiSource
from caco.tui.widgets.base_search_pane import BaseSearchPane
from caco.utils import format_author_year, truncate


class DoomwikiSearchPane(BaseSearchPane):
    """Search pane for finding and importing WADs from Doom Wiki."""

    search_placeholder = "Search Doom Wiki..."

    DEFAULT_CSS = """
    DoomwikiSearchPane #preview-extra {
        color: $primary-lighten-2;
    }
    """

    def _configure_columns(self, table: DataTable) -> None:
        table.add_column("Title", key="title", width=30)
        table.add_column("Author", key="author", width=20)
        table.add_column("Year", key="year", width=6)
        table.add_column("IWAD", key="iwad", width=12)
        table.add_column("Port", key="port", width=15)

    def _search_api(self, query: str) -> list[WikiEntry]:
        with DoomwikiSource() as source:
            return source.search(query)

    def _format_row(self, entry: WikiEntry) -> tuple[tuple, str]:
        year_text = str(entry.year) if entry.year else "-"
        iwad = entry.iwad[:12] if entry.iwad else "-"
        port = entry.port[:15] if entry.port else "-"
        title = entry.display_name[:30] if len(entry.display_name) > 30 else entry.display_name
        author = entry.author[:20] if entry.author and len(entry.author) > 20 else (entry.author or "-")
        return (
            (title, author, year_text, iwad, port),
            str(entry.page_id),
        )

    def _get_display_name(self, entry: WikiEntry) -> str:
        return entry.display_name

    def _update_preview(self, entry: WikiEntry) -> None:
        self.query_one("#preview-title", Static).update(entry.display_name)

        self.query_one("#preview-author", Static).update(
            format_author_year(entry.author, entry.year)
        )

        # Technical info (IWAD + Port)
        tech_parts = []
        if entry.iwad:
            tech_parts.append(f"IWAD: {entry.iwad}")
        if entry.port:
            tech_parts.append(f"Port: {entry.port}")
        self.query_one("#preview-extra", Static).update(
            " | ".join(tech_parts) if tech_parts else ""
        )

        self.query_one("#preview-desc", Static).update(
            truncate(entry.description or "No description available", 500)
        )

    def _do_import(self, entry: WikiEntry) -> int | None:
        result = ImportService().import_doomwiki(entry)
        if result.is_duplicate:
            self.notify(
                f"Already in library: {result.duplicate_title} (ID: {result.duplicate_id})",
                severity="warning",
            )
            self.query_one("#search-status", Static).update("WAD already exists in library")
            return None
        if result.error:
            self.notify(f"Import failed: {result.error}", severity="error")
            return None
        return result.wad_id

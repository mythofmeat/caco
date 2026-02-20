"""idgames search pane widget for the TUI."""

from textual.widgets import DataTable, Static

from caco.idgames.models import FileEntry
from caco.services import ImportService
from caco.sources.idgames import IdgamesSource
from caco.tui.widgets.base_search_pane import BaseSearchPane
from caco.utils import format_author_year, format_rating, truncate


class IdgamesSearchPane(BaseSearchPane):
    """Search pane for finding and importing WADs from idgames archive."""

    search_placeholder = "Search idgames archive..."

    def _configure_columns(self, table: DataTable) -> None:
        table.add_column("ID", key="id", width=8)
        table.add_column("Title", key="title", width=30)
        table.add_column("Author", key="author", width=20)
        table.add_column("Rating", key="rating", width=8)
        table.add_column("Date", key="date", width=12)

    def _search_api(self, query: str) -> list[FileEntry]:
        with IdgamesSource() as source:
            return source.search(query)

    def _format_row(self, entry: FileEntry) -> tuple[tuple, str]:
        rating_text = f"{entry.rating:.1f}" if entry.rating > 0 else "-"
        date_text = entry.date[:10] if entry.date else "-"
        return (
            (str(entry.id), entry.title or entry.filename, entry.author or "-",
             rating_text, date_text),
            str(entry.id),
        )

    def _get_display_name(self, entry: FileEntry) -> str:
        return entry.title or entry.filename

    def _update_preview(self, entry: FileEntry) -> None:
        self.query_one("#preview-title", Static).update(
            entry.title or entry.filename
        )

        year = None
        if entry.date:
            year = entry.date.split("-")[0] if "-" in entry.date else entry.date[:4]
        self.query_one("#preview-author", Static).update(
            format_author_year(entry.author, year)
        )

        extra = self.query_one("#preview-extra", Static)
        if entry.rating > 0:
            stars = format_rating(int(entry.rating))
            extra.update(f"Rating: {stars} ({entry.rating:.1f}, {entry.votes} votes)")
        else:
            extra.update("Rating: Not rated")

        self.query_one("#preview-desc", Static).update(
            truncate(entry.description or "No description available", 500)
        )

    def _do_import(self, entry: FileEntry) -> int | None:
        result = ImportService().import_idgames(entry)
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

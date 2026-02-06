"""idgames search pane widget for the TUI."""

from textual.widgets import DataTable, Static

from caco import db
from caco.idgames.models import FileEntry
from caco.sources.idgames import IdgamesSource
from caco.tui.widgets.base_search_pane import BaseSearchPane


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

        author_parts = []
        if entry.author:
            author_parts.append(entry.author)
        if entry.date:
            year = entry.date.split("-")[0] if "-" in entry.date else entry.date[:4]
            author_parts.append(f"({year})")
        self.query_one("#preview-author", Static).update(
            " ".join(author_parts) if author_parts else "Unknown author"
        )

        extra = self.query_one("#preview-extra", Static)
        if entry.rating > 0:
            stars_full = int(entry.rating)
            stars = "\u2605" * stars_full + "\u2606" * (5 - stars_full)
            extra.update(f"Rating: {stars} ({entry.rating:.1f}, {entry.votes} votes)")
        else:
            extra.update("Rating: Not rated")

        description = entry.description or "No description available"
        if len(description) > 500:
            description = description[:500] + "..."
        self.query_one("#preview-desc", Static).update(description)

    def _do_import(self, entry: FileEntry) -> int | None:
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
            return None

        with IdgamesSource() as source:
            return source.import_wad(entry)

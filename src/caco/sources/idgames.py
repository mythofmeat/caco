"""idgames archive source adapter."""

from pathlib import Path

from idgames.client import IdgamesClient
from idgames.models import FileEntry

from caco.db import SourceType, add_wad


class IdgamesSource:
    """Adapter for importing WADs from idgames archive."""

    def __init__(self):
        self.client = IdgamesClient()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.client.close()

    def search(self, query: str) -> list[FileEntry]:
        """Search idgames for WADs."""
        # Search by title first, then filename if no results
        results = self.client.search(query, type="title")
        if not results:
            results = self.client.search(query, type="filename")
        return results

    def get(self, file_id: int) -> FileEntry:
        """Get a specific file by ID."""
        return self.client.get(id=file_id)

    def import_wad(
        self,
        entry: FileEntry,
        tags: list[str] | None = None,
    ) -> int:
        """Import a WAD from idgames into the local database."""
        # Extract year from date if available
        year = None
        if entry.date:
            try:
                year = int(entry.date.split("-")[0])
            except (ValueError, IndexError):
                pass

        return add_wad(
            title=entry.title,
            author=entry.author,
            year=year,
            description=entry.description,
            source_type=SourceType.IDGAMES,
            source_id=str(entry.id),
            source_url=entry.url,
            filename=entry.filename,
            tags=tags,
        )

    def download(
        self,
        entry: FileEntry,
        dest: Path,
        mirror: int = 0,
    ) -> Path:
        """Download a WAD file. Returns the path to the downloaded file."""
        dest_file = dest / entry.filename
        for _ in self.client.download(entry, dest_file, mirror):
            pass  # Could add progress callback here
        return dest_file

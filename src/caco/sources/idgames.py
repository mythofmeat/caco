"""idgames archive source adapter."""

from pathlib import Path

from caco.idgames import IdgamesClient, FileEntry
from rich.console import Console
from rich.progress import Progress, BarColumn, DownloadColumn, TransferSpeedColumn

from caco.config import get_download_mirror
from caco.db import SourceType, add_wad
from caco.utils import extract_year


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
        year = extract_year(entry.date)

        # Build description, appending textfile content if available
        description = entry.description
        if entry.textfile:
            description = f"{description}\n\n---\n\n{entry.textfile}" if description else entry.textfile

        return add_wad(
            title=entry.title,
            author=entry.author,
            year=year,
            description=description,
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
        mirror: int | None = None,
        console: Console | None = None,
        progress_callback: "Callable | None" = None,
    ) -> Path:
        """Download a WAD file. Returns the path to the downloaded file.

        Args:
            progress_callback: Optional callable(downloaded, total, filename)
                for non-console progress reporting (e.g. GUI).
        """
        if mirror is None:
            mirror = get_download_mirror()
        dest_file = dest / entry.filename

        if console:
            with Progress(
                "[progress.description]{task.description}",
                BarColumn(),
                DownloadColumn(),
                TransferSpeedColumn(),
                console=console,
            ) as progress:
                task = progress.add_task(f"Downloading {entry.filename}", total=None)
                for downloaded, total in self.client.download(entry, dest_file, mirror):
                    progress.update(task, completed=downloaded, total=total)
        else:
            for downloaded, total in self.client.download(entry, dest_file, mirror):
                if progress_callback:
                    progress_callback(downloaded, total, entry.filename)

        return dest_file

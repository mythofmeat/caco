"""idgames archive API client."""

from pathlib import Path
from typing import Iterator

from caco.idgames.models import ApiInfo, Directory, FileEntry, Review, Vote
from caco.utils import BaseHttpClient, CacoSourceError


MIRRORS = [
    "https://youfailit.net/pub/idgames/",  # Fastest
    "https://www.quaddicted.com/files/idgames/",
    "https://ftpmirror1.infania.net/pub/idgames/",
    "https://mirror.braindrainlan.nu/pub/idgames/",
    "https://files.xvertigox.com/idgames/",
]


class IdgamesError(CacoSourceError):
    """Error from the idgames API."""

    pass


class IdgamesClient(BaseHttpClient):
    """Client for the idgames archive API."""

    BASE_URL = "https://www.doomworld.com/idgames/api/api.php"

    def _request(self, action: str, **params) -> dict:
        """Make a request to the API."""
        params = {k: v for k, v in params.items() if v is not None}
        params["action"] = action
        params["out"] = "json"

        response = self._client.get(self.BASE_URL, params=params)
        response.raise_for_status()

        data = response.json()

        if "error" in data:
            raise IdgamesError(data["error"].get("message", "Unknown error"))

        if "warning" in data:
            # Warnings still contain content, just log and continue
            pass

        result: dict = data.get("content", {})
        return result

    def ping(self) -> str:
        """Check if the API server is responding."""
        content = self._request("ping")
        status: str = content.get("status", "")
        return status

    def dbping(self) -> str:
        """Check if the database is responding."""
        content = self._request("dbping")
        status: str = content.get("status", "")
        return status

    def about(self) -> ApiInfo:
        """Get API information."""
        content = self._request("about")
        return ApiInfo(**content)

    def get(self, *, id: int | None = None, file: str | None = None) -> FileEntry:
        """Get file details by ID or filename."""
        if id is None and file is None:
            raise ValueError("Must provide either id or file")

        content = self._request("get", id=id, file=file)

        # Parse reviews if present
        reviews = []
        if "reviews" in content and content["reviews"]:
            review_data = content["reviews"].get("review")
            if review_data:
                if isinstance(review_data, dict):
                    review_data = [review_data]
                reviews = [Review(**r) for r in review_data]
        content["reviews"] = reviews

        return FileEntry(**content)

    def get_parent_dir(
        self, *, id: int | None = None, name: str | None = None
    ) -> Directory:
        """Get parent directory info."""
        if id is None and name is None:
            raise ValueError("Must provide either id or name")

        content = self._request("getparentdir", id=id, name=name)
        return Directory(**content)

    def get_dirs(
        self, *, id: int | None = None, name: str | None = None
    ) -> list[Directory]:
        """Get subdirectories of a directory."""
        content = self._request("getdirs", id=id, name=name)

        if not content:
            return []

        dirs = content.get("dir", [])
        if isinstance(dirs, dict):
            dirs = [dirs]

        return [Directory(**d) for d in dirs]

    def get_files(
        self, *, id: int | None = None, name: str | None = None
    ) -> list[FileEntry]:
        """Get files in a directory."""
        content = self._request("getfiles", id=id, name=name)

        if not content:
            return []

        files = content.get("file", [])
        if isinstance(files, dict):
            files = [files]

        return [FileEntry(**f) for f in files]

    def get_contents(
        self, *, id: int | None = None, name: str | None = None
    ) -> tuple[list[Directory], list[FileEntry]]:
        """Get both subdirectories and files in a directory."""
        content = self._request("getcontents", id=id, name=name)

        if not content:
            return [], []

        dirs = content.get("dir", [])
        if isinstance(dirs, dict):
            dirs = [dirs]

        files = content.get("file", [])
        if isinstance(files, dict):
            files = [files]

        return (
            [Directory(**d) for d in dirs],
            [FileEntry(**f) for f in files],
        )

    def latest_votes(self, limit: int | None = None) -> list[Vote]:
        """Get the latest votes."""
        content = self._request("latestvotes", limit=limit)

        if not content:
            return []

        votes = content.get("vote", [])
        if isinstance(votes, dict):
            votes = [votes]

        return [Vote(**v) for v in votes]

    def latest_files(
        self, limit: int | None = None, startid: int | None = None
    ) -> list[FileEntry]:
        """Get the latest files."""
        content = self._request("latestfiles", limit=limit, startid=startid)

        if not content:
            return []

        files = content.get("file", [])
        if isinstance(files, dict):
            files = [files]

        return [FileEntry(**f) for f in files]

    def search(
        self,
        query: str,
        *,
        type: str | None = None,
        sort: str | None = None,
        sort_dir: str | None = None,
    ) -> list[FileEntry]:
        """
        Search for files.

        Args:
            query: Search query string
            type: Field to search (filename, title, author, email, description, credits, editors, textfile)
            sort: Sort order (date, filename, size, rating)
            sort_dir: Sort direction (asc, desc)
        """
        content = self._request("search", query=query, type=type, sort=sort, dir=sort_dir)

        if not content:
            return []

        files = content.get("file", [])
        if isinstance(files, dict):
            files = [files]

        return [FileEntry(**f) for f in files]

    def get_download_url(self, entry: FileEntry, mirror: int = 0) -> str:
        """Get the download URL for a file entry."""
        # Normalize the path (remove double slashes)
        path = (entry.dir.strip("/") + "/" + entry.filename).replace("//", "/")
        return MIRRORS[mirror % len(MIRRORS)] + path

    def download(
        self,
        entry: FileEntry,
        dest: Path | None = None,
        mirror: int = 0,
    ) -> Iterator[tuple[int, int]]:
        """
        Download a file, yielding (bytes_downloaded, total_bytes) tuples.

        Uses atomic download: writes to a .partial file first, then renames
        on success. Cleans up the .partial file on failure.

        Args:
            entry: The file entry to download
            dest: Destination path (defaults to current dir with original filename)
            mirror: Mirror index to use (0-4)

        Yields:
            Tuples of (bytes_downloaded, total_bytes)
        """
        url = self.get_download_url(entry, mirror)
        dest = dest or Path(entry.filename)
        partial = dest.with_suffix(dest.suffix + ".partial")

        try:
            with self._client.stream("GET", url) as response:
                response.raise_for_status()
                total = int(response.headers.get("content-length", 0))
                downloaded = 0

                with open(partial, "wb") as f:
                    for chunk in response.iter_bytes(chunk_size=262144):
                        f.write(chunk)
                        downloaded += len(chunk)
                        yield downloaded, total

            # Rename to final destination only on complete success
            partial.rename(dest)
        except BaseException:
            # Clean up partial download on any failure (including GeneratorExit)
            if partial.exists():
                partial.unlink()
            raise

"""Qt model wrapping db.search_wads() with batch stats."""

from PySide6.QtCore import Qt, QAbstractTableModel, QModelIndex

from caco import db
from caco.player import format_duration
from caco.gui.constants import Column, DEFAULT_COLUMNS, ALL_COLUMNS
from caco.gui.theme import get_status_color, get_status_display
from caco.utils import format_rating


class WadTableModel(QAbstractTableModel):
    """Table model backed by caco's SQLite database.

    Uses batch stat functions to avoid N+1 queries, mirroring the TUI's
    WadTable.load_wads() pattern.
    """

    def __init__(self, columns: list[Column] | None = None, parent=None):
        super().__init__(parent)
        self._columns = columns or DEFAULT_COLUMNS
        self._wads: list[dict] = []
        self._wad_index: dict[int, int] = {}  # wad_id -> row index (O(1) lookup)
        # Batch-fetched stat maps
        self._playtime_map: dict[int, int] = {}
        self._last_played_map: dict[int, str] = {}
        self._times_beaten_map: dict[int, int] = {}
        self._session_count_map: dict[int, int] = {}

    def load(
        self,
        query: str | None = None,
        sort_by: str = "id",
        sort_desc: bool = False,
        include_deleted: bool = False,
    ) -> int:
        """Reload data from the database. Returns WAD count."""
        self.beginResetModel()

        self._wads = db.search_wads(
            query=query,
            sort_by=sort_by,
            sort_desc=sort_desc,
            include_deleted=include_deleted,
        )

        # Build O(1) wad_id → row index (mirrors TUI's _wad_id_to_row)
        self._wad_index = {w["id"]: i for i, w in enumerate(self._wads)}

        # Batch-fetch all stats in bulk
        wad_ids = [w["id"] for w in self._wads]
        if wad_ids:
            self._playtime_map = db.get_total_playtime_batch(wad_ids)
            self._last_played_map = db.get_last_played_batch(wad_ids)
            self._times_beaten_map = db.get_times_beaten_batch(wad_ids)
            self._session_count_map = db.get_session_count_batch(wad_ids)
        else:
            self._playtime_map = {}
            self._last_played_map = {}
            self._times_beaten_map = {}
            self._session_count_map = {}

        self.endResetModel()
        return len(self._wads)

    # ── QAbstractTableModel interface ──────────────────────────────

    def rowCount(self, parent=QModelIndex()):
        return len(self._wads)

    def columnCount(self, parent=QModelIndex()):
        return len(self._columns)

    def headerData(self, section, orientation, role=Qt.DisplayRole):
        if role == Qt.DisplayRole and orientation == Qt.Horizontal:
            if 0 <= section < len(self._columns):
                return self._columns[section].header
        return None

    def data(self, index, role=Qt.DisplayRole):
        if not index.isValid() or index.row() >= len(self._wads):
            return None

        wad = self._wads[index.row()]
        col = self._columns[index.column()]
        wad_id = wad["id"]

        if role == Qt.DisplayRole:
            return self._display_data(wad, col)
        elif role == Qt.ForegroundRole:
            if col == Column.STATUS:
                return get_status_color(wad["status"])
        elif role == Qt.TextAlignmentRole:
            if col in (Column.ID, Column.YEAR, Column.BEATEN, Column.PLAYTIME):
                return int(Qt.AlignRight | Qt.AlignVCenter)
            if col == Column.RATING:
                return int(Qt.AlignCenter | Qt.AlignVCenter)
        elif role == Qt.UserRole:
            # Return the wad_id for selection tracking
            return wad_id
        elif role == Qt.ToolTipRole:
            if col == Column.TITLE and wad.get("description"):
                desc = wad["description"]
                return desc[:300] + "..." if len(desc) > 300 else desc

        return None

    def _display_data(self, wad: dict, col: Column) -> str:
        """Get display string for a cell."""
        wad_id = wad["id"]

        if col == Column.ID:
            return str(wad_id)
        elif col == Column.TITLE:
            return wad["title"]
        elif col == Column.AUTHOR:
            return wad.get("author") or "-"
        elif col == Column.YEAR:
            return str(wad["year"]) if wad.get("year") else "-"
        elif col == Column.STATUS:
            return get_status_display(wad["status"])
        elif col == Column.RATING:
            stars = format_rating(wad.get("rating"))
            return stars if stars else "-"
        elif col == Column.BEATEN:
            count = self._times_beaten_map.get(wad_id, 0)
            return str(count) if count else "-"
        elif col == Column.PLAYTIME:
            seconds = self._playtime_map.get(wad_id, 0)
            return format_duration(seconds) if seconds else "-"
        elif col == Column.LAST_PLAYED:
            ts = self._last_played_map.get(wad_id)
            return ts[:10] if ts else "-"
        elif col == Column.TAGS:
            tags = wad.get("tags", [])
            if tags:
                if len(tags) > 3:
                    return ", ".join(tags[:3]) + f" +{len(tags) - 3}"
                return ", ".join(tags)
            return "-"
        elif col == Column.SOURCE:
            return wad.get("source_type", "-")

        return "-"

    # ── Public API ─────────────────────────────────────────────────

    @property
    def columns(self) -> list[Column]:
        """Current visible columns."""
        return list(self._columns)

    def set_columns(self, columns: list[Column]):
        """Change visible columns. Triggers a full layout change."""
        self.beginResetModel()
        self._columns = list(columns)
        self.endResetModel()

    def get_wad(self, row: int) -> dict | None:
        """Get the WAD dict at a given row."""
        if 0 <= row < len(self._wads):
            return self._wads[row]
        return None

    def get_wad_by_id(self, wad_id: int) -> dict | None:
        """Find a WAD dict by its ID (O(1) via index)."""
        row = self._wad_index.get(wad_id)
        if row is not None:
            return self._wads[row]
        return None

    def get_wad_stats(self, wad_id: int) -> dict:
        """Get pre-fetched stats for a WAD (mirrors TUI's get_wad_stats)."""
        return {
            "playtime": self._playtime_map.get(wad_id, 0),
            "last_played": self._last_played_map.get(wad_id),
            "times_beaten": self._times_beaten_map.get(wad_id, 0),
            "session_count": self._session_count_map.get(wad_id, 0),
        }

    def wad_count(self) -> int:
        """Return the current number of WADs."""
        return len(self._wads)

    def total_playtime(self) -> int:
        """Return total playtime across all loaded WADs."""
        return sum(self._playtime_map.values())

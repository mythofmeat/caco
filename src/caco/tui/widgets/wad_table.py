"""WAD list table widget."""

import asyncio

from rich.text import Text
from textual.binding import Binding
from textual.widgets import DataTable
from textual.message import Message

from caco import db
from caco.player import format_duration
from caco.tui.theme import get_status_color


class WadTable(DataTable):
    """DataTable for displaying WAD list with vim-style navigation."""

    BINDINGS = [
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
        Binding("g", "handle_g", "Top (gg)", show=False),
        Binding("G", "go_bottom", "Bottom", show=False),
        Binding("ctrl+d", "page_down", "Page Down", show=False),
        Binding("ctrl+u", "page_up", "Page Up", show=False),
        Binding("enter", "select_cursor", "Play", show=True),
    ]

    class WadSelected(Message):
        """Fired when cursor moves to a new WAD."""

        def __init__(self, wad_id: int | None) -> None:
            super().__init__()
            self.wad_id = wad_id

    class WadActivated(Message):
        """Fired when Enter is pressed on a WAD (to play it)."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._wads: list[dict] = []
        self._wad_id_to_row: dict[int, int] = {}
        self._g_pressed = False
        self._g_timeout_task: asyncio.Task | None = None
        # Unified stats map: {wad_id: {playtime, last_played, session_count, times_beaten}}
        self._stats_map: dict[int, dict] = {}

    def on_mount(self) -> None:
        """Set up the table columns."""
        self.cursor_type = "row"
        self.zebra_stripes = True

        # Add columns
        self.add_column("ID", key="id", width=5)
        self.add_column("Title", key="title", width=30)
        self.add_column("Author", key="author", width=20)
        self.add_column("Status", key="status", width=12)
        self.add_column("Playtime", key="playtime", width=10)

    def load_wads(
        self,
        query: str | None = None,
        sort_by: str = "id",
        sort_desc: bool = False,
        include_deleted: bool = False,
    ) -> int:
        """Load WADs from database and populate table.

        Returns the number of WADs loaded.
        """
        self._wads = db.search_wads(
            query, sort_by=sort_by, sort_desc=sort_desc,
            include_deleted=include_deleted,
        )
        self._wad_id_to_row.clear()
        self.clear()

        if not self._wads:
            self._stats_map = {}
            return 0

        # Batch-fetch all stats in 2 queries on 1 connection
        wad_ids = [wad["id"] for wad in self._wads]
        self._stats_map = db.get_wad_stats_batch(wad_ids)

        for i, wad in enumerate(self._wads):
            wad_id = wad["id"]
            self._wad_id_to_row[wad_id] = i

            # Format playtime from unified stats map
            playtime = self._stats_map.get(wad_id, {}).get("playtime", 0)
            playtime_str = format_duration(playtime) if playtime else "-"

            # Status with color styling using Rich Text
            status = wad["status"]
            color = get_status_color(status)
            status_text = Text(status, style=color) if color else status

            self.add_row(
                str(wad_id),
                wad["title"],
                wad["author"] or "-",
                status_text,
                playtime_str,
                key=str(wad_id),
            )

        # Notify about initial selection
        if self._wads:
            self.post_message(self.WadSelected(self._wads[0]["id"]))

        return len(self._wads)

    def get_wad_stats(self, wad_id: int) -> dict:
        """Get pre-fetched stats for a WAD (avoids extra DB queries).

        Returns dict with: playtime, last_played, times_beaten, session_count.
        """
        defaults = {"playtime": 0, "last_played": None, "times_beaten": 0, "session_count": 0}
        return self._stats_map.get(wad_id, defaults)

    def update_row(self, wad_id: int) -> bool:
        """Update a single row in-place without full table reload.

        Returns True if the row was updated, False if wad_id not found in table.
        """
        if wad_id not in self._wad_id_to_row:
            return False

        row_idx = self._wad_id_to_row[wad_id]
        wad = db.get_wad(wad_id)
        if not wad:
            return False

        # Update the cached wad data
        self._wads[row_idx] = wad

        # Refresh stats for this single WAD (1 call, 2 queries)
        self._stats_map.update(db.get_wad_stats_batch([wad_id]))

        # Update cells in the DataTable
        row_key = str(wad_id)
        playtime = self._stats_map.get(wad_id, {}).get("playtime", 0)
        playtime_str = format_duration(playtime) if playtime else "-"

        status = wad["status"]
        color = get_status_color(status)
        status_text = Text(status, style=color) if color else status

        self.update_cell(row_key, "title", wad["title"])
        self.update_cell(row_key, "author", wad["author"] or "-")
        self.update_cell(row_key, "status", status_text)
        self.update_cell(row_key, "playtime", playtime_str)

        return True

    def get_selected_wad_id(self) -> int | None:
        """Get the currently selected WAD ID."""
        if self.cursor_row is not None and 0 <= self.cursor_row < len(self._wads):
            return self._wads[self.cursor_row]["id"]
        return None

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        """Handle row highlight change."""
        if event.cursor_row is not None and 0 <= event.cursor_row < len(self._wads):
            wad_id = self._wads[event.cursor_row]["id"]
            self.post_message(self.WadSelected(wad_id))
        else:
            self.post_message(self.WadSelected(None))

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        """Handle Enter key on a row."""
        wad_id = self.get_selected_wad_id()
        if wad_id is not None:
            self.post_message(self.WadActivated(wad_id))

    async def action_cursor_down(self) -> None:
        """Move cursor down (j key)."""
        self._g_pressed = False
        if self.cursor_row is not None and self.cursor_row < len(self._wads) - 1:
            self.move_cursor(row=self.cursor_row + 1)

    async def action_cursor_up(self) -> None:
        """Move cursor up (k key)."""
        self._g_pressed = False
        if self.cursor_row is not None and self.cursor_row > 0:
            self.move_cursor(row=self.cursor_row - 1)

    async def action_page_down(self) -> None:
        """Move cursor down by half page (Ctrl+d)."""
        self._g_pressed = False
        if self.cursor_row is not None:
            # Move by ~10 rows or to end
            new_row = min(self.cursor_row + 10, len(self._wads) - 1)
            self.move_cursor(row=new_row)

    async def action_page_up(self) -> None:
        """Move cursor up by half page (Ctrl+u)."""
        self._g_pressed = False
        if self.cursor_row is not None:
            # Move up by ~10 rows or to start
            new_row = max(self.cursor_row - 10, 0)
            self.move_cursor(row=new_row)

    async def action_go_top(self) -> None:
        """Go to top of list (gg)."""
        self._g_pressed = False
        if self._wads:
            self.move_cursor(row=0)

    async def action_go_bottom(self) -> None:
        """Go to bottom of list (G)."""
        self._g_pressed = False
        if self._wads:
            self.move_cursor(row=len(self._wads) - 1)

    def action_handle_g(self) -> None:
        """Handle 'g' key press for gg motion."""
        if self._g_pressed:
            # Second g - go to top
            self._g_pressed = False
            if self._g_timeout_task:
                self._g_timeout_task.cancel()
            self.run_worker(self.action_go_top())
        else:
            # First g - wait for second
            self._g_pressed = True
            self._g_timeout_task = asyncio.create_task(self._g_timeout())

    async def _g_timeout(self) -> None:
        """Reset g state after timeout."""
        await asyncio.sleep(0.5)
        self._g_pressed = False

    def reset_g_state(self) -> None:
        """Reset g key state."""
        self._g_pressed = False

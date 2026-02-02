"""WAD list table widget."""

import asyncio

from rich.text import Text
from textual.binding import Binding
from textual.widgets import DataTable
from textual.message import Message

from caco import db
from caco.player import format_duration


# Status colors for table display
STATUS_COLORS = {
    "to-play": "dodger_blue1",
    "backlog": "yellow",
    "playing": "green1",
    "finished": "grey50",
    "abandoned": "red",
    "awaiting-update": "magenta",
}


class WadTable(DataTable):
    """DataTable for displaying WAD list with vim-style navigation."""

    BINDINGS = [
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
        Binding("g", "handle_g", "Top (gg)", show=False),
        Binding("G", "go_bottom", "Bottom", show=False),
        Binding("ctrl+d", "page_down", "Page Down", show=False),
        Binding("ctrl+u", "page_up", "Page Up", show=False),
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

    def on_mount(self) -> None:
        """Set up the table columns."""
        self.cursor_type = "row"
        self.zebra_stripes = True

        # Add columns
        self.add_column("ID", key="id", width=5)
        self.add_column("Title", key="title", width=30)
        self.add_column("Author", key="author", width=20)
        self.add_column("Status", key="status", width=12)
        self.add_column("Maps", key="maps", width=6)
        self.add_column("Playtime", key="playtime", width=10)

    def load_wads(
        self,
        query: str | None = None,
        sort_by: str = "id",
        sort_desc: bool = False,
    ) -> int:
        """Load WADs from database and populate table.

        Returns the number of WADs loaded.
        """
        self._wads = db.search_wads(query, sort_by=sort_by, sort_desc=sort_desc)
        self._wad_id_to_row.clear()
        self.clear()

        if not self._wads:
            return 0

        # Batch fetch stats
        wad_ids = [w["id"] for w in self._wads]
        maps_completed = db.get_maps_completed_batch(wad_ids)

        for i, wad in enumerate(self._wads):
            wad_id = wad["id"]
            self._wad_id_to_row[wad_id] = i

            # Format playtime
            playtime = db.get_total_playtime(wad_id)
            playtime_str = format_duration(playtime) if playtime else "-"

            # Format maps
            maps_count = maps_completed.get(wad_id, 0)
            maps_str = str(maps_count) if maps_count else "-"

            # Status with color styling using Rich Text
            status = wad["status"]
            color = STATUS_COLORS.get(status, "")
            status_text = Text(status, style=color) if color else status

            self.add_row(
                str(wad_id),
                wad["title"],
                wad["author"] or "-",
                status_text,
                maps_str,
                playtime_str,
                key=str(wad_id),
            )

        # Notify about initial selection
        if self._wads:
            self.post_message(self.WadSelected(self._wads[0]["id"]))

        return len(self._wads)

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

    def handle_g_key(self) -> bool:
        """Handle 'g' key press for gg motion. Returns True if handled.

        Deprecated: Use action_handle_g instead. Kept for compatibility.
        """
        self.action_handle_g()
        return True

    def reset_g_state(self) -> None:
        """Reset g key state."""
        self._g_pressed = False

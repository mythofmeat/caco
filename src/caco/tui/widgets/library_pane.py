"""Reusable library pane widget with table, info panel, and filtering."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Static

from caco import db
from caco.config import get_tui_config
from caco.db import STATUS_SHORTCUTS
from caco.player import play
from caco.tui.widgets.filter_input import FilterInput
from caco.tui.widgets.sort_select import SortSelect
from caco.tui.widgets.wad_info import WadInfoPanel
from caco.tui.widgets.wad_table import WadTable

# Sort field display names (for notifications)
SORT_DISPLAY = {
    "id": "ID",
    "title": "Title",
    "author": "Author",
    "playtime": "Playtime",
    "last_played": "Last Played",
    "year": "Year",
    "rating": "Rating",
}


class LibraryPane(Widget):
    """Reusable library pane with WAD table, info panel, filtering and sorting.

    This can be used standalone or embedded in tabs with a status filter.
    """

    # Note: j/k/g/G/ctrl+d/ctrl+u are handled by WadTable directly
    BINDINGS = [
        # Filter
        Binding("slash", "focus_filter", "Filter", show=True, key_display="/"),
        Binding("f", "focus_filter", "Filter", show=False),
        # Actions
        Binding("i", "show_details", "Info", show=True),
        Binding("h", "show_history", "History", show=True),
        Binding("e", "edit_wad", "Edit", show=True),
        Binding("d", "delete_wad", "Delete", show=True),
        # Status shortcuts
        Binding("s", "status_mode", "Status", show=True),
        # Sorting
        Binding("o", "focus_sort", "Sort", show=True),
        Binding("O", "toggle_sort_dir", "Sort Dir", show=False, key_display="O"),
        # Rating
        Binding("r", "cycle_rating", "Rating", show=True),
        Binding("R", "clear_rating", "Clear Rating", show=False, key_display="R"),
        # Beaten tracking
        Binding("+", "increment_beaten", "Beaten+", show=False),
        Binding("-", "decrement_beaten", "Beaten-", show=False),
        # Trash view (All tab only)
        Binding("T", "toggle_trash", "Trash", show=False, key_display="T"),
        Binding("u", "restore_wad", "Restore", show=False),
        # Stats
        Binding("S", "show_stats", "Stats", show=False, key_display="S"),
        # Cache
        Binding("C", "show_cache", "Cache", show=False, key_display="C"),
        # Panel toggle
        Binding("P", "toggle_panel", "Panel", show=False, key_display="P"),
        # Escape
        Binding("escape", "escape", "Cancel", show=False),
    ]

    class WadAction(Message):
        """Generic message for actions that need parent handling."""

        def __init__(self, wad_id: int, action: str) -> None:
            super().__init__()
            self.wad_id = wad_id
            self.action = action

    class WadImported(Message):
        """Fired when a WAD is imported (used by parent to refresh other panes)."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    DEFAULT_CSS = """
    LibraryPane {
        height: 100%;
        width: 100%;
    }

    LibraryPane #pane-header {
        height: 3;
        width: 100%;
        padding: 0 1;
        align: left middle;
    }

    LibraryPane #pane-content {
        height: 1fr;
    }

    LibraryPane #wad-list-container {
        width: 2fr;
        height: 100%;
    }

    LibraryPane #info-panel-container {
        width: 1fr;
        height: 100%;
        border-left: solid $primary;
    }

    LibraryPane #status-mode {
        dock: bottom;
        height: 1;
        background: $warning;
        color: $text;
        padding: 0 1;
        display: none;
    }

    LibraryPane #status-mode.visible {
        display: block;
    }
    """

    def __init__(
        self,
        status_filter: str | list[str] | None = None,
        **kwargs,
    ) -> None:
        """Initialize the library pane.

        Args:
            status_filter: If set, pre-filter to only show WADs with this status.
                          Can be a single status string or a list of statuses (OR query).
        """
        super().__init__(**kwargs)
        self._status_filter = status_filter
        self._current_query: str = ""
        self._status_mode = False
        self._show_deleted = False
        tui_config = get_tui_config()
        self._sort_by: str = tui_config.get("default_sort", "id")
        self._sort_desc: bool = tui_config.get("default_sort_desc", False)

    def compose(self) -> ComposeResult:
        with Horizontal(id="pane-header"):
            yield FilterInput(id="filter")
            yield SortSelect(sort_by=self._sort_by, sort_desc=self._sort_desc, id="sort-select")
        with Horizontal(id="pane-content"):
            with Vertical(id="wad-list-container"):
                yield WadTable(id="wad-table")
            with Vertical(id="info-panel-container"):
                yield WadInfoPanel(id="info-panel")
        yield Static("", id="status-mode")

    def on_mount(self) -> None:
        """Load initial data."""
        self._load_wads()
        # Only focus if this is the "All" tab (no status filter)
        # This prevents the last-mounted tab from stealing focus
        if self._status_filter is None:
            self.query_one("#wad-table", WadTable).focus()

    def _get_effective_query(self) -> str | None:
        """Get the effective query combining user query with status filter."""
        parts = []
        if self._status_filter:
            if isinstance(self._status_filter, list):
                # OR query for multiple statuses: "status:a , status:b"
                status_parts = " , ".join(f"status:{s}" for s in self._status_filter)
                parts.append(status_parts)
            else:
                parts.append(f"status:{self._status_filter}")
        if self._current_query:
            parts.append(self._current_query)

        if parts:
            return " ".join(parts)
        return None

    def _load_wads(self) -> None:
        """Load WADs with current query and sort settings."""
        table = self.query_one("#wad-table", WadTable)
        count = table.load_wads(
            self._get_effective_query(),
            sort_by=self._sort_by,
            sort_desc=self._sort_desc,
            include_deleted=self._show_deleted,
        )
        # Update filter bar count
        filter_widget = self.query_one("#filter", FilterInput)
        if self._show_deleted:
            filter_widget.set_wad_count(count, label="trash")
        else:
            filter_widget.set_wad_count(count)

    def refresh_data(self) -> None:
        """Public method to refresh WAD list (called by parent after import)."""
        self._load_wads()

    # -------------------------------------------------------------------------
    # Filter Actions
    # -------------------------------------------------------------------------

    def action_focus_filter(self) -> None:
        """Focus the filter input."""
        filter_widget = self.query_one("#filter", FilterInput)
        filter_widget.show()

    def on_filter_input_query_changed(self, event: FilterInput.QueryChanged) -> None:
        """Handle query change from filter."""
        self._current_query = event.query
        self._load_wads()

    def on_filter_input_escaped(self, event: FilterInput.Escaped) -> None:
        """Handle escape from filter - refocus the table."""
        self.query_one("#wad-table", WadTable).focus()

    def on_filter_input_submitted(self, event: FilterInput.Submitted) -> None:
        """Handle Enter on filter input - refocus the table."""
        self.query_one("#wad-table", WadTable).focus()

    # -------------------------------------------------------------------------
    # Sort Actions
    # -------------------------------------------------------------------------

    def action_focus_sort(self) -> None:
        """Focus the sort dropdown."""
        sort_select = self.query_one("#sort-select", SortSelect)
        select = sort_select.query_one("Select")
        select.focus()

    def on_sort_select_sort_changed(self, event: SortSelect.SortChanged) -> None:
        """Handle sort change from dropdown."""
        self._sort_by = event.sort_by
        self._sort_desc = event.sort_desc
        self._load_wads()
        direction = "↓" if self._sort_desc else "↑"
        display_name = SORT_DISPLAY.get(self._sort_by, self._sort_by)
        self.notify(f"Sort: {display_name} {direction}")
        # Return focus to table after sort change
        self.query_one("#wad-table", WadTable).focus()

    def action_toggle_sort_dir(self) -> None:
        """Toggle sort direction (keyboard shortcut)."""
        sort_select = self.query_one("#sort-select", SortSelect)
        sort_select.toggle_direction()

    # -------------------------------------------------------------------------
    # WAD Selection/Activation Events
    # -------------------------------------------------------------------------

    def on_wad_table_wad_selected(self, event: WadTable.WadSelected) -> None:
        """Update info panel when WAD selection changes."""
        panel = self.query_one("#info-panel", WadInfoPanel)
        if event.wad_id is not None:
            table = self.query_one("#wad-table", WadTable)
            stats = table.get_wad_stats(event.wad_id)
            panel.update_wad(event.wad_id, stats=stats)
        else:
            panel.update_wad(None)

    def on_wad_table_wad_activated(self, event: WadTable.WadActivated) -> None:
        """Handle Enter on a WAD row - play the WAD."""
        if self._show_deleted:
            self.notify("Cannot play a deleted WAD", severity="warning")
            return
        self._play_wad(event.wad_id)

    # -------------------------------------------------------------------------
    # Play Action
    # -------------------------------------------------------------------------

    def _play_wad(self, wad_id: int) -> None:
        """Play a WAD by ID."""
        wad = db.get_wad(wad_id)
        if not wad:
            self.notify("WAD not found", severity="error")
            return

        self.notify(f"Launching {wad['title']}...")
        self.run_worker(self._play_and_refresh(wad_id))

    async def _play_and_refresh(self, wad_id: int) -> None:
        """Play WAD and refresh after."""
        wad = db.get_wad(wad_id)
        if not wad:
            return

        error = None
        with self.app.suspend():
            try:
                play(wad_id)
            except ValueError as e:
                error = str(e)

        if error:
            self.notify(error, severity="error", timeout=10)
        else:
            self.notify(f"Finished playing {wad['title']}")

        self._load_wads()
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(wad_id)

    # -------------------------------------------------------------------------
    # Detail/Edit Screens
    # -------------------------------------------------------------------------

    def action_show_details(self) -> None:
        """Show WAD detail screen."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        from caco.tui.screens.wad_detail import WadDetailScreen

        self.app.push_screen(WadDetailScreen(wad_id))

    def action_show_history(self) -> None:
        """Show session history screen."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        from caco.tui.screens.sessions import SessionsScreen

        self.app.push_screen(SessionsScreen(wad_id))

    def action_edit_wad(self) -> None:
        """Open WAD edit screen."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        from caco.tui.screens.wad_edit import WadEditScreen

        def on_dismiss(result: bool | None) -> None:
            if result:
                self._load_wads()
                panel = self.query_one("#info-panel", WadInfoPanel)
                panel.update_wad(wad_id)

        self.app.push_screen(WadEditScreen(wad_id), on_dismiss)

    # -------------------------------------------------------------------------
    # Delete
    # -------------------------------------------------------------------------

    def action_delete_wad(self) -> None:
        """Delete the selected WAD (with confirmation)."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        from caco.tui.screens.confirm_delete import ConfirmDeleteScreen

        def on_dismiss(result: int | None) -> None:
            if result is not None:
                db.delete_wad(result)
                self.notify("Moved to trash")
                self._load_wads()
                self.post_message(self.WadImported(result))

        self.app.push_screen(ConfirmDeleteScreen(wad_id), on_dismiss)

    # -------------------------------------------------------------------------
    # Rating
    # -------------------------------------------------------------------------

    def action_cycle_rating(self) -> None:
        """Cycle rating 0→1→2→3→4→5→0 for selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        wad = db.get_wad(wad_id)
        if not wad:
            return

        current = wad.get("rating") or 0
        new_rating = (current + 1) % 6
        if new_rating == 0:
            db.update_wad(wad_id, rating=None)
            self.notify("Rating cleared")
        else:
            db.update_wad(wad_id, rating=new_rating)
            stars = "★" * new_rating + "☆" * (5 - new_rating)
            self.notify(f"Rating: {stars}")

        table.update_row(wad_id)
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(wad_id)

    def action_clear_rating(self) -> None:
        """Clear rating for selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        db.update_wad(wad_id, rating=None)
        table.update_row(wad_id)
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(wad_id)
        self.notify("Rating cleared")

    # -------------------------------------------------------------------------
    # Beaten Tracking
    # -------------------------------------------------------------------------

    def action_increment_beaten(self) -> None:
        """Increment beaten count for selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        db.add_wad_completion(wad_id)
        count = db.get_times_beaten(wad_id)
        table.update_row(wad_id)
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(wad_id)
        self.notify(f"Beaten: {count}x")

    def action_decrement_beaten(self) -> None:
        """Decrement beaten count for selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        completions = db.get_wad_completions(wad_id)
        if not completions:
            self.notify("No completions to remove", severity="warning")
            return

        # Delete most recent completion
        db.delete_wad_completion(completions[0]["id"])
        count = db.get_times_beaten(wad_id)
        table.update_row(wad_id)
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(wad_id)
        self.notify(f"Beaten: {count}x")

    # -------------------------------------------------------------------------
    # Status Mode
    # -------------------------------------------------------------------------

    def action_status_mode(self) -> None:
        """Enter status mode for quick status change."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        self._status_mode = True
        status_indicator = self.query_one("#status-mode", Static)
        status_indicator.update(
            "Status: [p]laying [f]inished [t]o-play [b]acklog [a]bandoned [w]aiting"
        )
        status_indicator.add_class("visible")

    def on_key(self, event) -> None:
        """Handle key events for status mode."""
        if self._status_mode:
            key = event.key
            if key in STATUS_SHORTCUTS:
                self._set_status(STATUS_SHORTCUTS[key])
                event.stop()
            elif key == "escape":
                self._exit_status_mode()
                event.stop()
            else:
                self._exit_status_mode()

    def _set_status(self, status: str) -> None:
        """Set status of selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id:
            db.update_wad(wad_id, status=status)
            self.notify(f"Status set to {status}")

            # If we have a status filter and the WAD no longer matches, do full reload
            if self._status_filter:
                filters = self._status_filter if isinstance(self._status_filter, list) else [self._status_filter]
                if status not in filters:
                    self._load_wads()
                else:
                    table.update_row(wad_id)
            else:
                table.update_row(wad_id)

            panel = self.query_one("#info-panel", WadInfoPanel)
            panel.update_wad(wad_id)
            # Notify parent that WAD data changed (for other tab refresh)
            self.post_message(self.WadImported(wad_id))

        self._exit_status_mode()

    def _exit_status_mode(self) -> None:
        """Exit status mode."""
        self._status_mode = False
        status_indicator = self.query_one("#status-mode", Static)
        status_indicator.remove_class("visible")
        status_indicator.update("")

    # -------------------------------------------------------------------------
    # Trash View
    # -------------------------------------------------------------------------

    def action_toggle_trash(self) -> None:
        """Toggle trash/deleted view (All tab only)."""
        if self._status_filter is not None:
            self.notify("Trash view is only available in the All tab", severity="warning")
            return

        self._show_deleted = not self._show_deleted
        self._load_wads()

        if self._show_deleted:
            self.notify("Showing trash (u=Restore)")
        else:
            self.notify("Showing library")

    def action_restore_wad(self) -> None:
        """Restore a deleted WAD from trash."""
        if not self._show_deleted:
            return

        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        if db.restore_wad(wad_id):
            self.notify("WAD restored from trash")
            self._load_wads()
            self.post_message(self.WadImported(wad_id))
        else:
            self.notify("Failed to restore WAD", severity="error")

    # -------------------------------------------------------------------------
    # Stats Screen
    # -------------------------------------------------------------------------

    def action_show_stats(self) -> None:
        """Show library stats screen."""
        from caco.tui.screens.stats import StatsScreen
        self.app.push_screen(StatsScreen())

    # -------------------------------------------------------------------------
    # Cache Screen
    # -------------------------------------------------------------------------

    def action_show_cache(self) -> None:
        """Show cache management screen."""
        from caco.tui.screens.cache import CacheScreen
        self.app.push_screen(CacheScreen())

    # -------------------------------------------------------------------------
    # Panel Toggle & Responsive Layout
    # -------------------------------------------------------------------------

    def action_toggle_panel(self) -> None:
        """Toggle info panel visibility."""
        container = self.query_one("#info-panel-container", Vertical)
        container.display = not container.display

    def on_resize(self, event) -> None:
        """Auto-hide info panel on narrow terminals."""
        container = self.query_one("#info-panel-container", Vertical)
        if event.size.width < 100:
            container.display = False
        else:
            container.display = True

    # -------------------------------------------------------------------------
    # Escape
    # -------------------------------------------------------------------------

    def action_escape(self) -> None:
        """Handle escape key."""
        if self._status_mode:
            self._exit_status_mode()
        else:
            filter_widget = self.query_one("#filter", FilterInput)
            if filter_widget.display:
                filter_widget.clear()
                self.query_one("#wad-table", WadTable).focus()

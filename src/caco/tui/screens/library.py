"""Library browser screen."""

import asyncio

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.screen import Screen
from textual.widgets import Footer, Static

from caco import db
from caco.player import play
from caco.tui.widgets.filter_input import FilterInput
from caco.tui.widgets.wad_info import WadInfoPanel
from caco.tui.widgets.wad_table import WadTable


# Status shortcuts for quick status change
STATUS_SHORTCUTS = {
    "p": "playing",
    "f": "finished",
    "t": "to-play",
    "b": "backlog",
    "a": "abandoned",
    "w": "awaiting-update",
}


class LibraryScreen(Screen):
    """Main library browser screen."""

    BINDINGS = [
        # Vim navigation
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
        Binding("g", "handle_g", "Top (gg)", show=False),
        Binding("G", "go_bottom", "Bottom", show=False, key_display="G"),
        Binding("ctrl+d", "page_down", "Page Down", show=False),
        Binding("ctrl+u", "page_up", "Page Up", show=False),
        # Actions
        Binding("slash", "show_filter", "Search", show=True, key_display="/"),
        Binding("enter", "play_wad", "Play", show=True),
        Binding("i", "show_details", "Info", show=True),
        Binding("h", "show_history", "History", show=True),
        # Status shortcuts
        Binding("s", "status_mode", "Status", show=True),
        # Quit
        Binding("q", "quit_or_back", "Quit", show=True),
        Binding("escape", "escape", "Cancel", show=False),
    ]

    def __init__(self) -> None:
        super().__init__()
        self._current_query: str = ""
        self._status_mode = False
        self._g_timeout_task: asyncio.Task | None = None

    def compose(self) -> ComposeResult:
        yield FilterInput(id="filter")
        with Horizontal():
            with Vertical(id="wad-list-container"):
                yield WadTable(id="wad-table")
            with Vertical(id="info-panel-container"):
                yield WadInfoPanel(id="info-panel")
        yield Static("", id="status-mode")
        yield Footer()

    def on_mount(self) -> None:
        """Load initial data."""
        self._load_wads()
        # Focus the table
        self.query_one("#wad-table", WadTable).focus()

    def _load_wads(self) -> None:
        """Load WADs with current query."""
        table = self.query_one("#wad-table", WadTable)
        table.load_wads(self._current_query if self._current_query else None)

    # -------------------------------------------------------------------------
    # Navigation Actions
    # -------------------------------------------------------------------------

    def action_cursor_down(self) -> None:
        """Move cursor down."""
        table = self.query_one("#wad-table", WadTable)
        table.run_worker(table.action_cursor_down())

    def action_cursor_up(self) -> None:
        """Move cursor up."""
        table = self.query_one("#wad-table", WadTable)
        table.run_worker(table.action_cursor_up())

    def action_page_down(self) -> None:
        """Page down."""
        table = self.query_one("#wad-table", WadTable)
        table.run_worker(table.action_page_down())

    def action_page_up(self) -> None:
        """Page up."""
        table = self.query_one("#wad-table", WadTable)
        table.run_worker(table.action_page_up())

    def action_handle_g(self) -> None:
        """Handle 'g' key for gg motion."""
        table = self.query_one("#wad-table", WadTable)

        # Cancel any existing timeout
        if self._g_timeout_task:
            self._g_timeout_task.cancel()

        if table.handle_g_key():
            # Start timeout to reset g state
            self._g_timeout_task = asyncio.create_task(self._g_timeout())

    async def _g_timeout(self) -> None:
        """Reset g state after timeout."""
        await asyncio.sleep(0.5)
        table = self.query_one("#wad-table", WadTable)
        table.reset_g_state()

    def action_go_bottom(self) -> None:
        """Go to bottom of list."""
        table = self.query_one("#wad-table", WadTable)
        table.run_worker(table.action_go_bottom())

    # -------------------------------------------------------------------------
    # Filter Actions
    # -------------------------------------------------------------------------

    def action_show_filter(self) -> None:
        """Show the filter input."""
        filter_widget = self.query_one("#filter", FilterInput)
        filter_widget.show()

    def on_filter_input_query_changed(self, event: FilterInput.QueryChanged) -> None:
        """Handle query change from filter."""
        self._current_query = event.query
        self._load_wads()
        # Refocus table
        self.query_one("#wad-table", WadTable).focus()

    def on_filter_input_escaped(self, event: FilterInput.Escaped) -> None:
        """Handle escape from filter."""
        self.query_one("#wad-table", WadTable).focus()

    # -------------------------------------------------------------------------
    # WAD Selection Events
    # -------------------------------------------------------------------------

    def on_wad_table_wad_selected(self, event: WadTable.WadSelected) -> None:
        """Update info panel when WAD selection changes."""
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(event.wad_id)

    # -------------------------------------------------------------------------
    # Play Action
    # -------------------------------------------------------------------------

    def action_play_wad(self) -> None:
        """Play the selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id is None:
            self.notify("No WAD selected", severity="warning")
            return

        wad = db.get_wad(wad_id)
        if not wad:
            self.notify("WAD not found", severity="error")
            return

        # Suspend the TUI and play
        self.notify(f"Launching {wad['title']}...")

        def play_wad():
            with self.app.suspend():
                try:
                    play(wad_id)
                except ValueError as e:
                    # Will show error after resuming
                    return str(e)
            return None

        # Run in worker to handle the suspend properly
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

        # Refresh data
        self._load_wads()
        panel = self.query_one("#info-panel", WadInfoPanel)
        panel.update_wad(wad_id)

    # -------------------------------------------------------------------------
    # Detail Screens
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
        status_indicator.update("Status: [p]laying [f]inished [t]o-play [b]acklog [a]bandoned [w]aiting")
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
                # Any other key exits status mode
                self._exit_status_mode()

    def _set_status(self, status: str) -> None:
        """Set status of selected WAD."""
        table = self.query_one("#wad-table", WadTable)
        wad_id = table.get_selected_wad_id()

        if wad_id:
            db.update_wad(wad_id, status=status)
            self.notify(f"Status set to {status}")
            self._load_wads()
            panel = self.query_one("#info-panel", WadInfoPanel)
            panel.update_wad(wad_id)

        self._exit_status_mode()

    def _exit_status_mode(self) -> None:
        """Exit status mode."""
        self._status_mode = False
        status_indicator = self.query_one("#status-mode", Static)
        status_indicator.remove_class("visible")
        status_indicator.update("")

    # -------------------------------------------------------------------------
    # Quit/Back
    # -------------------------------------------------------------------------

    def action_quit_or_back(self) -> None:
        """Quit the app."""
        self.app.exit()

    def action_escape(self) -> None:
        """Handle escape key."""
        if self._status_mode:
            self._exit_status_mode()
        else:
            filter_widget = self.query_one("#filter", FilterInput)
            if filter_widget.display:
                filter_widget.clear()
                self.query_one("#wad-table", WadTable).focus()

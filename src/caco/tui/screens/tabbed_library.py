"""Tabbed library browser screen."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import Footer, TabbedContent, TabPane, Tabs

from caco.tui.widgets.idgames_pane import IdgamesSearchPane
from caco.tui.widgets.library_pane import LibraryPane

# Tab IDs in order for cycling
TAB_ORDER = ["tab-all", "tab-playing", "tab-to-play", "tab-finished", "tab-search"]


class TabbedLibraryScreen(Screen):
    """Main library screen with tabs for different status filters."""

    BINDINGS = [
        Binding("tab", "next_tab", "Next Tab", show=False, priority=True),
        Binding("shift+tab", "prev_tab", "Prev Tab", show=False, priority=True),
        Binding("q", "quit_app", "Quit", show=True),
    ]

    DEFAULT_CSS = """
    TabbedLibraryScreen {
        layout: vertical;
    }

    TabbedLibraryScreen TabbedContent {
        height: 1fr;
    }

    TabbedLibraryScreen ContentSwitcher {
        height: 1fr;
    }

    TabbedLibraryScreen TabPane {
        height: 100%;
        padding: 0;
    }
    """

    def compose(self) -> ComposeResult:
        with TabbedContent(id="library-tabs"):
            with TabPane("All", id="tab-all"):
                yield LibraryPane(id="pane-all")
            with TabPane("Playing", id="tab-playing"):
                yield LibraryPane(status_filter="playing", id="pane-playing")
            with TabPane("To-Play", id="tab-to-play"):
                yield LibraryPane(status_filter="to-play", id="pane-to-play")
            with TabPane("Finished", id="tab-finished"):
                yield LibraryPane(status_filter="finished", id="pane-finished")
            with TabPane("Search", id="tab-search"):
                yield IdgamesSearchPane(id="pane-search")
        yield Footer()

    def on_mount(self) -> None:
        """Focus the first tab's table on mount."""
        # The LibraryPane handles its own focus on mount
        pass

    def on_library_pane_wad_imported(self, event: LibraryPane.WadImported) -> None:
        """Refresh all library panes when a WAD status changes or is imported."""
        # Refresh all library panes (not the search pane)
        for pane_id in ("pane-all", "pane-playing", "pane-to-play", "pane-finished"):
            try:
                pane = self.query_one(f"#{pane_id}", LibraryPane)
                pane.refresh_data()
            except Exception:
                pass

    def on_idgames_search_pane_wad_imported(
        self, event: "IdgamesSearchPane.WadImported"
    ) -> None:
        """Refresh library panes when a WAD is imported from idgames search."""
        self.on_library_pane_wad_imported(
            LibraryPane.WadImported(event.wad_id)
        )

    def action_next_tab(self) -> None:
        """Switch to next tab."""
        tabbed = self.query_one("#library-tabs", TabbedContent)
        current = tabbed.active
        if current in TAB_ORDER:
            idx = TAB_ORDER.index(current)
            next_idx = (idx + 1) % len(TAB_ORDER)
            tabbed.active = TAB_ORDER[next_idx]
        self._focus_active_pane()

    def action_prev_tab(self) -> None:
        """Switch to previous tab."""
        tabbed = self.query_one("#library-tabs", TabbedContent)
        current = tabbed.active
        if current in TAB_ORDER:
            idx = TAB_ORDER.index(current)
            prev_idx = (idx - 1) % len(TAB_ORDER)
            tabbed.active = TAB_ORDER[prev_idx]
        self._focus_active_pane()

    def _focus_active_pane(self) -> None:
        """Focus the table in the currently active tab pane."""
        tabbed = self.query_one("#library-tabs", TabbedContent)
        active_tab = tabbed.active
        if active_tab == "tab-search":
            # Focus search input in search tab
            try:
                pane = self.query_one("#pane-search", IdgamesSearchPane)
                pane.query_one("#search-input").focus()
            except Exception:
                pass
        else:
            # Focus the wad table in library panes
            pane_id = active_tab.replace("tab-", "pane-")
            try:
                pane = self.query_one(f"#{pane_id}", LibraryPane)
                pane.query_one("#wad-table").focus()
            except Exception:
                pass

    def action_quit_app(self) -> None:
        """Quit the application."""
        self.app.exit()

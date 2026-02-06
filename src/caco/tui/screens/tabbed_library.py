"""Tabbed library browser screen."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import Footer, TabbedContent, TabPane, Tabs

from caco.config import get_tui_config
from caco.tui.widgets.import_pane import ImportPane
from caco.tui.widgets.library_pane import LibraryPane

# Tab IDs in order for cycling
TAB_ORDER = ["tab-all", "tab-playing", "tab-to-play", "tab-finished", "tab-backlog", "tab-import"]


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
            with TabPane("Backlog", id="tab-backlog"):
                yield LibraryPane(status_filter="backlog", id="pane-backlog")
            with TabPane("Import", id="tab-import"):
                yield ImportPane(id="pane-import")
        yield Footer()

    def on_mount(self) -> None:
        """Set the default tab from config and focus."""
        tui_config = get_tui_config()
        default_tab = tui_config.get("default_tab", "all")
        tab_id = f"tab-{default_tab}"
        if tab_id in TAB_ORDER:
            tabbed = self.query_one("#library-tabs", TabbedContent)
            tabbed.active = tab_id

    def on_library_pane_wad_imported(self, event: LibraryPane.WadImported) -> None:
        """Refresh all library panes when a WAD status changes or is imported."""
        # Refresh all library panes (not the import pane)
        for pane_id in ("pane-all", "pane-playing", "pane-to-play", "pane-finished", "pane-backlog"):
            try:
                pane = self.query_one(f"#{pane_id}", LibraryPane)
                pane.refresh_data()
            except Exception:
                pass

    def on_import_pane_wad_imported(
        self, event: "ImportPane.WadImported"
    ) -> None:
        """Refresh library panes when a WAD is imported from any import source."""
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
        """Focus the appropriate element in the currently active tab pane."""
        tabbed = self.query_one("#library-tabs", TabbedContent)
        active_tab = tabbed.active
        if active_tab == "tab-import":
            # Import tab handles its own focus via source selector
            try:
                pane = self.query_one("#pane-import", ImportPane)
                # Try to focus the search input of the active import source
                pane._focus_active_pane(pane.query_one("#import-content").current or "source-idgames")
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

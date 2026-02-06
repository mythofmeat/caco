"""Import pane with source selector for the TUI."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal
from textual.message import Message
from textual.widget import Widget
from textual.widgets import ContentSwitcher, RadioButton, RadioSet

from caco.tui.widgets.base_search_pane import BaseSearchPane
from caco.tui.widgets.idgames_pane import IdgamesSearchPane
from caco.tui.widgets.doomwiki_pane import DoomwikiSearchPane
from caco.tui.widgets.doomworld_pane import DoomworldUrlPane
from caco.tui.widgets.url_pane import UrlImportPane
from caco.tui.widgets.local_pane import LocalImportPane


# Source IDs for ContentSwitcher
SOURCE_IDGAMES = "source-idgames"
SOURCE_DOOMWIKI = "source-doomwiki"
SOURCE_DOOMWORLD = "source-doomworld"
SOURCE_URL = "source-url"
SOURCE_LOCAL = "source-local"

# Source order for number key shortcuts
SOURCE_ORDER = [SOURCE_IDGAMES, SOURCE_DOOMWIKI, SOURCE_DOOMWORLD, SOURCE_URL, SOURCE_LOCAL]


class ImportPane(Widget):
    """Container pane with source selector and content switcher for imports."""

    BINDINGS = [
        Binding("1", "select_source('source-idgames')", "idgames", show=True),
        Binding("2", "select_source('source-doomwiki')", "Doomwiki", show=True),
        Binding("3", "select_source('source-doomworld')", "Doomworld", show=True),
        Binding("4", "select_source('source-url')", "URL", show=True),
        Binding("5", "select_source('source-local')", "Local", show=True),
    ]

    class WadImported(Message):
        """Fired when a WAD is successfully imported from any source."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    DEFAULT_CSS = """
    ImportPane {
        height: 100%;
        width: 100%;
    }

    ImportPane #source-selector {
        height: 3;
        width: 100%;
        padding: 0 1;
        align: left middle;
        background: $surface-darken-1;
    }

    ImportPane #source-selector RadioSet {
        width: auto;
    }

    ImportPane #source-selector RadioButton {
        padding: 0 1;
    }

    ImportPane #import-content {
        height: 1fr;
    }
    """

    def compose(self) -> ComposeResult:
        with Horizontal(id="source-selector"):
            with RadioSet(id="source-radio"):
                yield RadioButton("idgames", id="radio-idgames", value=True)
                yield RadioButton("Doomwiki", id="radio-doomwiki")
                yield RadioButton("Doomworld", id="radio-doomworld")
                yield RadioButton("URL", id="radio-url")
                yield RadioButton("Local", id="radio-local")
        with ContentSwitcher(id="import-content", initial=SOURCE_IDGAMES):
            yield IdgamesSearchPane(id=SOURCE_IDGAMES)
            yield DoomwikiSearchPane(id=SOURCE_DOOMWIKI)
            yield DoomworldUrlPane(id=SOURCE_DOOMWORLD)
            yield UrlImportPane(id=SOURCE_URL)
            yield LocalImportPane(id=SOURCE_LOCAL)

    def on_radio_set_changed(self, event: RadioSet.Changed) -> None:
        """Switch content when radio button changes."""
        radio_to_source = {
            "radio-idgames": SOURCE_IDGAMES,
            "radio-doomwiki": SOURCE_DOOMWIKI,
            "radio-doomworld": SOURCE_DOOMWORLD,
            "radio-url": SOURCE_URL,
            "radio-local": SOURCE_LOCAL,
        }
        if event.pressed and event.pressed.id:
            source_id = radio_to_source.get(event.pressed.id, SOURCE_IDGAMES)
            switcher = self.query_one("#import-content", ContentSwitcher)
            switcher.current = source_id
            self._focus_active_pane(source_id)

    def _focus_active_pane(self, source_id: str) -> None:
        """Focus the appropriate element in the active pane.

        For search-based panes, focus the results table (allows 1-5 source switching).
        For form-based panes, focus the source selector (Tab to enter form).
        """
        try:
            if source_id in (SOURCE_IDGAMES, SOURCE_DOOMWIKI):
                # Focus results table for search-based panes (allows number key shortcuts)
                pane = self.query_one(f"#{source_id}")
                pane.query_one("#results-table").focus()
            else:
                # For form-based panes, focus the source selector
                # This allows 1-5 switching; user can Tab into the form when ready
                self.query_one("#source-radio", RadioSet).focus()
        except Exception:
            pass

    def action_select_source(self, source_id: str) -> None:
        """Select a source by ID (for number key shortcuts)."""
        source_to_radio = {
            SOURCE_IDGAMES: "radio-idgames",
            SOURCE_DOOMWIKI: "radio-doomwiki",
            SOURCE_DOOMWORLD: "radio-doomworld",
            SOURCE_URL: "radio-url",
            SOURCE_LOCAL: "radio-local",
        }
        radio_id = source_to_radio.get(source_id)
        if radio_id:
            try:
                radio_set = self.query_one("#source-radio", RadioSet)
                radio = self.query_one(f"#{radio_id}", RadioButton)
                # Press the radio button to trigger the change event
                radio_set.focus()
                radio.toggle()
            except Exception:
                pass

    # Bubble up WadImported messages from child panes
    def on_base_search_pane_wad_imported(
        self, event: BaseSearchPane.WadImported
    ) -> None:
        """Relay import event from idgames or doomwiki search panes."""
        self.post_message(self.WadImported(event.wad_id))

    def on_doomworld_url_pane_wad_imported(
        self, event: DoomworldUrlPane.WadImported
    ) -> None:
        """Relay import event from doomworld pane."""
        self.post_message(self.WadImported(event.wad_id))

    def on_url_import_pane_wad_imported(
        self, event: UrlImportPane.WadImported
    ) -> None:
        """Relay import event from URL pane."""
        self.post_message(self.WadImported(event.wad_id))

    def on_local_import_pane_wad_imported(
        self, event: LocalImportPane.WadImported
    ) -> None:
        """Relay import event from local pane."""
        self.post_message(self.WadImported(event.wad_id))

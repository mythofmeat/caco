"""Import tab with five source panes."""

from PySide6.QtCore import Signal
from PySide6.QtWidgets import QWidget, QVBoxLayout, QTabWidget

from caco.gui.import_panes.idgames_pane import IdgamesPane
from caco.gui.import_panes.doomwiki_pane import DoomwikiPane
from caco.gui.import_panes.doomworld_pane import DoomworldPane
from caco.gui.import_panes.url_pane import UrlPane
from caco.gui.import_panes.local_pane import LocalPane


class ImportTab(QWidget):
    """Tab widget containing all five import source panes.

    Emits wad_imported when any source successfully imports a WAD,
    so the parent can refresh the library view.
    """

    wad_imported = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)

        self._tabs = QTabWidget()

        # idgames
        self._idgames = IdgamesPane()
        self._idgames.wad_imported.connect(self.wad_imported)
        self._tabs.addTab(self._idgames, "idgames")

        # Doom Wiki
        self._doomwiki = DoomwikiPane()
        self._doomwiki.wad_imported.connect(self.wad_imported)
        self._tabs.addTab(self._doomwiki, "Doom Wiki")

        # Doomworld
        self._doomworld = DoomworldPane()
        self._doomworld.wad_imported.connect(self.wad_imported)
        self._tabs.addTab(self._doomworld, "Doomworld")

        # URL
        self._url = UrlPane()
        self._url.wad_imported.connect(self.wad_imported)
        self._tabs.addTab(self._url, "URL")

        # Local
        self._local = LocalPane()
        self._local.wad_imported.connect(self.wad_imported)
        self._tabs.addTab(self._local, "Local")

        layout.addWidget(self._tabs)

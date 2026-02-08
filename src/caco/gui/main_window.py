"""Main application window with tab bar, toolbar, and status bar."""

from PySide6.QtCore import Qt, QSettings
from PySide6.QtGui import QKeySequence, QShortcut
from PySide6.QtWidgets import (
    QMainWindow,
    QTabBar,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
    QStatusBar,
    QMessageBox,
)

from caco import db
from caco.gui.constants import STATUS_TABS, Column, ALL_COLUMNS, DEFAULT_COLUMNS
from caco.gui.tabs.library_tab import LibraryTab
from caco.gui.tabs.import_tab import ImportTab
from caco.gui.dialogs.edit_dialog import EditDialog
from caco.gui.dialogs.delete_dialog import DeleteDialog
from caco.gui.dialogs.sessions_dialog import SessionsDialog
from caco.gui.dialogs.stats_dialog import StatsDialog
from caco.gui.dialogs.cache_dialog import CacheDialog
from caco.gui.workers.play_worker import PlayWorker
from caco.gui.theme import DOOM_PALETTE


class MainWindow(QMainWindow):
    """Caco GUI main window.

    Layout:
    - Tab bar across the top (status filters + Import)
    - Content area switches between LibraryTab and ImportTab
    - Status bar at the bottom
    """

    def __init__(self, config: dict | None = None):
        super().__init__()
        self._config = config or {}
        self._play_worker: PlayWorker | None = None

        self.setWindowTitle("Caco - Doom WAD Library")
        self.resize(
            self._config.get("window_width", 1200),
            self._config.get("window_height", 800),
        )

        # Central widget
        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        # -- Tab bar --
        self._tab_bar = QTabBar()
        self._tab_bar.setExpanding(False)
        self._tab_bar.setDrawBase(False)

        # Add status filter tabs
        for label, _query in STATUS_TABS:
            self._tab_bar.addTab(label)

        # Add Import tab
        self._import_tab_index = self._tab_bar.addTab("Import")

        self._tab_bar.currentChanged.connect(self._on_tab_changed)
        layout.addWidget(self._tab_bar)

        # -- Stacked content --
        self._stack = QStackedWidget()

        # Library tab (shared across all status filter tabs)
        self._library_tab = LibraryTab()
        self._library_tab.play_requested.connect(self._on_play)
        self._library_tab.edit_requested.connect(self._on_edit)
        self._library_tab.delete_requested.connect(self._on_delete)
        self._library_tab.sessions_requested.connect(self._on_sessions)
        self._library_tab.status_message.connect(self._update_status)
        self._stack.addWidget(self._library_tab)

        # Import tab
        self._import_tab = ImportTab()
        self._import_tab.wad_imported.connect(self._on_wad_imported)
        self._stack.addWidget(self._import_tab)

        layout.addWidget(self._stack)

        # -- Status bar --
        self._status_bar = QStatusBar()
        self.setStatusBar(self._status_bar)

        # -- Keyboard shortcuts --
        self._setup_shortcuts()

        # Apply default tab from config
        default_tab = self._config.get("default_tab", "all")
        self._apply_default_tab(default_tab)

        # Apply default sort from config
        default_sort = self._config.get("default_sort", "id")
        default_sort_desc = self._config.get("default_sort_desc", False)
        self._library_tab._sort.set_sort(default_sort, default_sort_desc)

        # QSettings for persistent state
        self._settings = QSettings("caco", "caco-gui")

        # Restore saved column visibility
        self._restore_columns()

        # Listen for column changes to persist them
        self._library_tab._list_view.columns_changed.connect(self._on_columns_changed)

        # Restore saved geometry
        self._restore_geometry()

    def _setup_shortcuts(self):
        """Global keyboard shortcuts."""
        QShortcut(QKeySequence("Ctrl+F"), self, self._focus_filter)
        QShortcut(QKeySequence("Escape"), self, self._on_escape)
        QShortcut(QKeySequence("F5"), self, self._library_tab.refresh)
        QShortcut(QKeySequence("Ctrl+S"), self, self._on_stats)
        QShortcut(QKeySequence("Ctrl+K"), self, self._on_cache)
        for i in range(min(9, self._tab_bar.count())):
            QShortcut(
                QKeySequence(f"Alt+{i + 1}"),
                self,
                lambda idx=i: self._tab_bar.setCurrentIndex(idx),
            )

    def _apply_default_tab(self, tab_name: str):
        """Select the default tab by name."""
        tab_map = {label.lower(): i for i, (label, _) in enumerate(STATUS_TABS)}
        index = tab_map.get(tab_name.lower(), 0)
        self._tab_bar.setCurrentIndex(index)

    def _on_tab_changed(self, index: int):
        """Handle tab bar changes."""
        if index == self._import_tab_index:
            self._stack.setCurrentWidget(self._import_tab)
        else:
            self._stack.setCurrentWidget(self._library_tab)
            if 0 <= index < len(STATUS_TABS):
                _, query = STATUS_TABS[index]
                self._library_tab.set_tab_query(query)

    def _update_status(self, msg: str):
        """Update the status bar message."""
        self._status_bar.showMessage(msg)

    def _focus_filter(self):
        """Focus the filter input."""
        if self._stack.currentWidget() == self._library_tab:
            self._library_tab.focus_filter()

    def _on_escape(self):
        """Handle Escape: clear filter or deselect."""
        if self._stack.currentWidget() == self._library_tab:
            self._library_tab.focus_filter()

    # ── Action handlers ────────────────────────────────────────────

    def _on_play(self, wad_id: int):
        """Launch sourceport in a background thread."""
        if self._play_worker and self._play_worker.isRunning():
            QMessageBox.information(
                self, "Already Playing",
                "A sourceport is already running. Wait for it to finish."
            )
            return

        wad = db.get_wad(wad_id)
        if not wad:
            return

        self._status_bar.showMessage(f"Launching {wad['title']}...")
        self._play_worker = PlayWorker(wad_id, parent=self)
        self._play_worker.finished.connect(self._on_play_finished)
        self._play_worker.error.connect(self._on_play_error)
        self._play_worker.start()

    def _on_play_finished(self, wad_id: int, duration):
        """Called when sourceport exits normally."""
        from caco.player import format_duration
        if duration:
            self._status_bar.showMessage(
                f"Session ended ({format_duration(duration)})", 5000
            )
        else:
            self._status_bar.showMessage("Session ended", 5000)
        self._library_tab.refresh()

    def _on_play_error(self, wad_id: int, error_msg: str):
        """Called when play fails."""
        QMessageBox.warning(self, "Cannot Play", error_msg)
        self._status_bar.clearMessage()

    def _on_edit(self, wad_id: int):
        """Open the edit dialog for a WAD."""
        dialog = EditDialog(wad_id, parent=self)
        if dialog.exec() == EditDialog.Accepted:
            self._library_tab.refresh()
            self._library_tab.select_wad(wad_id)

    def _on_delete(self, wad_id: int):
        """Open the delete confirmation dialog."""
        dialog = DeleteDialog(wad_id, parent=self)
        if dialog.exec() == DeleteDialog.Accepted:
            db.delete_wad(wad_id)
            self._library_tab.refresh()

    def _on_sessions(self, wad_id: int):
        """Open the session history dialog."""
        dialog = SessionsDialog(wad_id, parent=self)
        dialog.exec()

    def _on_wad_imported(self, wad_id: int):
        """Called when a WAD is imported from any source."""
        self._library_tab.refresh()
        self._status_bar.showMessage(f"WAD imported (ID: {wad_id})", 5000)

    def _on_stats(self):
        """Open the library statistics dialog."""
        dialog = StatsDialog(parent=self)
        dialog.exec()

    def _on_cache(self):
        """Open the cache management dialog."""
        dialog = CacheDialog(parent=self)
        dialog.exec()

    # ── Column visibility persistence ─────────────────────────────

    def _restore_columns(self):
        """Restore saved column visibility from QSettings."""
        saved = self._settings.value("visibleColumns")
        if saved and isinstance(saved, list):
            col_map = {c.name: c for c in ALL_COLUMNS}
            columns = [col_map[name] for name in saved if name in col_map]
            if columns:
                self._library_tab._model.set_columns(columns)
                self._library_tab._list_view._apply_column_widths()

    def _on_columns_changed(self, columns):
        """Persist column visibility when user changes it."""
        names = [c.name for c in columns]
        self._settings.setValue("visibleColumns", names)

    # ── Window geometry save/restore ──────────────────────────────

    def _restore_geometry(self):
        """Restore window geometry and splitter state from QSettings."""
        geometry = self._settings.value("geometry")
        if geometry:
            self.restoreGeometry(geometry)
        state = self._settings.value("windowState")
        if state:
            self.restoreState(state)
        splitter_state = self._settings.value("splitterState")
        if splitter_state:
            self._library_tab._splitter.restoreState(splitter_state)

    def closeEvent(self, event):
        """Save window geometry on close."""
        self._settings.setValue("geometry", self.saveGeometry())
        self._settings.setValue("windowState", self.saveState())
        self._settings.setValue("splitterState", self._library_tab._splitter.saveState())
        super().closeEvent(event)

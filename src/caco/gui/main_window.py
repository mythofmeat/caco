"""Main application window with tab bar, toolbar, and status bar."""

from PySide6.QtCore import Qt, QSettings
from PySide6.QtGui import QKeySequence, QShortcut, QAction
from PySide6.QtWidgets import (
    QMainWindow,
    QTabBar,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
    QStatusBar,
    QMessageBox,
    QMenu,
    QProgressBar,
    QDialog,
)

from caco import db
from caco.gui.constants import STATUS_TABS, Column, ALL_COLUMNS, DEFAULT_COLUMNS
from caco.gui.tabs.library_tab import LibraryTab
from caco.gui.tabs.import_tab import ImportTab
from caco.gui.dialogs.edit_dialog import (
    EditMetadataDialog,
    EditNotesDialog,
    EditSourceportDialog,
    EditCompanionsDialog,
)
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

    @staticmethod
    def _ensure_list(value) -> list | None:
        """Normalize QSettings value to a list.

        QSettings deserializes single-element lists as bare strings,
        so ["myTab"] comes back as "myTab" instead of a list.
        """
        if value is None:
            return None
        if isinstance(value, list):
            return value
        if isinstance(value, str):
            return [value]
        return None

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
        self._tab_bar.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self._tab_bar.customContextMenuRequested.connect(self._show_tab_context_menu)

        # Track which STATUS_TABS indices are visible (all by default)
        self._visible_status_tabs: list[int] = list(range(len(STATUS_TABS)))
        # Custom user-defined tabs: list of (name, query) tuples
        self._custom_tabs: list[tuple[str, str]] = []
        # Maps tab bar position → ("status", STATUS_TABS index) or ("custom", custom_tabs index)
        self._tab_mapping: list[tuple[str, int]] = []
        self._import_tab_index = -1

        self._rebuild_tab_bar()
        self._tab_bar.currentChanged.connect(self._on_tab_changed)
        layout.addWidget(self._tab_bar)

        # -- Stacked content --
        self._stack = QStackedWidget()

        # Library tab (shared across all status filter tabs)
        self._library_tab = LibraryTab()
        self._library_tab.play_requested.connect(self._on_play)
        self._library_tab.edit_metadata_requested.connect(self._on_edit_metadata)
        self._library_tab.edit_notes_requested.connect(self._on_edit_notes)
        self._library_tab.edit_sourceport_requested.connect(self._on_edit_sourceport)
        self._library_tab.edit_companions_requested.connect(self._on_edit_companions)
        self._library_tab.delete_requested.connect(self._on_delete)
        self._library_tab.sessions_requested.connect(self._on_sessions)
        self._library_tab.wad_stats_requested.connect(self._on_wad_stats)
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

        # Download progress bar (hidden until a download starts)
        self._download_bar = QProgressBar()
        self._download_bar.setFixedWidth(200)
        self._download_bar.setTextVisible(True)
        self._download_bar.hide()
        self._status_bar.addPermanentWidget(self._download_bar)

        # -- Menu bar --
        self._setup_menu_bar()

        # -- Keyboard shortcuts --
        self._setup_shortcuts()

        # QSettings for persistent state (needed before tab restore)
        self._settings = QSettings("caco", "caco-gui")

        # Restore sort and view BEFORE tab restore, so the first
        # tab-triggered refresh() reads the correct sort/view state.
        saved_sort = self._settings.value("sortField")
        saved_desc = self._settings.value("sortDesc")
        sort_field = saved_sort if isinstance(saved_sort, str) else self._config.get("default_sort", "id")
        sort_desc = (saved_desc == "true" or saved_desc is True) if saved_desc is not None else self._config.get("default_sort_desc", False)
        self._library_tab.set_sort(sort_field, sort_desc)

        saved_view = self._settings.value("viewType")
        view_type = saved_view if isinstance(saved_view, str) else self._config.get("default_view", "list")
        if view_type == "grid":
            self._library_tab.toggle_view()

        # Restore saved column visibility
        self._restore_columns()

        # Restore last active tab (triggers refresh with correct sort/view)
        self._restore_last_tab()

        # Restore saved card size for grid view
        self._restore_card_size()

        # Restore saved searches
        self._restore_saved_searches()

        # Listen for column changes to persist them
        self._library_tab.columns_changed.connect(self._on_columns_changed)

        # Listen for card size changes to persist them
        self._library_tab.card_size_changed.connect(self._on_card_size_changed)

        # Listen for saved searches changes to persist them
        self._library_tab.saved_searches_changed.connect(self._on_saved_searches_changed)

        # Listen for "save as tab" requests
        self._library_tab.save_as_tab_requested.connect(self._add_custom_tab)

        # Restore saved geometry
        self._restore_geometry()

    def _setup_menu_bar(self):
        """Set up the application menu bar."""
        menu_bar = self.menuBar()

        # Tools menu
        tools_menu = menu_bar.addMenu("&Tools")

        resources_action = tools_menu.addAction("&Resources...")
        resources_action.setShortcut(QKeySequence("Ctrl+R"))
        resources_action.triggered.connect(self._on_resources)

        tools_menu.addSeparator()

        stats_action = tools_menu.addAction("Library &Statistics...")
        stats_action.setShortcut(QKeySequence("Ctrl+S"))
        stats_action.triggered.connect(self._on_stats)

        cache_action = tools_menu.addAction("Cache &Management...")
        cache_action.setShortcut(QKeySequence("Ctrl+K"))
        cache_action.triggered.connect(self._on_cache)

    def _setup_shortcuts(self):
        """Global keyboard shortcuts."""
        QShortcut(QKeySequence("Ctrl+F"), self, self._focus_filter)
        QShortcut(QKeySequence("Escape"), self, self._on_escape)
        QShortcut(QKeySequence("F5"), self, self._library_tab.refresh)
        for i in range(min(9, self._tab_bar.count())):
            QShortcut(
                QKeySequence(f"Alt+{i + 1}"),
                self,
                lambda idx=i: self._tab_bar.setCurrentIndex(idx),
            )

    def _rebuild_tab_bar(self):
        """Rebuild the tab bar: status tabs + custom tabs + Import."""
        self._tab_bar.blockSignals(True)
        while self._tab_bar.count():
            self._tab_bar.removeTab(0)

        self._tab_mapping = []
        for i in self._visible_status_tabs:
            label, _ = STATUS_TABS[i]
            self._tab_bar.addTab(label)
            self._tab_mapping.append(("status", i))

        for i, (name, _query) in enumerate(self._custom_tabs):
            self._tab_bar.addTab(name)
            self._tab_mapping.append(("custom", i))

        # Import tab is always last
        self._import_tab_index = self._tab_bar.addTab("Import")
        self._tab_bar.blockSignals(False)

    def _tab_bar_index_for_name(self, name: str) -> int:
        """Find tab bar index for a tab name (case-insensitive)."""
        name_lower = name.lower()
        for bar_idx, (kind, idx) in enumerate(self._tab_mapping):
            if kind == "status" and STATUS_TABS[idx][0].lower() == name_lower:
                return bar_idx
            elif kind == "custom" and self._custom_tabs[idx][0].lower() == name_lower:
                return bar_idx
        return 0

    def _restore_last_tab(self):
        """Restore the last active tab from QSettings, falling back to config."""
        # Restore visible status tabs
        saved_visible = self._ensure_list(self._settings.value("visibleTabs"))
        if saved_visible:
            name_to_idx = {label.lower(): i for i, (label, _) in enumerate(STATUS_TABS)}
            restored = [name_to_idx[n.lower()] for n in saved_visible if n.lower() in name_to_idx]
            if 0 not in restored:
                restored.insert(0, 0)
            if restored:
                self._visible_status_tabs = sorted(restored)

        # Restore custom tabs
        ct_names = self._ensure_list(self._settings.value("customTabNames"))
        ct_queries = self._ensure_list(self._settings.value("customTabQueries"))
        if ct_names and ct_queries:
            self._custom_tabs = list(zip(ct_names, ct_queries))

        self._rebuild_tab_bar()

        # Restore last active tab by name
        saved_tab = self._settings.value("lastTabName")
        if saved_tab and isinstance(saved_tab, str):
            if saved_tab.lower() == "import":
                self._tab_bar.setCurrentIndex(self._import_tab_index)
                return
            idx = self._tab_bar_index_for_name(saved_tab)
            self._tab_bar.setCurrentIndex(idx)
            return

        # Fall back to config default
        default_tab = self._config.get("default_tab", "all")
        self._tab_bar.setCurrentIndex(self._tab_bar_index_for_name(default_tab))

    def _on_tab_changed(self, index: int):
        """Handle tab bar changes."""
        if index == self._import_tab_index:
            self._stack.setCurrentWidget(self._import_tab)
        elif 0 <= index < len(self._tab_mapping):
            self._stack.setCurrentWidget(self._library_tab)
            kind, idx = self._tab_mapping[index]
            if kind == "status":
                _, query = STATUS_TABS[idx]
            else:
                _, query = self._custom_tabs[idx]
            self._library_tab.set_tab_query(query)

    def _show_tab_context_menu(self, pos):
        """Right-click on tab bar: show/hide status tabs, manage custom tabs."""
        menu = QMenu(self)

        # Status tab visibility
        for i, (label, _query) in enumerate(STATUS_TABS):
            action = QAction(label, self)
            action.setCheckable(True)
            action.setChecked(i in self._visible_status_tabs)
            if i == 0:
                action.setEnabled(False)
            action.toggled.connect(lambda checked, idx=i: self._toggle_tab(idx, checked))
            menu.addAction(action)

        # Custom tab removal
        if self._custom_tabs:
            menu.addSeparator()
            remove_menu = menu.addMenu("Remove custom tab...")
            for name, _query in self._custom_tabs:
                rm_action = remove_menu.addAction(name)
                rm_action.triggered.connect(lambda checked, n=name: self._remove_custom_tab(n))

        menu.exec(self._tab_bar.mapToGlobal(pos))

    def _current_tab_name(self) -> str | None:
        """Get the name of the currently selected tab."""
        cur = self._tab_bar.currentIndex()
        if cur == self._import_tab_index:
            return "Import"
        if 0 <= cur < len(self._tab_mapping):
            kind, idx = self._tab_mapping[cur]
            if kind == "status":
                return STATUS_TABS[idx][0]
            return self._custom_tabs[idx][0]
        return None

    def _select_tab_by_name(self, name: str | None):
        """Select a tab by name after a rebuild."""
        if not name:
            return
        if name == "Import":
            self._tab_bar.setCurrentIndex(self._import_tab_index)
        else:
            self._tab_bar.setCurrentIndex(self._tab_bar_index_for_name(name))

    def _toggle_tab(self, status_idx: int, visible: bool):
        """Show or hide a status tab."""
        if visible and status_idx not in self._visible_status_tabs:
            self._visible_status_tabs.append(status_idx)
            self._visible_status_tabs.sort()
        elif not visible and status_idx in self._visible_status_tabs:
            if len(self._visible_status_tabs) <= 1:
                return
            self._visible_status_tabs.remove(status_idx)

        cur_name = self._current_tab_name()
        self._rebuild_tab_bar()
        self._select_tab_by_name(cur_name)

    def _add_custom_tab(self, name: str, query: str):
        """Add a custom tab with the given name and query."""
        # Replace if name already exists
        self._custom_tabs = [(n, q) for n, q in self._custom_tabs if n != name]
        self._custom_tabs.append((name, query))

        self._rebuild_tab_bar()
        # Switch to the new tab
        self._tab_bar.setCurrentIndex(self._tab_bar_index_for_name(name))
        self._persist_custom_tabs()

    def _remove_custom_tab(self, name: str):
        """Remove a custom tab by name."""
        cur_name = self._current_tab_name()
        self._custom_tabs = [(n, q) for n, q in self._custom_tabs if n != name]

        self._rebuild_tab_bar()
        # If we removed the active tab, go to "All"
        if cur_name == name:
            self._tab_bar.setCurrentIndex(0)
        else:
            self._select_tab_by_name(cur_name)
        self._persist_custom_tabs()

    def _persist_custom_tabs(self):
        """Save custom tabs to QSettings."""
        names = [n for n, _ in self._custom_tabs]
        queries = [q for _, q in self._custom_tabs]
        self._settings.setValue("customTabNames", names)
        self._settings.setValue("customTabQueries", queries)

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

    @staticmethod
    def _can_resolve_wad(wad: dict) -> bool:
        """Check if a WAD file can be obtained without showing the unavailable dialog.

        Mirrors the preconditions in player.get_wad_path() without side effects
        (no downloads, no DB writes).
        """
        from pathlib import Path

        if wad.get("cached_path") and Path(wad["cached_path"]).exists():
            return True
        if wad.get("idgames_id"):
            return True
        if wad["source_type"] == "idgames" and wad.get("source_id"):
            return True
        if wad["source_type"] == "local" and wad.get("source_url") and Path(wad["source_url"]).exists():
            return True
        return False

    def _on_play(self, wad_id: int):
        """Launch sourceport in a background thread."""
        if self._play_worker and self._play_worker.isRunning():
            reply = QMessageBox.question(
                self, "Already Playing",
                "A sourceport is already running.\n\n"
                "Do you want to force-stop it?",
                QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
                QMessageBox.StandardButton.No,
            )
            if reply == QMessageBox.StandardButton.Yes:
                self._play_worker.stop_sourceport()
            return

        wad = db.get_wad(wad_id)
        if not wad:
            return

        # Pre-check: can we resolve the WAD file?
        if not self._can_resolve_wad(wad):
            from caco.gui.dialogs.link_dialog import WadUnavailableDialog

            dialog = WadUnavailableDialog(wad_id, parent=self)
            if dialog.exec() == QDialog.DialogCode.Accepted:
                self._library_tab.refresh()
                self._library_tab.select_wad(wad_id)
                self._on_play(wad_id)
            return

        self._status_bar.showMessage(f"Launching {wad['title']}...")
        self._play_worker = PlayWorker(wad_id, parent=self)
        self._play_worker.finished.connect(self._on_play_finished)
        self._play_worker.error.connect(self._on_play_error)
        self._play_worker.download_progress.connect(self._on_download_progress)
        self._play_worker.start()

    def _on_download_progress(self, downloaded: int, total: int, filename: str):
        """Update status bar progress during WAD download."""
        if not self._download_bar.isVisible():
            self._download_bar.show()
            self._status_bar.showMessage(f"Downloading {filename}...")
        if total > 0:
            self._download_bar.setMaximum(total)
            self._download_bar.setValue(downloaded)
        else:
            # Unknown total: use indeterminate mode
            self._download_bar.setMaximum(0)

    def _on_play_finished(self, wad_id: int, result):
        """Called when sourceport exits."""
        self._download_bar.hide()
        from caco.player import format_duration, PlayResult

        if isinstance(result, PlayResult) and result.duration:
            self._status_bar.showMessage(
                f"Session ended ({format_duration(result.duration)})", 5000
            )
        else:
            self._status_bar.showMessage("Session ended", 5000)

        if isinstance(result, PlayResult) and result.crashed:
            QMessageBox.warning(
                self,
                "Sourceport Crash",
                f"Sourceport exited with code {result.exit_code}.\n\n"
                "The session was still recorded.",
            )

        self._library_tab.refresh()

    def _on_play_error(self, wad_id: int, error_msg: str):
        """Called when play fails."""
        self._download_bar.hide()
        QMessageBox.warning(self, "Cannot Play", error_msg)
        self._status_bar.clearMessage()

    def _exec_edit_dialog(self, wad_id: int, dialog: QDialog):
        """Execute an edit dialog and refresh the library on accept."""
        if not getattr(dialog, "_wad", None):
            return
        if dialog.exec() == QDialog.DialogCode.Accepted:
            self._library_tab.refresh()
            self._library_tab.select_wad(wad_id)

    def _on_edit_metadata(self, wad_id: int):
        """Open the metadata editing dialog."""
        self._exec_edit_dialog(wad_id, EditMetadataDialog(wad_id, parent=self))

    def _on_edit_notes(self, wad_id: int):
        """Open the notes editing dialog."""
        self._exec_edit_dialog(wad_id, EditNotesDialog(wad_id, parent=self))

    def _on_edit_sourceport(self, wad_id: int):
        """Open the sourceport settings dialog."""
        self._exec_edit_dialog(wad_id, EditSourceportDialog(wad_id, parent=self))

    def _on_edit_companions(self, wad_id: int):
        """Open the companion files dialog."""
        self._exec_edit_dialog(wad_id, EditCompanionsDialog(wad_id, parent=self))

    def _on_delete(self, wad_id: int):
        """Open the delete confirmation dialog."""
        dialog = DeleteDialog(wad_id, parent=self)
        if dialog.exec() == QDialog.DialogCode.Accepted:
            db.delete_wad(wad_id)
            self._library_tab.refresh()

    def _on_sessions(self, wad_id: int):
        """Open the session history dialog."""
        dialog = SessionsDialog(wad_id, parent=self)
        dialog.exec()

    def _on_wad_stats(self, wad_id: int):
        """Open the per-map stats dialog."""
        from caco.gui.dialogs.wad_stats_dialog import WadStatsDialog

        dialog = WadStatsDialog(wad_id, parent=self)
        dialog.exec()
        if dialog.changed:
            self._library_tab.refresh()
            self._library_tab.select_wad(wad_id)

    def _on_wad_imported(self, wad_id: int):
        """Called when a WAD is imported from any source."""
        self._library_tab.refresh()
        self._status_bar.showMessage(f"WAD imported (ID: {wad_id})", 5000)

    def _on_resources(self):
        """Open the IWAD/id24 resource management dialog."""
        from caco.gui.dialogs.resources_dialog import ResourcesDialog

        dialog = ResourcesDialog(parent=self)
        dialog.exec()

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
        saved = self._ensure_list(self._settings.value("visibleColumns"))
        if saved:
            col_map = {c.name: c for c in ALL_COLUMNS}
            columns = [col_map[name] for name in saved if name in col_map]
            if columns:
                self._library_tab.set_visible_columns(columns)

    def _on_columns_changed(self, columns):
        """Persist column visibility when user changes it."""
        names = [c.name for c in columns]
        self._settings.setValue("visibleColumns", names)

    # ── Card size persistence ─────────────────────────────────────

    def _restore_card_size(self):
        """Restore saved grid card size from QSettings."""
        saved = self._settings.value("cardSize")
        if saved is not None:
            try:
                self._library_tab.set_card_size(int(saved))
            except (ValueError, TypeError):
                pass

    def _on_card_size_changed(self, width: int):
        """Persist card size when user adjusts slider."""
        self._settings.setValue("cardSize", width)

    # ── Saved searches persistence ────────────────────────────────

    def _restore_saved_searches(self):
        """Restore saved searches from QSettings."""
        names = self._ensure_list(self._settings.value("savedSearchNames"))
        queries = self._ensure_list(self._settings.value("savedSearchQueries"))
        if names and queries:
            searches = list(zip(names, queries))
            self._library_tab.set_saved_searches(searches)

    def _on_saved_searches_changed(self, searches: list):
        """Persist saved searches when user adds/deletes one."""
        names = [s[0] for s in searches]
        queries = [s[1] for s in searches]
        self._settings.setValue("savedSearchNames", names)
        self._settings.setValue("savedSearchQueries", queries)

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
            self._library_tab.restore_splitter_state(splitter_state)

    def closeEvent(self, event):
        """Save window state on close."""
        self._settings.setValue("geometry", self.saveGeometry())
        self._settings.setValue("windowState", self.saveState())
        self._settings.setValue("splitterState", self._library_tab.save_splitter_state())

        # Save current tab by name
        self._settings.setValue("lastTabName", self._current_tab_name() or "All")

        # Save visible status tabs
        visible_names = [STATUS_TABS[i][0] for i in self._visible_status_tabs]
        self._settings.setValue("visibleTabs", visible_names)

        # Save custom tabs
        self._persist_custom_tabs()

        # Save sort order
        self._settings.setValue("sortField", self._library_tab.get_sort_field())
        self._settings.setValue("sortDesc", self._library_tab.is_sort_descending())

        # Save view type
        self._settings.setValue("viewType", "grid" if self._library_tab.is_grid_view() else "list")

        super().closeEvent(event)

"""Library tab: filter + sort + list/grid view + detail panel."""

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QSplitter,
    QLabel,
    QPushButton,
    QStackedWidget,
    QSlider,
    QMenu,
    QInputDialog,
)

from caco.player import format_duration
from caco.gui.models.wad_model import WadTableModel
from caco.gui.views.list_view import WadListView
from caco.gui.views.grid_view import WadGridView
from caco.gui.views.detail_panel import DetailPanel
from caco.gui.views.filter_bar import FilterBar
from caco.gui.views.sort_controls import SortControls
from caco.gui.thumbnails.loader import ThumbnailLoader


class LibraryTab(QWidget):
    """Composite widget: toolbar + list/grid + detail panel.

    Both list and grid views share the same WadTableModel.
    Switching views is just toggling visibility — no data reload.
    """

    play_requested = Signal(int)
    edit_requested = Signal(int)
    delete_requested = Signal(int)
    sessions_requested = Signal(int)
    status_message = Signal(str)
    card_size_changed = Signal(int)
    saved_searches_changed = Signal(list)  # list of (name, query) tuples
    save_as_tab_requested = Signal(str, str)  # (name, query)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._tab_query: str | None = None
        self._user_query: str = ""
        self._is_grid_view = False
        self._saved_searches: list[tuple[str, str]] = []  # (name, query)

        # Model (shared between list and grid)
        self._model = WadTableModel()

        # Thumbnail loader
        self._thumb_loader = ThumbnailLoader(self)
        self._thumb_loader.thumbnail_ready.connect(self._on_thumbnail_ready)

        # === Layout ===
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)

        # -- Toolbar row --
        toolbar = QHBoxLayout()
        toolbar.setContentsMargins(8, 6, 8, 6)
        toolbar.setSpacing(8)

        # View toggle button
        self._view_toggle = QPushButton("Grid")
        self._view_toggle.setToolTip("Switch between List and Grid view")
        self._view_toggle.setFixedWidth(60)
        self._view_toggle.clicked.connect(self._toggle_view)
        toolbar.addWidget(self._view_toggle)

        # Card size slider (grid view only)
        self._size_label = QLabel("Size:")
        toolbar.addWidget(self._size_label)
        self._size_slider = QSlider(Qt.Horizontal)
        self._size_slider.setRange(120, 360)
        self._size_slider.setValue(200)
        self._size_slider.setFixedWidth(100)
        self._size_slider.setToolTip("Card size")
        self._size_slider.valueChanged.connect(self._on_card_size_changed)
        toolbar.addWidget(self._size_slider)
        # Hide slider in list view (default)
        self._size_label.hide()
        self._size_slider.hide()

        # Sort controls
        self._sort = SortControls()
        self._sort.sort_changed.connect(self._on_sort_changed)
        toolbar.addWidget(self._sort)

        toolbar.addStretch()

        # Filter
        filter_label = QLabel("Filter:")
        toolbar.addWidget(filter_label)
        self._filter = FilterBar()
        self._filter.setMinimumWidth(250)
        self._filter.query_changed.connect(self._on_filter_changed)
        toolbar.addWidget(self._filter)

        # Saved searches button
        self._saved_btn = QPushButton("Searches")
        self._saved_btn.setObjectName("saved_searches_btn")
        self._saved_btn.setToolTip("Saved searches")
        self._saved_btn.setFixedWidth(90)
        self._saved_btn.clicked.connect(self._show_saved_searches_menu)
        toolbar.addWidget(self._saved_btn)

        main_layout.addLayout(toolbar)

        # -- Content: splitter with views + detail panel --
        self._splitter = QSplitter(Qt.Horizontal)

        # View stack (list and grid)
        self._view_stack = QStackedWidget()

        # List view
        self._list_view = WadListView()
        self._list_view.setModel(self._model)
        self._list_view.wad_selected.connect(self._on_wad_selected)
        self._list_view.wad_activated.connect(self._on_wad_activated)
        self._list_view.selection_cleared.connect(self._on_selection_cleared)
        self._list_view.play_requested.connect(self.play_requested)
        self._list_view.edit_requested.connect(self.edit_requested)
        self._list_view.delete_requested.connect(self.delete_requested)
        self._list_view.sessions_requested.connect(self.sessions_requested)
        self._view_stack.addWidget(self._list_view)

        # Grid view
        self._grid_view = WadGridView()
        self._grid_view.setModel(self._model)
        self._grid_view.wad_selected.connect(self._on_wad_selected)
        self._grid_view.wad_activated.connect(self._on_wad_activated)
        self._grid_view.selection_cleared.connect(self._on_selection_cleared)
        self._grid_view.play_requested.connect(self.play_requested)
        self._grid_view.edit_requested.connect(self.edit_requested)
        self._grid_view.delete_requested.connect(self.delete_requested)
        self._grid_view.sessions_requested.connect(self.sessions_requested)
        self._view_stack.addWidget(self._grid_view)

        self._splitter.addWidget(self._view_stack)

        # Detail panel
        self._detail = DetailPanel()
        self._detail.play_requested.connect(self.play_requested)
        self._detail.edit_requested.connect(self.edit_requested)
        self._detail.delete_requested.connect(self.delete_requested)
        self._splitter.addWidget(self._detail)

        # Splitter proportions (roughly 70/30)
        self._splitter.setStretchFactor(0, 7)
        self._splitter.setStretchFactor(1, 3)

        main_layout.addWidget(self._splitter)

        # Start in list view
        self._view_stack.setCurrentWidget(self._list_view)

        # Initial load
        self.refresh()

    # ── Public API ─────────────────────────────────────────────────

    def set_tab_query(self, query: str | None):
        """Set the status filter from the tab bar."""
        self._tab_query = query
        self.refresh()

    def refresh(self):
        """Reload data from the database with current filters."""
        combined = self._build_combined_query()

        count = self._model.load(
            query=combined,
            sort_by=self._sort.current_field(),
            sort_desc=self._sort.is_descending(),
        )

        total_playtime = self._model.total_playtime()
        pt_str = format_duration(total_playtime) if total_playtime else "0s"
        self.status_message.emit(f"{count} WADs | Total playtime: {pt_str}")

        # Select first row if available
        if count > 0:
            active_view = self._grid_view if self._is_grid_view else self._list_view
            active_view.setCurrentIndex(self._model.index(0, 0))

        # Pre-load thumbnails for grid view
        if self._is_grid_view:
            self._request_visible_thumbnails()

    def select_wad(self, wad_id: int):
        """Select a specific WAD in the active view."""
        if self._is_grid_view:
            # Grid view: find row and select
            model = self._model
            for row in range(model.rowCount()):
                idx = model.index(row, 0)
                if model.data(idx, Qt.UserRole) == wad_id:
                    self._grid_view.setCurrentIndex(idx)
                    self._grid_view.scrollTo(idx)
                    return
        else:
            self._list_view.select_wad(wad_id)

    def set_query(self, query: str):
        """Set the filter bar text and refresh."""
        self._filter.set_query(query)

    def get_selected_wad_id(self) -> int | None:
        """Return the currently selected WAD ID, or None."""
        return self._detail._wad_id

    def focus_filter(self):
        """Focus the filter input."""
        self._filter.setFocus()
        self._filter.selectAll()

    # ── Sort management ─────────────────────────────────────────────

    def set_sort(self, field: str, descending: bool) -> None:
        """Set the sort field and direction."""
        self._sort.set_sort(field, descending)

    def get_sort_field(self) -> str:
        """Return the current sort field name."""
        return self._sort.current_field()

    def is_sort_descending(self) -> bool:
        """Return whether the current sort is descending."""
        return self._sort.is_descending()

    # ── View management ──────────────────────────────────────────

    def toggle_view(self) -> None:
        """Switch between list and grid views."""
        self._toggle_view()

    def is_grid_view(self) -> bool:
        """Return whether the grid view is currently active."""
        return self._is_grid_view

    # ── Column management ────────────────────────────────────────

    def set_visible_columns(self, columns: list) -> None:
        """Set the visible columns for the list view."""
        self._model.set_columns(columns)
        self._list_view._apply_column_widths()

    @property
    def columns_changed(self):
        """Signal emitted when list view columns change."""
        return self._list_view.columns_changed

    # ── Splitter state (for geometry persistence) ────────────────

    def save_splitter_state(self):
        """Return the current splitter state for persistence."""
        return self._splitter.saveState()

    def restore_splitter_state(self, state) -> None:
        """Restore a previously saved splitter state."""
        self._splitter.restoreState(state)

    # ── View toggle ────────────────────────────────────────────────

    def set_card_size(self, width: int):
        """Set grid card size (called from MainWindow to restore saved value)."""
        self._size_slider.setValue(width)

    def card_size(self) -> int:
        """Return current grid card width."""
        return self._size_slider.value()

    def _toggle_view(self):
        """Switch between list and grid views."""
        self._is_grid_view = not self._is_grid_view
        if self._is_grid_view:
            self._view_stack.setCurrentWidget(self._grid_view)
            self._view_toggle.setText("List")
            self._size_label.show()
            self._size_slider.show()
            self._request_visible_thumbnails()
        else:
            self._view_stack.setCurrentWidget(self._list_view)
            self._view_toggle.setText("Grid")
            self._size_label.hide()
            self._size_slider.hide()

    def _on_card_size_changed(self, width: int):
        """Resize grid cards when slider moves."""
        self._grid_view.set_card_size(width)
        self.card_size_changed.emit(width)

    # ── Saved searches ─────────────────────────────────────────────

    def set_saved_searches(self, searches: list[tuple[str, str]]):
        """Set saved searches (called from MainWindow to restore)."""
        self._saved_searches = list(searches)

    def _show_saved_searches_menu(self):
        """Show menu with saved searches and save/delete options."""
        menu = QMenu(self)

        if self._saved_searches:
            for name, query in self._saved_searches:
                action = menu.addAction(name)
                action.setToolTip(query)
                action.triggered.connect(lambda checked, q=query: self._filter.set_query(q))
            menu.addSeparator()

            # Delete submenu
            delete_menu = menu.addMenu("Delete...")
            for name, query in self._saved_searches:
                del_action = delete_menu.addAction(name)
                del_action.triggered.connect(lambda checked, n=name: self._delete_saved_search(n))
            menu.addSeparator()

        # Save current filter
        current_query = self._filter.text().strip()
        save_action = menu.addAction("Save current filter...")
        save_action.setEnabled(bool(current_query))
        save_action.triggered.connect(self._save_current_filter)

        tab_action = menu.addAction("Save as tab...")
        tab_action.setEnabled(bool(current_query))
        tab_action.triggered.connect(self._save_as_tab)

        menu.exec(self._saved_btn.mapToGlobal(self._saved_btn.rect().bottomLeft()))

    def _save_current_filter(self):
        """Prompt for a name and save the current filter query."""
        query = self._filter.text().strip()
        if not query:
            return

        name, ok = QInputDialog.getText(self, "Save Search", "Name:", text=query)
        if ok and name.strip():
            name = name.strip()
            # Replace if name already exists
            self._saved_searches = [(n, q) for n, q in self._saved_searches if n != name]
            self._saved_searches.append((name, query))
            self.saved_searches_changed.emit(self._saved_searches)

    def _save_as_tab(self):
        """Prompt for a name and emit signal to create a custom tab."""
        query = self._filter.text().strip()
        if not query:
            return

        name, ok = QInputDialog.getText(self, "Save as Tab", "Tab name:", text=query)
        if ok and name.strip():
            self.save_as_tab_requested.emit(name.strip(), query)

    def _delete_saved_search(self, name: str):
        """Remove a saved search by name."""
        self._saved_searches = [(n, q) for n, q in self._saved_searches if n != name]
        self.saved_searches_changed.emit(self._saved_searches)

    def _request_visible_thumbnails(self):
        """Request thumbnails for all loaded WADs (for grid view)."""
        for row in range(self._model.rowCount()):
            wad = self._model.get_wad(row)
            if wad:
                self._thumb_loader.request(
                    wad["id"],
                    cached_path=wad.get("cached_path"),
                    source_type=wad.get("source_type"),
                    source_url=wad.get("source_url"),
                    title=wad.get("title", ""),
                )

    # ── Internal slots ────────────────────────────────────────────

    def _build_combined_query(self) -> str | None:
        parts = []
        if self._tab_query:
            parts.append(self._tab_query)
        if self._user_query:
            parts.append(self._user_query)
        return " ".join(parts) if parts else None

    def _on_filter_changed(self, text: str):
        self._user_query = text
        self.refresh()

    def _on_sort_changed(self, field: str, desc: bool):
        self.refresh()

    def _on_wad_selected(self, wad_id: int):
        stats = self._model.get_wad_stats(wad_id)
        wad = self._model.get_wad_by_id(wad_id)
        self._detail.update_wad(wad_id, stats=stats, wad=wad)

        # Request thumbnail for detail panel
        if wad:
            self._thumb_loader.request(
                wad_id,
                cached_path=wad.get("cached_path"),
                source_type=wad.get("source_type"),
                source_url=wad.get("source_url"),
                title=wad.get("title", ""),
            )

    def _on_wad_activated(self, wad_id: int):
        self.play_requested.emit(wad_id)

    def _on_thumbnail_ready(self, wad_id: int, pixmap):
        """Thumbnail arrived — update both detail panel and grid delegate."""
        if self._detail._wad_id == wad_id:
            self._detail.set_thumbnail(pixmap)
        # Always update the grid delegate's cache
        self._grid_view.set_thumbnail(wad_id, pixmap)

    def _on_selection_cleared(self):
        self._detail.clear()

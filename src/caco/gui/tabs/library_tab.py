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

    def __init__(self, parent=None):
        super().__init__(parent)
        self._tab_query: str | None = None
        self._user_query: str = ""
        self._is_grid_view = False

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

    def focus_filter(self):
        """Focus the filter input."""
        self._filter.setFocus()
        self._filter.selectAll()

    # ── View toggle ────────────────────────────────────────────────

    def _toggle_view(self):
        """Switch between list and grid views."""
        self._is_grid_view = not self._is_grid_view
        if self._is_grid_view:
            self._view_stack.setCurrentWidget(self._grid_view)
            self._view_toggle.setText("List")
            self._request_visible_thumbnails()
        else:
            self._view_stack.setCurrentWidget(self._list_view)
            self._view_toggle.setText("Grid")

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
        self._detail.update_wad(wad_id, stats=stats)

        # Request thumbnail for detail panel
        wad = self._model.get_wad_by_id(wad_id)
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

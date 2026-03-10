"""WAD list view (QTableView) with context menu and keyboard shortcuts."""

from typing import cast

from PySide6.QtCore import Qt, Signal, QModelIndex, QPersistentModelIndex
from PySide6.QtGui import QKeySequence, QShortcut, QAction, QResizeEvent
from PySide6.QtWidgets import QTableView, QHeaderView, QAbstractItemView, QMenu

from caco.gui.constants import DEFAULT_COLUMNS, ALL_COLUMNS, Column
from caco.gui.models.wad_model import WadTableModel
from caco.gui.views import build_wad_context_menu


class WadListView(QTableView):
    """Table view for browsing WADs with context menu support."""

    wad_selected = Signal(int)       # Emitted when selection changes (wad_id)
    wad_activated = Signal(int)      # Emitted on double-click/Enter (wad_id)
    selection_cleared = Signal()     # Emitted when nothing is selected
    columns_changed = Signal(list)   # Emitted when column visibility changes

    # Action signals for context menu
    play_requested = Signal(int)
    edit_metadata_requested = Signal(int)
    edit_notes_requested = Signal(int)
    edit_sourceport_requested = Signal(int)
    edit_companions_requested = Signal(int)
    delete_requested = Signal(int)
    sessions_requested = Signal(int)
    wad_stats_requested = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)

        # Table appearance
        self.setAlternatingRowColors(True)
        self.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.setShowGrid(False)
        self.setSortingEnabled(False)
        self.verticalHeader().setVisible(False)
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.customContextMenuRequested.connect(self._show_context_menu)

        # Column sizing — proportional widths, recalculated on resize
        header = self.horizontalHeader()
        header.setStretchLastSection(False)
        header.setSectionResizeMode(QHeaderView.ResizeMode.Interactive)
        header.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        header.customContextMenuRequested.connect(self._show_header_context_menu)

        # Keyboard shortcuts
        self._setup_shortcuts()

    def setModel(self, model):
        """Override to set column widths after model is attached."""
        super().setModel(model)
        if model:
            self._apply_column_widths()

    def _apply_column_widths(self):
        """Set column widths proportionally based on current view width."""
        model = self.model()
        if not model or model.columnCount() == 0:
            return

        # Get columns from model if available, else use defaults
        if hasattr(model, 'columns'):
            columns = model.columns
        else:
            columns = DEFAULT_COLUMNS[:model.columnCount()]

        # Account for vertical scrollbar width
        scrollbar_width = self.verticalScrollBar().width() if self.verticalScrollBar().isVisible() else 20
        available = self.viewport().width() or (self.width() - scrollbar_width)
        if available <= 0:
            return

        total_weight = sum(c.weight for c in columns)
        if total_weight == 0:
            return

        for i, col in enumerate(columns):
            if i < model.columnCount():
                width = max(col.min_width, int(available * col.weight / total_weight))
                self.setColumnWidth(i, width)

    def resizeEvent(self, event: QResizeEvent):
        """Recalculate proportional column widths on window resize."""
        super().resizeEvent(event)
        self._apply_column_widths()

    def _setup_shortcuts(self):
        """Vim-style and standard keyboard shortcuts."""
        # j/k for up/down
        QShortcut(QKeySequence("j"), self, self._move_down)
        QShortcut(QKeySequence("k"), self, self._move_up)
        # G for bottom, gg for top (simplified: just g for top)
        QShortcut(QKeySequence("Shift+G"), self, self._go_bottom)

    def _move_down(self):
        idx = self.currentIndex()
        if idx.isValid() and idx.row() < self.model().rowCount() - 1:
            new_idx = self.model().index(idx.row() + 1, 0)
            self.setCurrentIndex(new_idx)
        elif not idx.isValid() and self.model().rowCount() > 0:
            self.setCurrentIndex(self.model().index(0, 0))

    def _move_up(self):
        idx = self.currentIndex()
        if idx.isValid() and idx.row() > 0:
            new_idx = self.model().index(idx.row() - 1, 0)
            self.setCurrentIndex(new_idx)

    def _go_bottom(self):
        if self.model().rowCount() > 0:
            last = self.model().index(self.model().rowCount() - 1, 0)
            self.setCurrentIndex(last)

    def currentChanged(self, current: QModelIndex | QPersistentModelIndex, previous: QModelIndex | QPersistentModelIndex):
        """Override to emit wad_selected when cursor moves."""
        super().currentChanged(current, previous)
        wad_id = self._wad_id_at(current)
        if wad_id is not None:
            self.wad_selected.emit(wad_id)
        else:
            self.selection_cleared.emit()

    def keyPressEvent(self, event):
        """Handle Enter/Return for activation."""
        if event.key() in (Qt.Key_Return, Qt.Key_Enter):
            wad_id = self._wad_id_at(self.currentIndex())
            if wad_id is not None:
                self.wad_activated.emit(wad_id)
                return
        super().keyPressEvent(event)

    def mouseDoubleClickEvent(self, event):
        """Handle double-click for activation."""
        idx = self.indexAt(event.pos())
        wad_id = self._wad_id_at(idx)
        if wad_id is not None:
            self.wad_activated.emit(wad_id)
        else:
            super().mouseDoubleClickEvent(event)

    def _wad_id_at(self, index: QModelIndex | QPersistentModelIndex) -> int | None:
        """Get wad_id from a model index using UserRole."""
        if index.isValid():
            # Always read from column 0 to get UserRole data
            idx = self.model().index(index.row(), 0)
            result: int | None = self.model().data(idx, Qt.ItemDataRole.UserRole)
            return result
        return None

    def _show_context_menu(self, pos):
        """Show right-click context menu."""
        idx = self.indexAt(pos)
        wad_id = self._wad_id_at(idx)
        if wad_id is None:
            return
        menu = build_wad_context_menu(self, wad_id)
        menu.exec(self.viewport().mapToGlobal(pos))

    def select_wad(self, wad_id: int) -> bool:
        """Select a WAD by its ID. Returns True if found."""
        model = self.model()
        if not model:
            return False
        for row in range(model.rowCount()):
            idx = model.index(row, 0)
            if model.data(idx, Qt.ItemDataRole.UserRole) == wad_id:
                self.setCurrentIndex(idx)
                self.scrollTo(idx)
                return True
        return False

    def _show_header_context_menu(self, pos):
        """Show context menu on header for column visibility."""
        model = self.model()
        if not model or not hasattr(model, "columns"):
            return

        current_cols = model.columns
        menu = QMenu(self)

        for col in ALL_COLUMNS:
            action = QAction(col.header, self)
            action.setCheckable(True)
            action.setChecked(col in current_cols)
            # Prevent hiding the last visible column
            if col in current_cols and len(current_cols) <= 1:
                action.setEnabled(False)
            action.toggled.connect(lambda checked, c=col: self._toggle_column(c, checked))
            menu.addAction(action)

        menu.exec(self.horizontalHeader().mapToGlobal(pos))

    def _toggle_column(self, col: Column, visible: bool):
        """Add or remove a column from the model."""
        raw_model = self.model()
        if not raw_model or not hasattr(raw_model, "columns"):
            return

        model = cast(WadTableModel, raw_model)
        current = model.columns
        if visible and col not in current:
            # Insert in canonical order (matching ALL_COLUMNS order)
            new_cols = [c for c in ALL_COLUMNS if c in current or c == col]
            model.set_columns(new_cols)
            self._apply_column_widths()
            self.columns_changed.emit(new_cols)
        elif not visible and col in current and len(current) > 1:
            new_cols = [c for c in current if c != col]
            model.set_columns(new_cols)
            self._apply_column_widths()
            self.columns_changed.emit(new_cols)

"""Cache management dialog."""

from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QHBoxLayout,
    QLabel,
    QTableWidget,
    QTableWidgetItem,
    QHeaderView,
    QPushButton,
    QDialogButtonBox,
    QMessageBox,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE


def _format_size(size_bytes: int) -> str:
    """Format file size as human-readable string."""
    if size_bytes >= 1024 * 1024 * 1024:
        return f"{size_bytes / (1024 ** 3):.1f} GB"
    elif size_bytes >= 1024 * 1024:
        return f"{size_bytes / (1024 ** 2):.1f} MB"
    elif size_bytes >= 1024:
        return f"{size_bytes / 1024:.1f} KB"
    return f"{size_bytes} B"


class CacheDialog(QDialog):
    """Modal dialog for managing the WAD file cache.

    Shows cached files with their sizes, and allows clearing
    individual or all cached files.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Cache Management")
        self.setMinimumWidth(600)
        self.setMinimumHeight(400)

        self._layout = QVBoxLayout(self)
        self._layout.setSpacing(8)

        # Status line
        self._status = QLabel("")
        self._status.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        self._layout.addWidget(self._status)

        # Table
        self._table = QTableWidget(0, 4)
        self._table.setHorizontalHeaderLabels(["ID", "Title", "Path", "Size"])
        self._table.setAlternatingRowColors(True)
        self._table.setEditTriggers(QTableWidget.NoEditTriggers)
        self._table.setSelectionBehavior(QTableWidget.SelectRows)
        self._table.setSelectionMode(QTableWidget.SingleSelection)
        self._table.verticalHeader().setVisible(False)

        header = self._table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(2, QHeaderView.Stretch)
        header.setSectionResizeMode(3, QHeaderView.ResizeToContents)

        self._layout.addWidget(self._table)

        # Action buttons
        btn_row = QHBoxLayout()

        clear_btn = QPushButton("Clear Selected")
        clear_btn.clicked.connect(self._clear_selected)
        btn_row.addWidget(clear_btn)

        clear_all_btn = QPushButton("Clear All")
        clear_all_btn.setObjectName("delete_button")
        clear_all_btn.clicked.connect(self._clear_all)
        btn_row.addWidget(clear_all_btn)

        btn_row.addStretch()

        close_btn = QDialogButtonBox(QDialogButtonBox.Close)
        close_btn.rejected.connect(self.reject)
        btn_row.addWidget(close_btn)

        self._layout.addLayout(btn_row)

        # Load data
        self._load()

    def _load(self):
        """Populate table with cached WADs."""
        cached = db.get_cached_wads()
        self._table.setRowCount(len(cached))

        total_size = 0
        file_count = 0

        for row, wad in enumerate(cached):
            self._table.setItem(row, 0, QTableWidgetItem(str(wad["id"])))
            self._table.setItem(row, 1, QTableWidgetItem(wad["title"]))

            path = wad.get("cached_path") or ""
            self._table.setItem(row, 2, QTableWidgetItem(path))

            # Check actual file size
            if path:
                p = Path(path)
                if p.exists():
                    size = p.stat().st_size
                    total_size += size
                    file_count += 1
                    size_item = QTableWidgetItem(_format_size(size))
                    size_item.setTextAlignment(Qt.AlignRight | Qt.AlignVCenter)
                else:
                    size_item = QTableWidgetItem("missing")
                    size_item.setForeground(Qt.red)
            else:
                size_item = QTableWidgetItem("-")

            self._table.setItem(row, 3, size_item)

        self._status.setText(
            f"{file_count} cached file(s), {_format_size(total_size)} total"
        )

    def _clear_selected(self):
        """Clear cache for the selected WAD."""
        row = self._table.currentRow()
        if row < 0:
            return

        wad_id_item = self._table.item(row, 0)
        if not wad_id_item:
            return

        wad_id = int(wad_id_item.text())
        title = self._table.item(row, 1).text()
        path = self._table.item(row, 2).text()

        # Delete the actual file
        if path:
            p = Path(path)
            if p.exists():
                p.unlink()

        db.clear_cached_path(wad_id)
        self._status.setText(f"Cleared cache for '{title}'")
        self._load()

    def _clear_all(self):
        """Clear all cached files."""
        reply = QMessageBox.question(
            self,
            "Clear All Cache",
            "Delete all cached WAD files? They can be re-downloaded.",
            QMessageBox.Yes | QMessageBox.No,
        )
        if reply != QMessageBox.Yes:
            return

        # Delete actual files
        cached = db.get_cached_wads()
        deleted = 0
        for wad in cached:
            path = wad.get("cached_path")
            if path:
                p = Path(path)
                if p.exists():
                    p.unlink()
                    deleted += 1

        count = db.clear_all_cached_paths()
        self._status.setText(f"Cleared {deleted} file(s), {count} path(s) reset")
        self._load()

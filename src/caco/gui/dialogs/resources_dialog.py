"""IWAD and id24 WAD resource management dialog."""

from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QAbstractItemView,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QVBoxLayout,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE


class ResourcesDialog(QDialog):
    """Modal dialog for managing IWAD and id24 WAD registries."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Resources")
        self.setMinimumWidth(700)
        self.setMinimumHeight(500)

        layout = QVBoxLayout(self)
        layout.setSpacing(8)

        # -- Tab widget --
        self._tabs = QTabWidget()
        layout.addWidget(self._tabs)

        # IWAD tab
        iwad_widget = self._build_iwad_tab()
        self._tabs.addTab(iwad_widget, "IWADs")

        # id24 tab
        id24_widget = self._build_id24_tab()
        self._tabs.addTab(id24_widget, "id24 WADs")

        # -- Import section --
        import_layout = QHBoxLayout()

        self._path_input = QLineEdit()
        self._path_input.setPlaceholderText("Path to IWAD or id24 WAD file...")
        self._path_input.returnPressed.connect(self._on_import)
        import_layout.addWidget(self._path_input)

        browse_btn = QPushButton("Browse...")
        browse_btn.clicked.connect(self._on_browse)
        import_layout.addWidget(browse_btn)

        import_btn = QPushButton("Import")
        import_btn.clicked.connect(self._on_import)
        import_layout.addWidget(import_btn)

        layout.addLayout(import_layout)

        # -- Status + Close --
        self._status = QLabel("")
        self._status.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(self._status)

        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

        # Load data
        self._load_iwads()
        self._load_id24()

    def _build_iwad_tab(self):
        """Build the IWAD tab contents."""
        widget = self._make_tab_widget()
        tab_layout = widget.layout()

        self._iwad_table = QTableWidget(0, 5)
        self._iwad_table.setHorizontalHeaderLabels(
            ["Family", "Variant", "Title", "Path", "Preferred"]
        )
        self._setup_table(self._iwad_table)

        header = self._iwad_table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)
        header.setSectionResizeMode(4, QHeaderView.ResizeMode.ResizeToContents)

        tab_layout.addWidget(self._iwad_table)

        btn_row = QHBoxLayout()
        remove_btn = QPushButton("Remove Selected")
        remove_btn.setObjectName("delete_button")
        remove_btn.clicked.connect(self._remove_iwad)
        btn_row.addWidget(remove_btn)
        btn_row.addStretch()
        tab_layout.addLayout(btn_row)

        return widget

    def _build_id24_tab(self):
        """Build the id24 tab contents."""
        widget = self._make_tab_widget()
        tab_layout = widget.layout()

        self._id24_table = QTableWidget(0, 4)
        self._id24_table.setHorizontalHeaderLabels(
            ["Name", "Version", "Title", "Path"]
        )
        self._setup_table(self._id24_table)

        header = self._id24_table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)

        tab_layout.addWidget(self._id24_table)

        btn_row = QHBoxLayout()
        remove_btn = QPushButton("Remove Selected")
        remove_btn.setObjectName("delete_button")
        remove_btn.clicked.connect(self._remove_id24)
        btn_row.addWidget(remove_btn)
        btn_row.addStretch()
        tab_layout.addLayout(btn_row)

        return widget

    @staticmethod
    def _make_tab_widget():
        from PySide6.QtWidgets import QWidget
        w = QWidget()
        w.setLayout(QVBoxLayout())
        w.layout().setContentsMargins(0, 4, 0, 0)
        return w

    @staticmethod
    def _setup_table(table: QTableWidget):
        table.setAlternatingRowColors(True)
        table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        table.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        table.verticalHeader().setVisible(False)

    def _load_iwads(self):
        """Populate IWAD table."""
        iwads = db.get_all_iwads()
        self._iwad_table.setRowCount(len(iwads))

        # Determine preferred variant per family
        preferred: dict[str, str | None] = {}
        families = {row["family"] for row in iwads}
        for family in families:
            pref = db.get_iwad(family)
            if pref:
                preferred[family] = pref.get("variant")

        for row_idx, row in enumerate(iwads):
            self._iwad_table.setItem(row_idx, 0, QTableWidgetItem(row["family"]))
            self._iwad_table.setItem(row_idx, 1, QTableWidgetItem(row["variant"]))
            self._iwad_table.setItem(row_idx, 2, QTableWidgetItem(row.get("title") or ""))
            self._iwad_table.setItem(row_idx, 3, QTableWidgetItem(row.get("path") or ""))

            is_preferred = preferred.get(row["family"]) == row["variant"]
            pref_item = QTableWidgetItem("\u2713" if is_preferred else "")
            pref_item.setTextAlignment(Qt.AlignmentFlag.AlignCenter)
            self._iwad_table.setItem(row_idx, 4, pref_item)

        self._update_status(iwads=iwads)

    def _load_id24(self):
        """Populate id24 table."""
        id24s = db.get_all_id24()
        self._id24_table.setRowCount(len(id24s))

        for row_idx, row in enumerate(id24s):
            self._id24_table.setItem(row_idx, 0, QTableWidgetItem(row.get("name") or ""))
            self._id24_table.setItem(row_idx, 1, QTableWidgetItem(row.get("version") or ""))
            self._id24_table.setItem(row_idx, 2, QTableWidgetItem(row.get("title") or ""))
            self._id24_table.setItem(row_idx, 3, QTableWidgetItem(row.get("path") or ""))

        self._update_status(id24s=id24s)

    def _update_status(self, iwads=None, id24s=None):
        """Update status label."""
        if iwads is None:
            iwads = db.get_all_iwads()
        if id24s is None:
            id24s = db.get_all_id24()
        self._status.setText(f"{len(iwads)} IWAD(s), {len(id24s)} id24 WAD(s)")

    def _remove_iwad(self):
        """Remove the selected IWAD."""
        row = self._iwad_table.currentRow()
        if row < 0:
            return

        family = self._iwad_table.item(row, 0).text()
        variant = self._iwad_table.item(row, 1).text()

        removed_paths = db.remove_iwad_with_paths(family, variant)
        for p in removed_paths:
            path = Path(p)
            if path.exists():
                path.unlink()

        self._status.setText(f"Removed IWAD: {family}/{variant}")
        self._load_iwads()

    def _remove_id24(self):
        """Remove the selected id24 WAD."""
        row = self._id24_table.currentRow()
        if row < 0:
            return

        name = self._id24_table.item(row, 0).text()

        removed_paths = db.remove_id24_with_paths(name)
        for p in removed_paths:
            path = Path(p)
            if path.exists():
                path.unlink()

        self._status.setText(f"Removed id24: {name}")
        self._load_id24()

    def _on_browse(self):
        """Open file dialog to select a WAD file."""
        path, _ = QFileDialog.getOpenFileName(
            self,
            "Select IWAD or id24 WAD",
            "",
            "WAD files (*.wad);;All files (*)",
        )
        if path:
            self._path_input.setText(path)

    def _on_import(self):
        """Import file from path input."""
        path_str = self._path_input.text().strip()
        if not path_str:
            return

        path = Path(path_str).expanduser().resolve()
        if not path.exists():
            QMessageBox.warning(self, "File Not Found", f"File not found: {path_str}")
            return

        from caco.services.resource_service import register_iwad, register_id24

        result = register_iwad(path)
        if result:
            family, variant, title = result
            self._status.setText(f"Registered IWAD: {title} ({family}/{variant})")
            self._load_iwads()
            self._tabs.setCurrentIndex(0)  # Switch to IWAD tab
            self._path_input.clear()
            return

        result = register_id24(path)
        if result:
            name, version, title = result
            self._status.setText(f"Registered id24: {title} ({version})")
            self._load_id24()
            self._tabs.setCurrentIndex(1)  # Switch to id24 tab
            self._path_input.clear()
            return

        QMessageBox.warning(
            self,
            "Unrecognized File",
            "The file is not a recognized IWAD or id24 WAD.",
        )

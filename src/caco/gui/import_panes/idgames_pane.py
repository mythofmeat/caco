"""idgames archive search and import pane."""

from PySide6.QtCore import Qt, Signal, QThreadPool
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QLineEdit,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QHeaderView,
    QLabel,
    QMessageBox,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE
from caco.gui.workers.search_worker import IdgamesSearchWorker
from caco.gui.workers.import_worker import IdgamesImportWorker
from caco.utils import format_rating


class IdgamesPane(QWidget):
    """Search idgames archive and import results."""

    wad_imported = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._results = []
        self._pool = QThreadPool.globalInstance()

        layout = QVBoxLayout(self)
        layout.setSpacing(8)

        # Search row
        search_row = QHBoxLayout()
        self._search_input = QLineEdit()
        self._search_input.setPlaceholderText("Search idgames archive...")
        self._search_input.returnPressed.connect(self._do_search)
        search_row.addWidget(self._search_input)

        self._search_btn = QPushButton("Search")
        self._search_btn.clicked.connect(self._do_search)
        search_row.addWidget(self._search_btn)
        layout.addLayout(search_row)

        # Status
        self._status = QLabel("")
        self._status.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(self._status)

        # Results table
        self._table = QTableWidget(0, 5)
        self._table.setHorizontalHeaderLabels(["ID", "Title", "Author", "Rating", "Date"])
        self._table.setAlternatingRowColors(True)
        self._table.setEditTriggers(QTableWidget.NoEditTriggers)
        self._table.setSelectionBehavior(QTableWidget.SelectRows)
        self._table.setSelectionMode(QTableWidget.SingleSelection)
        self._table.verticalHeader().setVisible(False)

        header = self._table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.Stretch)
        header.setSectionResizeMode(2, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(4, QHeaderView.ResizeToContents)

        self._table.currentCellChanged.connect(self._on_selection_changed)
        self._table.cellDoubleClicked.connect(self._on_double_click)
        layout.addWidget(self._table)

        # Preview area
        self._preview = QLabel("Select a result to see details")
        self._preview.setWordWrap(True)
        self._preview.setStyleSheet(
            f"background-color: {DOOM_PALETTE['bg_medium']}; "
            f"padding: 8px; border: 1px solid {DOOM_PALETTE['border']}; border-radius: 4px;"
        )
        self._preview.setMinimumHeight(80)
        self._preview.setAlignment(Qt.AlignTop | Qt.AlignLeft)
        layout.addWidget(self._preview)

        # Import button
        btn_row = QHBoxLayout()
        btn_row.addStretch()
        self._import_btn = QPushButton("Import Selected")
        self._import_btn.setEnabled(False)
        self._import_btn.clicked.connect(self._do_import)
        btn_row.addWidget(self._import_btn)
        layout.addLayout(btn_row)

    def _do_search(self):
        query = self._search_input.text().strip()
        if not query:
            return

        self._status.setText("Searching...")
        self._search_btn.setEnabled(False)
        self._table.setRowCount(0)
        self._results = []
        self._import_btn.setEnabled(False)

        worker = IdgamesSearchWorker(query)
        worker.signals.finished.connect(self._on_search_done)
        worker.signals.error.connect(self._on_search_error)
        self._pool.start(worker)

    def _on_search_done(self, results):
        self._search_btn.setEnabled(True)
        self._results = results
        self._status.setText(f"{len(results)} result(s)")

        self._table.setRowCount(len(results))
        for row, entry in enumerate(results):
            self._table.setItem(row, 0, QTableWidgetItem(str(entry.id)))
            self._table.setItem(row, 1, QTableWidgetItem(entry.title or ""))
            self._table.setItem(row, 2, QTableWidgetItem(entry.author or ""))

            rating_str = format_rating(round(entry.rating)) if entry.rating is not None else ""
            self._table.setItem(row, 3, QTableWidgetItem(rating_str))

            date_str = entry.date[:10] if entry.date else ""
            self._table.setItem(row, 4, QTableWidgetItem(date_str))

        if results:
            self._table.selectRow(0)

    def _on_search_error(self, error_msg):
        self._search_btn.setEnabled(True)
        self._status.setText(f"Error: {error_msg}")

    def _on_selection_changed(self, row, col, prev_row, prev_col):
        if 0 <= row < len(self._results):
            entry = self._results[row]
            self._import_btn.setEnabled(True)

            parts = [f"<b>{entry.title}</b>"]
            if entry.author:
                parts.append(f"by {entry.author}")
            if entry.date:
                parts.append(f"({entry.date[:4]})")
            if entry.rating is not None:
                parts.append(f"<br>{format_rating(round(entry.rating))}")
            if entry.description:
                from caco.utils import truncate
                parts.append(f"<br><br>{truncate(entry.description, 300)}")

            self._preview.setText(" ".join(parts))
        else:
            self._import_btn.setEnabled(False)
            self._preview.setText("Select a result to see details")

    def _on_double_click(self, row, col):
        self._do_import()

    def _do_import(self):
        row = self._table.currentRow()
        if row < 0 or row >= len(self._results):
            return

        entry = self._results[row]

        # Duplicate check
        existing = db.find_duplicate(
            db.SourceType.IDGAMES,
            source_id=str(entry.id),
            filename=entry.filename,
            author=entry.author,
        )
        if existing:
            reply = QMessageBox.question(
                self,
                "Duplicate Found",
                f"'{existing['title']}' (ID: {existing['id']}) already in library.\n\nImport anyway?",
                QMessageBox.Yes | QMessageBox.No,
            )
            if reply != QMessageBox.Yes:
                return

        self._import_btn.setEnabled(False)
        self._status.setText(f"Importing {entry.title}...")

        worker = IdgamesImportWorker(entry)
        worker.signals.finished.connect(self._on_import_done)
        worker.signals.error.connect(self._on_import_error)
        self._pool.start(worker)

    def _on_import_done(self, wad_id):
        self._import_btn.setEnabled(True)
        self._status.setText(f"Imported! (ID: {wad_id})")
        self.wad_imported.emit(wad_id)

    def _on_import_error(self, error_msg):
        self._import_btn.setEnabled(True)
        self._status.setText(f"Import error: {error_msg}")

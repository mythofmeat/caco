"""Doom Wiki search and import pane."""

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
    QFileDialog,
)

from caco.gui.theme import DOOM_PALETTE
from caco.gui.workers.search_worker import DoomwikiSearchWorker
from caco.services import ImportService


class DoomwikiPane(QWidget):
    """Search Doom Wiki and import results."""

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
        self._search_input.setPlaceholderText("Search Doom Wiki...")
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
        self._table.setHorizontalHeaderLabels(["Title", "Author", "Year", "IWAD", "Port"])
        self._table.setAlternatingRowColors(True)
        self._table.setEditTriggers(QTableWidget.NoEditTriggers)
        self._table.setSelectionBehavior(QTableWidget.SelectRows)
        self._table.setSelectionMode(QTableWidget.SingleSelection)
        self._table.verticalHeader().setVisible(False)

        header = self._table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.Stretch)
        header.setSectionResizeMode(1, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(2, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(4, QHeaderView.ResizeToContents)

        self._table.currentCellChanged.connect(self._on_selection_changed)
        self._table.cellDoubleClicked.connect(self._on_double_click)
        layout.addWidget(self._table)

        # Preview
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

        worker = DoomwikiSearchWorker(query)
        worker.signals.finished.connect(self._on_search_done)
        worker.signals.error.connect(self._on_search_error)
        self._pool.start(worker)

    def _on_search_done(self, results):
        self._search_btn.setEnabled(True)
        self._results = results
        self._status.setText(f"{len(results)} result(s)")

        self._table.setRowCount(len(results))
        for row, entry in enumerate(results):
            self._table.setItem(row, 0, QTableWidgetItem(entry.display_name or entry.title))
            self._table.setItem(row, 1, QTableWidgetItem(entry.author or ""))
            self._table.setItem(row, 2, QTableWidgetItem(str(entry.year) if entry.year else ""))
            self._table.setItem(row, 3, QTableWidgetItem(entry.iwad or ""))
            self._table.setItem(row, 4, QTableWidgetItem(entry.source_port or ""))

        if results:
            self._table.selectRow(0)

    def _on_search_error(self, error_msg):
        self._search_btn.setEnabled(True)

        # Offer JSON fallback when API is blocked by WAF
        if "Cloudflare challenge" in error_msg or "WAF challenge" in error_msg:
            from caco.services.json_import import doomwiki_api_url
            query = self._search_input.text().strip()
            url = doomwiki_api_url(query)
            reply = QMessageBox.question(
                self,
                "API Blocked",
                f"Doom Wiki API is blocked by WAF.\n\n"
                f"You can open this URL in your browser, save the JSON response, "
                f"and load it here:\n\n{url}",
                QMessageBox.Open | QMessageBox.Cancel,
            )
            if reply == QMessageBox.Open:
                self._load_from_json()
                return

        self._status.setText(f"Error: {error_msg}")

    def _load_from_json(self):
        """Load search results from a saved Doom Wiki API JSON file."""
        path, _ = QFileDialog.getOpenFileName(
            self, "Load Doom Wiki JSON", "", "JSON files (*.json)"
        )
        if not path:
            return

        try:
            from caco.services.json_import import parse_doomwiki_json
            entries = parse_doomwiki_json(path)
        except Exception as e:
            self._status.setText(f"Error reading JSON: {e}")
            return

        if not entries:
            self._status.setText("No WAD pages found in JSON")
            return

        self._on_search_done(entries)

    def _on_selection_changed(self, row, col, prev_row, prev_col):
        if 0 <= row < len(self._results):
            entry = self._results[row]
            self._import_btn.setEnabled(True)

            parts = [f"<b>{entry.display_name or entry.title}</b>"]
            author_year = []
            if entry.author:
                author_year.append(f"by {entry.author}")
            if entry.year:
                author_year.append(f"({entry.year})")
            if author_year:
                parts.append(" ".join(author_year))

            tech = []
            if entry.iwad:
                tech.append(entry.iwad)
            if entry.source_port:
                tech.append(entry.source_port)
            if tech:
                parts.append(f"<br><i>{' | '.join(tech)}</i>")

            if entry.description:
                desc = entry.description[:300]
                if len(entry.description) > 300:
                    desc += "..."
                parts.append(f"<br><br>{desc}")

            self._preview.setText("<br>".join(parts[:3]) + ("".join(parts[3:]) if len(parts) > 3 else ""))
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
        svc = ImportService()

        result = svc.import_doomwiki(entry)
        if result.is_duplicate:
            reply = QMessageBox.question(
                self,
                "Duplicate Found",
                f"'{result.duplicate_title}' (ID: {result.duplicate_id}) already in library.\n\nImport anyway?",
                QMessageBox.Yes | QMessageBox.No,
            )
            if reply != QMessageBox.Yes:
                return
            result = svc.import_doomwiki(entry, force=True)

        if result.error:
            self._status.setText(f"Import error: {result.error}")
        elif result.ok:
            self._status.setText(f"Imported! (ID: {result.wad_id})")
            self.wad_imported.emit(result.wad_id)

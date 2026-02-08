"""Doomworld forum thread URL import pane."""

from PySide6.QtCore import Qt, Signal, QThreadPool
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QFormLayout,
    QLineEdit,
    QPushButton,
    QLabel,
    QMessageBox,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE
from caco.gui.workers.search_worker import DoomworldFetchWorker
from caco.gui.workers.import_worker import DoomworldImportWorker


class DoomworldPane(QWidget):
    """Fetch a Doomworld forum thread and import with editable fields."""

    wad_imported = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._thread = None  # Fetched ForumThread object
        self._pool = QThreadPool.globalInstance()

        layout = QVBoxLayout(self)
        layout.setSpacing(8)

        # URL input row
        url_row = QHBoxLayout()
        self._url_input = QLineEdit()
        self._url_input.setPlaceholderText("Paste Doomworld forum thread URL...")
        self._url_input.returnPressed.connect(self._do_fetch)
        url_row.addWidget(self._url_input)

        self._fetch_btn = QPushButton("Fetch")
        self._fetch_btn.clicked.connect(self._do_fetch)
        url_row.addWidget(self._fetch_btn)
        layout.addLayout(url_row)

        # Status
        self._status = QLabel("")
        self._status.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(self._status)

        # Preview
        self._preview = QLabel("Enter a Doomworld forum URL to fetch thread info")
        self._preview.setWordWrap(True)
        self._preview.setStyleSheet(
            f"background-color: {DOOM_PALETTE['bg_medium']}; "
            f"padding: 8px; border: 1px solid {DOOM_PALETTE['border']}; border-radius: 4px;"
        )
        self._preview.setMinimumHeight(60)
        self._preview.setAlignment(Qt.AlignTop | Qt.AlignLeft)
        layout.addWidget(self._preview)

        # Editable form (hidden until fetch completes)
        self._form_widget = QWidget()
        form = QFormLayout(self._form_widget)

        self._title_input = QLineEdit()
        form.addRow("Title:", self._title_input)

        self._author_input = QLineEdit()
        form.addRow("Author:", self._author_input)

        self._year_input = QLineEdit()
        self._year_input.setMaxLength(4)
        self._year_input.setMaximumWidth(80)
        form.addRow("Year:", self._year_input)

        self._tags_input = QLineEdit()
        self._tags_input.setPlaceholderText("Comma-separated tags")
        form.addRow("Tags:", self._tags_input)

        self._form_widget.setVisible(False)
        layout.addWidget(self._form_widget)

        # Import button
        btn_row = QHBoxLayout()
        btn_row.addStretch()
        self._import_btn = QPushButton("Import")
        self._import_btn.setEnabled(False)
        self._import_btn.clicked.connect(self._do_import)
        btn_row.addWidget(self._import_btn)
        layout.addLayout(btn_row)

        layout.addStretch()

    def _do_fetch(self):
        url = self._url_input.text().strip()
        if not url:
            return

        self._status.setText("Fetching thread...")
        self._fetch_btn.setEnabled(False)
        self._form_widget.setVisible(False)
        self._import_btn.setEnabled(False)
        self._thread = None

        worker = DoomworldFetchWorker(url)
        worker.signals.finished.connect(self._on_fetch_done)
        worker.signals.error.connect(self._on_fetch_error)
        self._pool.start(worker)

    def _on_fetch_done(self, results):
        self._fetch_btn.setEnabled(True)

        if not results:
            self._status.setText("Could not fetch thread. Check the URL.")
            return

        thread = results[0]
        self._thread = thread
        self._status.setText("Thread fetched!")

        # Fill preview
        parts = [f"<b>{thread.title}</b>"]
        meta = []
        if thread.author:
            meta.append(f"by {thread.author}")
        if thread.posted_date:
            meta.append(thread.posted_date)
        if meta:
            parts.append(f"<i>{', '.join(meta)}</i>")
        if thread.first_post_text:
            excerpt = thread.first_post_text[:200]
            if len(thread.first_post_text) > 200:
                excerpt += "..."
            parts.append(f"<br>{excerpt}")
        self._preview.setText("<br>".join(parts))

        # Fill editable form
        self._title_input.setText(thread.title or "")
        self._author_input.setText(thread.author or "")
        if thread.posted_date:
            # Try to extract year from date string
            from caco.utils import extract_year
            year = extract_year(thread.posted_date)
            self._year_input.setText(str(year) if year else "")
        else:
            self._year_input.setText("")

        self._form_widget.setVisible(True)
        self._import_btn.setEnabled(True)

    def _on_fetch_error(self, error_msg):
        self._fetch_btn.setEnabled(True)
        self._status.setText(f"Error: {error_msg}")

    def _do_import(self):
        if not self._thread:
            return

        title = self._title_input.text().strip() or None
        author = self._author_input.text().strip() or None
        year = None
        year_text = self._year_input.text().strip()
        if year_text:
            try:
                year = int(year_text)
            except ValueError:
                pass

        tags = [t.strip().lower() for t in self._tags_input.text().split(",") if t.strip()]

        # Duplicate check
        existing = db.find_duplicate(
            db.SourceType.DOOMWORLD,
            source_id=self._thread.thread_id,
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
        self._status.setText("Importing...")

        worker = DoomworldImportWorker(
            self._thread,
            tags=tags or None,
            title=title,
            author=author,
            year=year,
        )
        worker.signals.finished.connect(self._on_import_done)
        worker.signals.error.connect(self._on_import_error)
        self._pool.start(worker)

    def _on_import_done(self, wad_id):
        self._import_btn.setEnabled(True)
        self._status.setText(f"Imported! (ID: {wad_id})")
        self._form_widget.setVisible(False)
        self._thread = None
        self.wad_imported.emit(wad_id)

    def _on_import_error(self, error_msg):
        self._import_btn.setEnabled(True)
        self._status.setText(f"Import error: {error_msg}")

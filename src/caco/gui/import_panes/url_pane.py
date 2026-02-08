"""Manual URL import form pane."""

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QFormLayout,
    QLineEdit,
    QTextEdit,
    QPushButton,
    QLabel,
    QHBoxLayout,
    QMessageBox,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE


class UrlPane(QWidget):
    """Manual form for importing a WAD by URL.

    No source adapter — directly calls db.add_wad() with SourceType.URL.
    """

    wad_imported = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)

        layout = QVBoxLayout(self)
        layout.setSpacing(8)

        hint = QLabel("Add a WAD by download URL. Metadata is entered manually.")
        hint.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(hint)

        # Form
        form = QFormLayout()

        self._title_input = QLineEdit()
        self._title_input.setPlaceholderText("Required")
        form.addRow("Title:", self._title_input)

        self._url_input = QLineEdit()
        self._url_input.setPlaceholderText("https://...")
        form.addRow("URL:", self._url_input)

        self._author_input = QLineEdit()
        form.addRow("Author:", self._author_input)

        self._year_input = QLineEdit()
        self._year_input.setMaxLength(4)
        self._year_input.setMaximumWidth(80)
        form.addRow("Year:", self._year_input)

        self._tags_input = QLineEdit()
        self._tags_input.setPlaceholderText("Comma-separated tags")
        form.addRow("Tags:", self._tags_input)

        self._notes_input = QTextEdit()
        self._notes_input.setMaximumHeight(80)
        form.addRow("Notes:", self._notes_input)

        layout.addLayout(form)

        # Status
        self._status = QLabel("")
        self._status.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(self._status)

        # Import button
        btn_row = QHBoxLayout()
        btn_row.addStretch()
        self._import_btn = QPushButton("Import")
        self._import_btn.clicked.connect(self._do_import)
        btn_row.addWidget(self._import_btn)
        layout.addLayout(btn_row)

        layout.addStretch()

    def _do_import(self):
        title = self._title_input.text().strip()
        url = self._url_input.text().strip()

        if not title:
            QMessageBox.warning(self, "Validation Error", "Title is required.")
            return
        if not url:
            QMessageBox.warning(self, "Validation Error", "URL is required.")
            return

        # Parse year
        year = None
        year_text = self._year_input.text().strip()
        if year_text:
            try:
                year = int(year_text)
            except ValueError:
                QMessageBox.warning(self, "Validation Error", "Year must be a number.")
                return

        tags = [t.strip().lower() for t in self._tags_input.text().split(",") if t.strip()]
        notes = self._notes_input.toPlainText().strip() or None

        # Duplicate check
        existing = db.find_duplicate(
            db.SourceType.URL,
            source_url=url,
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

        author = self._author_input.text().strip() or None

        wad_id = db.add_wad(
            title=title,
            source_type=db.SourceType.URL,
            author=author,
            year=year,
            source_url=url,
            tags=tags or None,
        )

        # Set notes separately if provided
        if notes:
            db.update_wad(wad_id, notes=notes)

        self._status.setText(f"Imported! (ID: {wad_id})")
        self.wad_imported.emit(wad_id)

        # Clear form
        self._title_input.clear()
        self._url_input.clear()
        self._author_input.clear()
        self._year_input.clear()
        self._tags_input.clear()
        self._notes_input.clear()

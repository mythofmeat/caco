"""Local file import form pane."""

from pathlib import Path

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QFormLayout,
    QLineEdit,
    QTextEdit,
    QPushButton,
    QLabel,
    QFileDialog,
    QMessageBox,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE


class LocalPane(QWidget):
    """Import a WAD from a local file path.

    Auto-fills title from filename. The file path is stored as source_url
    for deduplication, and cached_path if the file exists.
    """

    wad_imported = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)

        layout = QVBoxLayout(self)
        layout.setSpacing(8)

        hint = QLabel("Add a WAD from a local file. Title is auto-filled from filename.")
        hint.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(hint)

        # File path row
        path_row = QHBoxLayout()
        self._path_input = QLineEdit()
        self._path_input.setPlaceholderText("/path/to/wad.wad")
        self._path_input.textChanged.connect(self._on_path_changed)
        path_row.addWidget(self._path_input)

        browse_btn = QPushButton("Browse...")
        browse_btn.clicked.connect(self._browse)
        path_row.addWidget(browse_btn)
        layout.addLayout(path_row)

        # File info
        self._file_info = QLabel("")
        self._file_info.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']}; font-size: 11px;")
        layout.addWidget(self._file_info)

        # Form
        form = QFormLayout()

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

    def _browse(self):
        path, _ = QFileDialog.getOpenFileName(
            self,
            "Select WAD File",
            str(Path.home()),
            "WAD Files (*.wad *.pk3 *.pk7 *.zip);;All Files (*)",
        )
        if path:
            self._path_input.setText(path)

    def _on_path_changed(self, text: str):
        """Auto-fill title from filename and show file info."""
        path = Path(text.strip()).expanduser()

        if path.name:
            # Auto-fill title: filename stem with underscores/dashes replaced
            stem = path.stem
            title = stem.replace("_", " ").replace("-", " ").title()
            self._title_input.setText(title)

        if path.exists() and path.is_file():
            size = path.stat().st_size
            if size > 1024 * 1024:
                size_str = f"{size / (1024 * 1024):.1f} MB"
            elif size > 1024:
                size_str = f"{size / 1024:.1f} KB"
            else:
                size_str = f"{size} bytes"
            self._file_info.setText(f"File found: {size_str}")
        elif text.strip():
            self._file_info.setText("File not found (will record as reference)")
        else:
            self._file_info.setText("")

    def _do_import(self):
        path_text = self._path_input.text().strip()
        title = self._title_input.text().strip()

        if not path_text:
            QMessageBox.warning(self, "Validation Error", "File path is required.")
            return
        if not title:
            QMessageBox.warning(self, "Validation Error", "Title is required.")
            return

        path = Path(path_text).expanduser().resolve()

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
        author = self._author_input.text().strip() or None

        # Duplicate check
        existing = db.find_duplicate(
            db.SourceType.LOCAL,
            source_url=str(path),
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

        # Set cached_path only if file actually exists
        cached_path = str(path) if path.exists() else None

        wad_id = db.add_wad(
            title=title,
            source_type=db.SourceType.LOCAL,
            author=author,
            year=year,
            source_url=str(path),
            filename=path.name,
            cached_path=cached_path,
            tags=tags or None,
        )

        if notes:
            db.update_wad(wad_id, notes=notes)

        self._status.setText(f"Imported! (ID: {wad_id})")
        self.wad_imported.emit(wad_id)

        # Clear form
        self._path_input.clear()
        self._title_input.clear()
        self._author_input.clear()
        self._year_input.clear()
        self._tags_input.clear()
        self._notes_input.clear()
        self._file_info.clear()

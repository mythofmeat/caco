"""WAD metadata editing form dialog."""

import json

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QHBoxLayout,
    QFormLayout,
    QGroupBox,
    QLineEdit,
    QComboBox,
    QTextEdit,
    QDialogButtonBox,
    QMessageBox,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE


# Status options: (display, value)
_STATUS_OPTIONS = [
    ("To Play", "to-play"),
    ("Backlog", "backlog"),
    ("Playing", "playing"),
    ("Finished", "finished"),
    ("Abandoned", "abandoned"),
    ("Awaiting Update", "awaiting-update"),
]

# Rating options: (display, value)
_RATING_OPTIONS = [
    ("Not Rated", None),
    ("\u2605", 1),
    ("\u2605\u2605", 2),
    ("\u2605\u2605\u2605", 3),
    ("\u2605\u2605\u2605\u2605", 4),
    ("\u2605\u2605\u2605\u2605\u2605", 5),
]


class EditDialog(QDialog):
    """Modal dialog for editing WAD metadata.

    Mirrors the TUI's wad_edit screen with identical fields:
    - Basic info: title, author, year, status, rating, tags
    - Text: notes, description
    - Launch config: custom IWAD, sourceport, extra args
    """

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id
        self._wad = db.get_wad(wad_id)
        if not self._wad:
            return

        self.setWindowTitle(f"Edit: {self._wad['title']}")
        self.setMinimumWidth(500)
        self.setMinimumHeight(600)

        layout = QVBoxLayout(self)
        layout.setSpacing(12)

        # -- Basic Info --
        basic_group = QGroupBox("Basic Info")
        basic_form = QFormLayout(basic_group)

        self._title_input = QLineEdit(self._wad["title"])
        basic_form.addRow("Title:", self._title_input)

        self._author_input = QLineEdit(self._wad.get("author") or "")
        basic_form.addRow("Author:", self._author_input)

        self._year_input = QLineEdit(str(self._wad["year"]) if self._wad.get("year") else "")
        self._year_input.setMaxLength(4)
        self._year_input.setMaximumWidth(80)
        basic_form.addRow("Year:", self._year_input)

        self._status_combo = QComboBox()
        for display, value in _STATUS_OPTIONS:
            self._status_combo.addItem(display, value)
        # Select current status
        for i, (_, value) in enumerate(_STATUS_OPTIONS):
            if value == self._wad["status"]:
                self._status_combo.setCurrentIndex(i)
                break
        basic_form.addRow("Status:", self._status_combo)

        self._rating_combo = QComboBox()
        for display, rating_val in _RATING_OPTIONS:
            self._rating_combo.addItem(display, rating_val)
        # Select current rating
        current_rating = self._wad.get("rating")
        for i, (_, rating_val) in enumerate(_RATING_OPTIONS):
            if rating_val == current_rating:
                self._rating_combo.setCurrentIndex(i)
                break
        basic_form.addRow("Rating:", self._rating_combo)

        self._tags_input = QLineEdit(", ".join(self._wad.get("tags", [])))
        self._tags_input.setPlaceholderText("Comma-separated tags")
        basic_form.addRow("Tags:", self._tags_input)

        layout.addWidget(basic_group)

        # -- Text Fields --
        text_group = QGroupBox("Text")
        text_form = QFormLayout(text_group)

        self._notes_input = QTextEdit()
        self._notes_input.setPlainText(self._wad.get("notes") or "")
        self._notes_input.setMaximumHeight(100)
        text_form.addRow("Notes:", self._notes_input)

        self._desc_input = QTextEdit()
        self._desc_input.setPlainText(self._wad.get("description") or "")
        self._desc_input.setMaximumHeight(100)
        text_form.addRow("Description:", self._desc_input)

        layout.addWidget(text_group)

        # -- Launch Config --
        launch_group = QGroupBox("Launch Config")
        launch_form = QFormLayout(launch_group)

        self._iwad_input = QLineEdit(self._wad.get("custom_iwad") or "")
        self._iwad_input.setPlaceholderText("Override global IWAD")
        launch_form.addRow("Custom IWAD:", self._iwad_input)

        self._sourceport_input = QLineEdit(self._wad.get("custom_sourceport") or "")
        self._sourceport_input.setPlaceholderText("Override global sourceport")
        launch_form.addRow("Sourceport:", self._sourceport_input)

        # Parse existing custom_args JSON into space-separated string
        args_str = ""
        if self._wad.get("custom_args"):
            try:
                args_list = json.loads(self._wad["custom_args"])
                if isinstance(args_list, list):
                    args_str = " ".join(args_list)
            except json.JSONDecodeError:
                pass
        self._args_input = QLineEdit(args_str)
        self._args_input.setPlaceholderText("Extra sourceport arguments")
        launch_form.addRow("Extra Args:", self._args_input)

        # Parse existing companion_files JSON into newline-separated string
        companion_str = ""
        if self._wad.get("companion_files"):
            try:
                companion_list = json.loads(self._wad["companion_files"])
                if isinstance(companion_list, list):
                    companion_str = "\n".join(companion_list)
            except json.JSONDecodeError:
                pass
        self._companion_input = QTextEdit()
        self._companion_input.setPlainText(companion_str)
        self._companion_input.setPlaceholderText("One file path per line (DEH, music WADs, etc.)")
        self._companion_input.setMaximumHeight(80)
        launch_form.addRow("Companion Files:", self._companion_input)

        layout.addWidget(launch_group)

        # -- Buttons --
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self._save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _save(self):
        """Validate and save changes to the database."""
        title = self._title_input.text().strip()
        if not title:
            QMessageBox.warning(self, "Validation Error", "Title is required.")
            return

        # Validate year
        year = None
        year_text = self._year_input.text().strip()
        if year_text:
            try:
                year = int(year_text)
                if year < 1993 or year > 2100:
                    QMessageBox.warning(self, "Validation Error", "Year must be between 1993 and 2100.")
                    return
            except ValueError:
                QMessageBox.warning(self, "Validation Error", "Year must be a number.")
                return

        # Build update fields
        fields = {
            "title": title,
            "author": self._author_input.text().strip() or None,
            "year": year,
            "status": self._status_combo.currentData(),
            "rating": self._rating_combo.currentData(),
            "notes": self._notes_input.toPlainText().strip() or None,
            "description": self._desc_input.toPlainText().strip() or None,
            "custom_iwad": self._iwad_input.text().strip() or None,
            "custom_sourceport": self._sourceport_input.text().strip() or None,
        }

        # Parse extra args into JSON array
        args_text = self._args_input.text().strip()
        if args_text:
            fields["custom_args"] = json.dumps(args_text.split())
        else:
            fields["custom_args"] = None

        # Parse companion files (one per line)
        companion_text = self._companion_input.toPlainText().strip()
        if companion_text:
            companion_list = [line.strip() for line in companion_text.splitlines() if line.strip()]
            fields["companion_files"] = json.dumps(companion_list) if companion_list else None
        else:
            fields["companion_files"] = None

        # Update WAD in database
        db.update_wad(self._wad_id, **fields)

        # Sync tags: remove old, add new
        old_tags = set(self._wad.get("tags", []))
        new_tags = {t.strip().lower() for t in self._tags_input.text().split(",") if t.strip()}

        for tag in old_tags - new_tags:
            db.remove_tag(self._wad_id, tag)
        for tag in new_tags - old_tags:
            db.add_tag(self._wad_id, tag)

        self.accept()

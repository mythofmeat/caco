"""WAD editing dialogs — split into focused sections.

Each dialog handles a subset of WAD fields:
- EditMetadataDialog: title, author, year, status, rating, tags, description
- EditNotesDialog: notes
- EditSourceportDialog: sourceport, config profile, IWAD, complevel, extra args
- EditCompanionsDialog: companion file management
"""

import json

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QHBoxLayout,
    QFormLayout,
    QLineEdit,
    QComboBox,
    QTextEdit,
    QDialogButtonBox,
    QFileDialog,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPushButton,
)

from caco import db


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


class EditMetadataDialog(QDialog):
    """Edit WAD metadata: title, author, year, status, rating, tags, description."""

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id
        self._wad = db.get_wad(wad_id)
        if not self._wad:
            return

        self.setWindowTitle(f"Edit Metadata: {self._wad['title']}")
        self.setMinimumWidth(450)

        layout = QVBoxLayout(self)
        layout.setSpacing(10)

        form = QFormLayout()

        self._title_input = QLineEdit(self._wad["title"])
        form.addRow("Title:", self._title_input)

        # Author | Year row
        author_year = QHBoxLayout()
        self._author_input = QLineEdit(self._wad.get("author") or "")
        author_year.addWidget(self._author_input, 3)
        self._year_input = QLineEdit(str(self._wad["year"]) if self._wad.get("year") else "")
        self._year_input.setMaxLength(4)
        self._year_input.setMaximumWidth(80)
        self._year_input.setPlaceholderText("Year")
        author_year.addWidget(self._year_input, 1)
        form.addRow("Author | Year:", author_year)

        # Status | Rating row
        status_rating = QHBoxLayout()
        self._status_combo = QComboBox()
        for display, value in _STATUS_OPTIONS:
            self._status_combo.addItem(display, value)
        for i, (_, value) in enumerate(_STATUS_OPTIONS):
            if value == self._wad["status"]:
                self._status_combo.setCurrentIndex(i)
                break
        status_rating.addWidget(self._status_combo, 2)

        self._rating_combo = QComboBox()
        for display, rating_val in _RATING_OPTIONS:
            self._rating_combo.addItem(display, rating_val)
        current_rating = self._wad.get("rating")
        for i, (_, rating_val) in enumerate(_RATING_OPTIONS):
            if rating_val == current_rating:
                self._rating_combo.setCurrentIndex(i)
                break
        status_rating.addWidget(self._rating_combo, 1)
        form.addRow("Status | Rating:", status_rating)

        self._tags_input = QLineEdit(", ".join(self._wad.get("tags", [])))
        self._tags_input.setPlaceholderText("Comma-separated tags")
        form.addRow("Tags:", self._tags_input)

        self._desc_input = QTextEdit()
        self._desc_input.setPlainText(self._wad.get("description") or "")
        self._desc_input.setMaximumHeight(120)
        form.addRow("Description:", self._desc_input)

        layout.addLayout(form)

        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self._save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _save(self):
        title = self._title_input.text().strip()
        if not title:
            QMessageBox.warning(self, "Validation Error", "Title is required.")
            return

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

        fields = {
            "title": title,
            "author": self._author_input.text().strip() or None,
            "year": year,
            "status": self._status_combo.currentData(),
            "rating": self._rating_combo.currentData(),
            "description": self._desc_input.toPlainText().strip() or None,
        }

        db.update_wad(self._wad_id, **fields)

        # Sync tags
        old_tags = set(self._wad.get("tags", []))
        new_tags = {t.strip().lower() for t in self._tags_input.text().split(",") if t.strip()}
        for tag in old_tags - new_tags:
            db.remove_tag(self._wad_id, tag)
        for tag in new_tags - old_tags:
            db.add_tag(self._wad_id, tag)

        self.accept()


class EditNotesDialog(QDialog):
    """Edit WAD notes."""

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id
        self._wad = db.get_wad(wad_id)
        if not self._wad:
            return

        self.setWindowTitle(f"Edit Notes: {self._wad['title']}")
        self.setMinimumWidth(450)
        self.setMinimumHeight(300)

        layout = QVBoxLayout(self)
        layout.setSpacing(10)

        self._notes_input = QTextEdit()
        self._notes_input.setPlainText(self._wad.get("notes") or "")
        self._notes_input.setPlaceholderText("Your notes about this WAD...")
        layout.addWidget(self._notes_input)

        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self._save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _save(self):
        db.update_wad(self._wad_id, notes=self._notes_input.toPlainText().strip() or None)
        self.accept()


class EditSourceportDialog(QDialog):
    """Edit sourceport settings: sourceport, config profile, IWAD, complevel, extra args."""

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id
        self._wad = db.get_wad(wad_id)
        if not self._wad:
            return

        self.setWindowTitle(f"Sourceport Settings: {self._wad['title']}")
        self.setMinimumWidth(450)

        layout = QVBoxLayout(self)
        layout.setSpacing(10)

        form = QFormLayout()

        # Sourceport | Config profile row
        sp_config = QHBoxLayout()
        self._sourceport_input = QLineEdit(self._wad.get("custom_sourceport") or "")
        self._sourceport_input.setPlaceholderText("Override global sourceport")
        sp_config.addWidget(self._sourceport_input, 2)

        self._config_input = QLineEdit(self._wad.get("custom_config") or "")
        self._config_input.setPlaceholderText("Config profile")
        sp_config.addWidget(self._config_input, 1)
        form.addRow("Sourceport | Config:", sp_config)

        # IWAD | Complevel row
        iwad_cl = QHBoxLayout()
        self._iwad_combo = QComboBox()
        self._iwad_combo.setEditable(True)
        self._iwad_combo.setInsertPolicy(QComboBox.InsertPolicy.NoInsert)
        self._iwad_combo.addItem("(none)", "")
        all_iwads = db.get_all_iwads()
        seen_families: set[str] = set()
        for row in all_iwads:
            family = row["family"]
            if family not in seen_families:
                seen_families.add(family)
                self._iwad_combo.addItem(family, family)
        current_iwad = self._wad.get("custom_iwad") or ""
        idx = self._iwad_combo.findData(current_iwad)
        if idx >= 0:
            self._iwad_combo.setCurrentIndex(idx)
        elif current_iwad:
            self._iwad_combo.setCurrentText(current_iwad)
        else:
            self._iwad_combo.setCurrentIndex(0)
        iwad_cl.addWidget(self._iwad_combo, 2)

        self._complevel_input = QLineEdit(
            str(self._wad["complevel"]) if self._wad.get("complevel") is not None else ""
        )
        self._complevel_input.setPlaceholderText("e.g., 9, boom, mbf21")
        self._complevel_input.setMaximumWidth(120)
        iwad_cl.addWidget(self._complevel_input, 1)
        form.addRow("IWAD | Complevel:", iwad_cl)

        # Extra arguments
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
        form.addRow("Extra Args:", self._args_input)

        layout.addLayout(form)

        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self._save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _save(self):
        # Validate complevel
        complevel = None
        complevel_text = self._complevel_input.text().strip()
        if complevel_text:
            from caco.complevel import parse_complevel
            complevel = parse_complevel(complevel_text)
            if complevel is None:
                QMessageBox.warning(
                    self, "Validation Error",
                    "Invalid complevel. Use integer or alias (vanilla, boom, mbf, mbf21)."
                )
                return

        iwad_text = self._iwad_combo.currentText().strip()
        custom_iwad = iwad_text if iwad_text and iwad_text != "(none)" else None

        fields: dict = {
            "custom_iwad": custom_iwad,
            "custom_sourceport": self._sourceport_input.text().strip() or None,
            "complevel": complevel,
            "custom_config": self._config_input.text().strip() or None,
        }

        args_text = self._args_input.text().strip()
        fields["custom_args"] = json.dumps(args_text.split()) if args_text else None

        db.update_wad(self._wad_id, **fields)
        self.accept()


class EditCompanionsDialog(QDialog):
    """Manage companion files for a WAD."""

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id
        self._wad = db.get_wad(wad_id)
        if not self._wad:
            return

        self.setWindowTitle(f"Companion Files: {self._wad['title']}")
        self.setMinimumWidth(400)

        layout = QVBoxLayout(self)
        layout.setSpacing(10)

        self._companion_list = QListWidget()
        self._original_companions = db.get_wad_companions(self._wad_id)
        for comp in self._original_companions:
            item = QListWidgetItem(comp["filename"])
            item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
            item.setCheckState(Qt.CheckState.Checked if comp["enabled"] else Qt.CheckState.Unchecked)
            item.setData(Qt.ItemDataRole.UserRole, comp["id"])
            self._companion_list.addItem(item)
        layout.addWidget(self._companion_list)

        btn_row = QHBoxLayout()
        add_btn = QPushButton("Add File...")
        add_btn.clicked.connect(self._add_file)
        remove_btn = QPushButton("Remove")
        remove_btn.clicked.connect(self._remove_file)
        btn_row.addWidget(add_btn)
        btn_row.addWidget(remove_btn)
        btn_row.addStretch()
        layout.addLayout(btn_row)

        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self._save)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _add_file(self):
        path, _ = QFileDialog.getOpenFileName(
            self, "Add Companion File", "",
            "WAD/DEH Files (*.wad *.deh *.bex *.pk3 *.lmp);;All Files (*)",
        )
        if not path:
            return
        from pathlib import Path
        filename = Path(path).name
        item = QListWidgetItem(filename)
        item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
        item.setCheckState(Qt.CheckState.Checked)
        item.setData(Qt.ItemDataRole.UserRole, path)
        self._companion_list.addItem(item)

    def _remove_file(self):
        current = self._companion_list.currentItem()
        if current:
            self._companion_list.takeItem(self._companion_list.row(current))

    def _save(self):
        from caco.services.companion_service import register_companion, unregister_companion

        existing_ids: dict[int, bool] = {}
        pending_paths: list[str] = []
        for i in range(self._companion_list.count()):
            item = self._companion_list.item(i)
            data = item.data(Qt.ItemDataRole.UserRole)
            enabled = item.checkState() == Qt.CheckState.Checked
            if isinstance(data, int):
                existing_ids[data] = enabled
            else:
                pending_paths.append(data)

        for path in pending_paths:
            register_companion(path, self._wad_id)

        original_ids = {c["id"] for c in self._original_companions}
        for comp_id in original_ids - set(existing_ids.keys()):
            unregister_companion(self._wad_id, comp_id, orphan_policy="keep")

        for comp in self._original_companions:
            comp_id = comp["id"]
            if comp_id in existing_ids:
                new_enabled = existing_ids[comp_id]
                if new_enabled != bool(comp["enabled"]):
                    db.set_companion_enabled(self._wad_id, comp_id, new_enabled)

        self.accept()

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
    QFileDialog,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPushButton,
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

        self._iwad_combo = QComboBox()
        self._iwad_combo.setEditable(True)
        self._iwad_combo.setInsertPolicy(QComboBox.InsertPolicy.NoInsert)
        self._iwad_combo.addItem("(none)", "")
        # Populate with registered IWAD families
        all_iwads = db.get_all_iwads()
        seen_families: set[str] = set()
        for row in all_iwads:
            family = row["family"]
            if family not in seen_families:
                seen_families.add(family)
                self._iwad_combo.addItem(family, family)
        # Set current value
        current_iwad = self._wad.get("custom_iwad") or ""
        idx = self._iwad_combo.findData(current_iwad)
        if idx >= 0:
            self._iwad_combo.setCurrentIndex(idx)
        elif current_iwad:
            self._iwad_combo.setCurrentText(current_iwad)
        else:
            self._iwad_combo.setCurrentIndex(0)
        launch_form.addRow("Custom IWAD:", self._iwad_combo)

        self._sourceport_input = QLineEdit(self._wad.get("custom_sourceport") or "")
        self._sourceport_input.setPlaceholderText("Override global sourceport")
        launch_form.addRow("Sourceport:", self._sourceport_input)

        self._complevel_input = QLineEdit(
            str(self._wad["complevel"]) if self._wad.get("complevel") is not None else ""
        )
        self._complevel_input.setPlaceholderText("e.g., 9, boom, mbf21")
        self._complevel_input.setMaximumWidth(120)
        launch_form.addRow("Complevel:", self._complevel_input)

        self._config_input = QLineEdit(self._wad.get("custom_config") or "")
        self._config_input.setPlaceholderText("e.g., default, controller")
        launch_form.addRow("Config Profile:", self._config_input)

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

        # Companion files list with checkboxes
        companion_container = QVBoxLayout()
        self._companion_list = QListWidget()
        self._companion_list.setMaximumHeight(100)
        self._original_companions = db.get_wad_companions(self._wad_id)
        for comp in self._original_companions:
            item = QListWidgetItem(comp["filename"])
            item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
            item.setCheckState(Qt.CheckState.Checked if comp["enabled"] else Qt.CheckState.Unchecked)
            item.setData(Qt.ItemDataRole.UserRole, comp["id"])
            self._companion_list.addItem(item)
        companion_container.addWidget(self._companion_list)

        companion_buttons = QHBoxLayout()
        add_file_btn = QPushButton("Add File...")
        add_file_btn.clicked.connect(self._add_companion_file)
        remove_file_btn = QPushButton("Remove")
        remove_file_btn.clicked.connect(self._remove_companion_file)
        companion_buttons.addWidget(add_file_btn)
        companion_buttons.addWidget(remove_file_btn)
        companion_buttons.addStretch()
        companion_container.addLayout(companion_buttons)
        launch_form.addRow("Companion Files:", companion_container)

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

        # Parse custom IWAD from combo
        iwad_text = self._iwad_combo.currentText().strip()
        custom_iwad = iwad_text if iwad_text and iwad_text != "(none)" else None

        # Build update fields
        fields = {
            "title": title,
            "author": self._author_input.text().strip() or None,
            "year": year,
            "status": self._status_combo.currentData(),
            "rating": self._rating_combo.currentData(),
            "notes": self._notes_input.toPlainText().strip() or None,
            "description": self._desc_input.toPlainText().strip() or None,
            "custom_iwad": custom_iwad,
            "custom_sourceport": self._sourceport_input.text().strip() or None,
            "complevel": complevel,
            "custom_config": self._config_input.text().strip() or None,
        }

        # Parse extra args into JSON array
        args_text = self._args_input.text().strip()
        if args_text:
            fields["custom_args"] = json.dumps(args_text.split())
        else:
            fields["custom_args"] = None

        # Update WAD in database
        db.update_wad(self._wad_id, **fields)

        # Sync companion files: handle new files, removals, and enable/disable toggles
        self._save_companions()

        # Sync tags: remove old, add new
        old_tags = set(self._wad.get("tags", []))
        new_tags = {t.strip().lower() for t in self._tags_input.text().split(",") if t.strip()}

        for tag in old_tags - new_tags:
            db.remove_tag(self._wad_id, tag)
        for tag in new_tags - old_tags:
            db.add_tag(self._wad_id, tag)

        self.accept()

    def _add_companion_file(self):
        """Open file picker to stage a companion file (registered on save)."""
        path, _ = QFileDialog.getOpenFileName(
            self, "Add Companion File", "",
            "WAD/DEH Files (*.wad *.deh *.bex *.pk3 *.lmp);;All Files (*)",
        )
        if not path:
            return

        from pathlib import Path
        filename = Path(path).name
        # Store file path (str) in UserRole — registered on save, not now
        item = QListWidgetItem(filename)
        item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
        item.setCheckState(Qt.CheckState.Checked)
        item.setData(Qt.ItemDataRole.UserRole, path)
        self._companion_list.addItem(item)

    def _remove_companion_file(self):
        """Remove the selected companion file from the list."""
        current = self._companion_list.currentItem()
        if not current:
            return
        self._companion_list.takeItem(self._companion_list.row(current))

    def _save_companions(self):
        """Sync companion files: register new, remove deleted, toggle enabled."""
        from caco.services.companion_service import register_companion, unregister_companion

        # Partition list widget items into existing (int ID) and pending (str path)
        existing_ids: dict[int, bool] = {}
        pending_paths: list[str] = []
        for i in range(self._companion_list.count()):
            item = self._companion_list.item(i)
            data = item.data(Qt.ItemDataRole.UserRole)
            enabled = item.checkState() == Qt.CheckState.Checked
            if isinstance(data, int):
                existing_ids[data] = enabled
            else:
                # Pending file path — register now on save
                pending_paths.append(data)

        # Register pending companion files
        for path in pending_paths:
            register_companion(path, self._wad_id)

        # Remove companions that were in the original list but not in current
        original_ids = {c["id"] for c in self._original_companions}
        for comp_id in original_ids - set(existing_ids.keys()):
            unregister_companion(self._wad_id, comp_id, orphan_policy="keep")

        # Update enabled/disabled state for remaining companions
        for comp in self._original_companions:
            comp_id = comp["id"]
            if comp_id in existing_ids:
                new_enabled = existing_ids[comp_id]
                if new_enabled != bool(comp["enabled"]):
                    db.set_companion_enabled(self._wad_id, comp_id, new_enabled)

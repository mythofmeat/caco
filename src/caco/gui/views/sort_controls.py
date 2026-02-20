"""Sort field selector and direction toggle."""

from PySide6.QtCore import Signal
from PySide6.QtWidgets import QWidget, QHBoxLayout, QComboBox, QPushButton

from caco.gui.constants import SORT_FIELDS


class SortControls(QWidget):
    """Combo box for sort field + toggle button for asc/desc."""

    sort_changed = Signal(str, bool)  # (field_key, descending)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._desc = False

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(4)

        self._combo = QComboBox()
        for label, key in SORT_FIELDS.items():
            self._combo.addItem(label, key)
        self._combo.currentIndexChanged.connect(self._on_changed)
        layout.addWidget(self._combo)

        self._dir_btn = QPushButton("\u2191")  # Up arrow = ascending
        self._dir_btn.setObjectName("sort_dir_btn")
        self._dir_btn.setFixedWidth(30)
        self._dir_btn.setToolTip("Toggle sort direction")
        self._dir_btn.clicked.connect(self._toggle_direction)
        layout.addWidget(self._dir_btn)

    def _on_changed(self) -> None:
        self.sort_changed.emit(self.current_field(), self._desc)

    def _toggle_direction(self) -> None:
        self._desc = not self._desc
        self._dir_btn.setText("\u2193" if self._desc else "\u2191")
        self._dir_btn.setToolTip("Descending" if self._desc else "Ascending")
        self.sort_changed.emit(self.current_field(), self._desc)

    def current_field(self) -> str:
        data: str | None = self._combo.currentData()
        return data or "id"

    def is_descending(self) -> bool:
        return bool(self._desc)

    def set_sort(self, field: str, desc: bool) -> None:
        """Set sort field and direction programmatically (no signal emitted)."""
        self._combo.blockSignals(True)
        for i in range(self._combo.count()):
            if self._combo.itemData(i) == field:
                self._combo.setCurrentIndex(i)
                break
        self._combo.blockSignals(False)
        self._desc = desc
        self._dir_btn.setText("\u2193" if desc else "\u2191")

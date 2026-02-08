"""Filter input with beets-style query support."""

from PySide6.QtCore import Signal, QTimer
from PySide6.QtWidgets import QLineEdit


class FilterBar(QLineEdit):
    """Search/filter input that emits after a debounce delay.

    Supports the same beets-style query syntax as the CLI:
    - Field queries: status:playing, author:romero
    - Free text: "ancient aliens"
    - Negation: ^status:finished
    - OR groups: status:playing , status:to-play
    """

    query_changed = Signal(str)

    def __init__(self, parent=None, debounce_ms: int = 300):
        super().__init__(parent)
        self.setPlaceholderText("Filter... (e.g. status:playing, author:romero)")
        self.setClearButtonEnabled(True)

        self._debounce = QTimer(self)
        self._debounce.setSingleShot(True)
        self._debounce.setInterval(debounce_ms)
        self._debounce.timeout.connect(self._emit_query)

        self.textChanged.connect(self._on_text_changed)

    def _on_text_changed(self, text: str) -> None:
        self._debounce.start()

    def _emit_query(self) -> None:
        self.query_changed.emit(self.text().strip())

    def set_query(self, query: str) -> None:
        """Set the filter text programmatically without triggering debounce."""
        self._debounce.stop()
        self.setText(query)
        self._emit_query()

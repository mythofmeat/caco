"""Delete confirmation dialog with WAD stats."""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QLabel,
    QDialogButtonBox,
)

from caco import db
from caco.player import format_duration
from caco.gui.theme import DOOM_PALETTE


class DeleteDialog(QDialog):
    """Confirmation dialog for soft-deleting a WAD.

    Shows the WAD title, author, and play stats (session count, playtime)
    so the user can make an informed decision.
    """

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id

        wad = db.get_wad(wad_id)
        if not wad:
            self.reject()
            return

        self.setWindowTitle("Delete WAD")
        self.setMinimumWidth(350)

        layout = QVBoxLayout(self)
        layout.setSpacing(12)

        # Title
        title_label = QLabel("Delete WAD?")
        title_label.setStyleSheet(
            f"font-size: 16px; font-weight: bold; color: {DOOM_PALETTE['red']};"
        )
        layout.addWidget(title_label)

        # WAD info
        info_parts = [f"<b>{wad['title']}</b>"]
        if wad.get("author"):
            info_parts.append(f"by {wad['author']}")
        info_label = QLabel(" ".join(info_parts))
        info_label.setTextFormat(Qt.TextFormat.RichText)
        layout.addWidget(info_label)

        # Stats
        stats = db.get_wad_stats(wad_id)
        stat_parts = []
        if stats["session_count"]:
            stat_parts.append(f"{stats['session_count']} session(s)")
        if stats["total_playtime"]:
            stat_parts.append(f"{format_duration(stats['total_playtime'])} played")
        if stat_parts:
            stats_label = QLabel(" | ".join(stat_parts))
            stats_label.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
            layout.addWidget(stats_label)

        # Hint
        hint = QLabel("Moves to trash. Use 'caco restore' to recover.")
        hint.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']}; font-size: 11px;")
        layout.addWidget(hint)

        # Buttons
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Yes | QDialogButtonBox.StandardButton.No)
        buttons.button(QDialogButtonBox.StandardButton.Yes).setText("Delete")
        buttons.button(QDialogButtonBox.StandardButton.Yes).setStyleSheet(
            f"color: {DOOM_PALETTE['red']};"
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

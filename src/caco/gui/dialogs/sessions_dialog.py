"""Session history dialog."""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QAbstractItemView,
    QDialog,
    QVBoxLayout,
    QLabel,
    QTableWidget,
    QTableWidgetItem,
    QHeaderView,
    QDialogButtonBox,
)

from caco import db
from caco.player import format_duration
from caco.gui.theme import DOOM_PALETTE


class SessionsDialog(QDialog):
    """Modal dialog showing play session history for a WAD.

    Columns: Date, Started, Duration, Sourceport.
    """

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id

        wad = db.get_wad(wad_id)
        title = wad["title"] if wad else f"WAD {wad_id}"
        sessions = db.get_sessions(wad_id)

        self.setWindowTitle(f"Sessions: {title}")
        self.setMinimumWidth(600)
        self.setMinimumHeight(400)

        layout = QVBoxLayout(self)

        # Header
        header = QLabel(f"Session History: {title}")
        header.setStyleSheet(f"font-size: 14px; font-weight: bold; color: {DOOM_PALETTE['text_accent']};")
        layout.addWidget(header)

        count_label = QLabel(f"{len(sessions)} session(s)")
        count_label.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(count_label)

        # Table
        table = QTableWidget(len(sessions), 4)
        table.setHorizontalHeaderLabels(["Date", "Started", "Duration", "Sourceport"])
        table.setAlternatingRowColors(True)
        table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        table.verticalHeader().setVisible(False)

        header_view = table.horizontalHeader()
        header_view.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        header_view.setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        header_view.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        header_view.setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)

        for row, session in enumerate(sessions):
            started = session.get("started_at") or ""
            date_str = started[:10] if started else "-"
            time_str = started[11:16] if len(started) > 16 else "-"

            duration = session.get("duration_seconds")
            dur_str = format_duration(duration) if duration else "-"

            port = session.get("sourceport") or "-"

            table.setItem(row, 0, QTableWidgetItem(date_str))
            table.setItem(row, 1, QTableWidgetItem(time_str))

            dur_item = QTableWidgetItem(dur_str)
            dur_item.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            table.setItem(row, 2, dur_item)

            table.setItem(row, 3, QTableWidgetItem(port))

        layout.addWidget(table)

        # Close button
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

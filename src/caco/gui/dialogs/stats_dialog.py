"""Library statistics overview dialog."""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QHBoxLayout,
    QLabel,
    QGroupBox,
    QFormLayout,
    QTableWidget,
    QTableWidgetItem,
    QHeaderView,
    QDialogButtonBox,
)

from caco import db
from caco.player import format_duration
from caco.gui.theme import DOOM_PALETTE, get_status_display, get_status_color


class StatsDialog(QDialog):
    """Modal dialog showing library-wide statistics.

    Sections mirror the TUI's stats screen:
    - Overview (total WADs, sessions, playtime)
    - By Status breakdown
    - Completion stats
    - Monthly activity table
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Library Statistics")
        self.setMinimumWidth(550)
        self.setMinimumHeight(500)

        layout = QVBoxLayout(self)
        layout.setSpacing(12)

        stats = db.get_library_stats()
        completion = db.get_completion_rate()
        activity = db.get_wads_played_by_period("month")

        # -- Overview --
        overview = QGroupBox("Overview")
        ov_form = QFormLayout(overview)
        ov_form.addRow("Total WADs:", QLabel(str(stats["total_wads"])))
        ov_form.addRow("Total Sessions:", QLabel(str(stats["total_sessions"])))
        pt = format_duration(stats["total_playtime"]) if stats["total_playtime"] else "0s"
        ov_form.addRow("Total Playtime:", QLabel(pt))
        ov_form.addRow("WADs Played:", QLabel(str(stats["wads_with_sessions"])))
        layout.addWidget(overview)

        # -- By Status --
        status_group = QGroupBox("By Status")
        status_layout = QHBoxLayout(status_group)
        for status_val, count in sorted(stats["wads_by_status"].items(), key=lambda x: -x[1]):
            display = get_status_display(status_val)
            color = get_status_color(status_val)
            label = QLabel(f"{display}: {count}")
            label.setStyleSheet(f"color: {color.name()}; font-weight: bold; padding: 4px 8px;")
            status_layout.addWidget(label)
        status_layout.addStretch()
        layout.addWidget(status_group)

        # -- Completion --
        comp_group = QGroupBox("Completion")
        comp_form = QFormLayout(comp_group)
        comp_form.addRow("Played WADs:", QLabel(str(completion["played_wads"])))
        comp_form.addRow("Finished WADs:", QLabel(str(completion["finished_wads"])))
        rate = completion["completion_rate"]
        rate_label = QLabel(f"{rate:.0%}")
        rate_label.setStyleSheet(f"color: {DOOM_PALETTE['green']}; font-weight: bold;")
        comp_form.addRow("Completion Rate:", rate_label)
        comp_form.addRow("Total Completions:", QLabel(str(completion["total_completions"])))
        layout.addWidget(comp_group)

        # -- Monthly Activity --
        if activity:
            act_group = QGroupBox("Monthly Activity")
            act_layout = QVBoxLayout(act_group)

            months = activity[:12]  # Last 12 months
            table = QTableWidget(len(months), 4)
            table.setHorizontalHeaderLabels(["Period", "WADs", "Sessions", "Playtime"])
            table.setAlternatingRowColors(True)
            table.setEditTriggers(QTableWidget.NoEditTriggers)
            table.verticalHeader().setVisible(False)

            header = table.horizontalHeader()
            header.setSectionResizeMode(0, QHeaderView.ResizeToContents)
            header.setSectionResizeMode(1, QHeaderView.ResizeToContents)
            header.setSectionResizeMode(2, QHeaderView.ResizeToContents)
            header.setSectionResizeMode(3, QHeaderView.Stretch)

            for row, entry in enumerate(months):
                table.setItem(row, 0, QTableWidgetItem(entry["period"]))

                wad_item = QTableWidgetItem(str(entry["wad_count"]))
                wad_item.setTextAlignment(Qt.AlignRight | Qt.AlignVCenter)
                table.setItem(row, 1, wad_item)

                sess_item = QTableWidgetItem(str(entry["session_count"]))
                sess_item.setTextAlignment(Qt.AlignRight | Qt.AlignVCenter)
                table.setItem(row, 2, sess_item)

                pt_str = format_duration(entry["total_playtime"]) if entry["total_playtime"] else "-"
                pt_item = QTableWidgetItem(pt_str)
                pt_item.setTextAlignment(Qt.AlignRight | Qt.AlignVCenter)
                table.setItem(row, 3, pt_item)

            act_layout.addWidget(table)
            layout.addWidget(act_group)

        # Close
        buttons = QDialogButtonBox(QDialogButtonBox.Close)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

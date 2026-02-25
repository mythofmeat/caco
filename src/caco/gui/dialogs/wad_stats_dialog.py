"""Per-map WAD statistics dialog."""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QAbstractItemView,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE
from caco.wad_stats import (
    format_time_secs,
    format_time_tics,
    skill_name,
    stats_from_json,
)


class WadStatsDialog(QDialog):
    """Modal dialog showing per-map completion statistics for a WAD.

    Displays a table of per-map stats from a completion record's stats_snapshot.
    If multiple completions have stats, allows switching between them.
    """

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id

        wad = db.get_wad(wad_id)
        title = wad["title"] if wad else f"WAD {wad_id}"

        self.setWindowTitle(f"Map Stats: {title}")
        self.setMinimumWidth(700)
        self.setMinimumHeight(500)

        layout = QVBoxLayout(self)

        # Header
        header = QLabel(f"Map Statistics: {title}")
        header.setStyleSheet(
            f"font-size: 14px; font-weight: bold; color: {DOOM_PALETTE['text_accent']};"
        )
        layout.addWidget(header)

        # Completion selector (if multiple with stats)
        completions = db.get_wad_completions(wad_id)
        self._stats_completions = [
            c for c in completions if c.get("stats_snapshot")
        ]

        if len(self._stats_completions) > 1:
            selector_row = QHBoxLayout()
            selector_row.addWidget(QLabel("Completion:"))
            self._selector = QComboBox()
            for c in self._stats_completions:
                date = c["completed_at"][:16].replace("T", " ") if c["completed_at"] else "-"
                label = f"#{c['id']} ({date})"
                if c.get("notes"):
                    label += f" - {c['notes']}"
                self._selector.addItem(label)
            self._selector.currentIndexChanged.connect(self._on_selection_changed)
            selector_row.addWidget(self._selector)
            selector_row.addStretch()
            layout.addLayout(selector_row)

        # Summary label
        self._summary = QLabel()
        self._summary.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(self._summary)

        # Stats table
        self._table = QTableWidget()
        self._table.setAlternatingRowColors(True)
        self._table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        self._table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self._table.verticalHeader().setVisible(False)
        layout.addWidget(self._table)

        # Close button
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

        # Load first completion
        if self._stats_completions:
            self._load_stats(0)

    def _on_selection_changed(self, index: int) -> None:
        self._load_stats(index)

    def _load_stats(self, index: int) -> None:
        comp = self._stats_completions[index]
        wad_stats = stats_from_json(comp["stats_snapshot"])
        played = wad_stats.played_maps

        self._summary.setText(
            f"Format: {wad_stats.format} | "
            f"Maps played: {len(played)} | "
            f"Total time: {wad_stats.total_time_display}"
        )

        if wad_stats.format == "stats_txt":
            self._populate_stats_txt(played)
        else:
            self._populate_levelstat(played)

    def _populate_stats_txt(self, maps: list) -> None:
        """Populate table for nyan-doom/dsda-doom stats.txt format."""
        headers = ["Map", "Skill", "Time", "Max Time", "NM Time",
                    "Exits", "K", "I", "S"]
        self._table.setColumnCount(len(headers))
        self._table.setHorizontalHeaderLabels(headers)
        self._table.setRowCount(len(maps))

        for row, m in enumerate(maps):
            self._table.setItem(row, 0, QTableWidgetItem(m.lump))
            self._table.setItem(row, 1, QTableWidgetItem(skill_name(m.best_skill)))

            self._set_right_aligned(row, 2, format_time_tics(m.best_time))
            self._set_right_aligned(row, 3, format_time_tics(m.best_max_time))
            self._set_right_aligned(row, 4, format_time_tics(m.best_nm_time))
            self._set_right_aligned(row, 5, str(m.total_exits))

            k = f"{m.kills}/{m.total_kills}" if m.total_kills >= 0 else str(m.kills)
            i = f"{m.items}/{m.total_items}" if m.total_items >= 0 else str(m.items)
            s = f"{m.secrets}/{m.total_secrets}" if m.total_secrets >= 0 else str(m.secrets)

            self._set_right_aligned(row, 6, k)
            self._set_right_aligned(row, 7, i)
            self._set_right_aligned(row, 8, s)

        self._auto_resize_columns()

    def _populate_levelstat(self, maps: list) -> None:
        """Populate table for dsda-doom levelstat.txt format."""
        headers = ["Map", "Time", "Total Time", "K", "I", "S"]
        self._table.setColumnCount(len(headers))
        self._table.setHorizontalHeaderLabels(headers)
        self._table.setRowCount(len(maps))

        for row, m in enumerate(maps):
            self._table.setItem(row, 0, QTableWidgetItem(m.lump))
            self._set_right_aligned(row, 1, format_time_secs(m.time_secs))
            self._set_right_aligned(row, 2, format_time_secs(m.total_time_secs))
            self._set_right_aligned(row, 3, f"{m.kills}/{m.total_kills}")
            self._set_right_aligned(row, 4, f"{m.items}/{m.total_items}")
            self._set_right_aligned(row, 5, f"{m.secrets}/{m.total_secrets}")

        self._auto_resize_columns()

    def _set_right_aligned(self, row: int, col: int, text: str) -> None:
        item = QTableWidgetItem(text)
        item.setTextAlignment(
            Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter
        )
        self._table.setItem(row, col, item)

    def _auto_resize_columns(self) -> None:
        header = self._table.horizontalHeader()
        for col in range(self._table.columnCount() - 1):
            header.setSectionResizeMode(col, QHeaderView.ResizeMode.ResizeToContents)
        header.setSectionResizeMode(
            self._table.columnCount() - 1, QHeaderView.ResizeMode.Stretch
        )

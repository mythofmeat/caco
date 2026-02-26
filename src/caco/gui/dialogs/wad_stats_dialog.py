"""Per-map WAD statistics dialog with import/export support."""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QAbstractItemView,
    QComboBox,
    QDialog,
    QFileDialog,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
)

from caco import db
from caco.gui.theme import DOOM_PALETTE
from caco.wad_stats import (
    format_stats,
    format_time_secs,
    format_time_tics,
    parse_stats_file,
    skill_name,
    stats_from_json,
    stats_to_json,
)

_STATS_FILE_FILTER = "Stats files (*.txt);;All files (*)"


class WadStatsDialog(QDialog):
    """Modal dialog showing per-map completion statistics for a WAD.

    Displays a table of per-map stats from a completion record's stats_snapshot.
    If multiple completions have stats, allows switching between them.
    Supports importing stats from files and exporting stats back to text.
    """

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id
        self._changed = False

        wad = db.get_wad(wad_id)
        self._title = wad["title"] if wad else f"WAD {wad_id}"

        self.setWindowTitle(f"Map Stats: {self._title}")
        self.setMinimumWidth(700)
        self.setMinimumHeight(500)

        layout = QVBoxLayout(self)

        # Header
        header = QLabel(f"Map Statistics: {self._title}")
        header.setStyleSheet(
            f"font-size: 14px; font-weight: bold; color: {DOOM_PALETTE['text_accent']};"
        )
        layout.addWidget(header)

        # Completion selector (rebuilt on data change)
        self._selector_row = QHBoxLayout()
        self._selector_label = QLabel("Completion:")
        self._selector_row.addWidget(self._selector_label)
        self._selector = QComboBox()
        self._selector.currentIndexChanged.connect(self._on_selection_changed)
        self._selector_row.addWidget(self._selector)
        self._selector_row.addStretch()
        layout.addLayout(self._selector_row)

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

        # Button bar: [Import Stats...] [Export Stats...] <stretch> [Close]
        btn_layout = QHBoxLayout()

        self._import_btn = QPushButton("Import Stats...")
        self._import_btn.setToolTip("Import stats from a stats.txt or levelstat.txt file")
        self._import_btn.clicked.connect(self._on_import)
        btn_layout.addWidget(self._import_btn)

        self._export_btn = QPushButton("Export Stats...")
        self._export_btn.setToolTip("Export current stats to a text file")
        self._export_btn.clicked.connect(self._on_export)
        btn_layout.addWidget(self._export_btn)

        btn_layout.addStretch()

        close_btn = QPushButton("Close")
        close_btn.clicked.connect(self.reject)
        btn_layout.addWidget(close_btn)

        layout.addLayout(btn_layout)

        # Load completions
        self._stats_completions: list[dict] = []
        self._all_completions: list[dict] = []
        self._reload_completions()

    @property
    def changed(self) -> bool:
        """Whether stats were imported (caller should refresh)."""
        return self._changed

    def _reload_completions(self) -> None:
        """Reload completions from DB and refresh the UI."""
        self._all_completions = db.get_wad_completions(self._wad_id)
        self._stats_completions = []

        # Prepend live stats from wad's stats_snapshot if available
        wad = db.get_wad(self._wad_id)
        if wad and wad.get("stats_snapshot"):
            self._stats_completions.append({
                "id": None,
                "completed_at": None,
                "stats_snapshot": wad["stats_snapshot"],
                "notes": None,
                "_live": True,
            })

        self._stats_completions.extend(
            c for c in self._all_completions if c.get("stats_snapshot")
        )

        # Rebuild selector
        self._selector.blockSignals(True)
        self._selector.clear()
        for c in self._stats_completions:
            if c.get("_live"):
                label = "Current (live)"
            else:
                date = c["completed_at"][:16].replace("T", " ") if c["completed_at"] else "-"
                label = f"#{c['id']} ({date})"
                if c.get("notes"):
                    label += f" - {c['notes']}"
            self._selector.addItem(label)
        self._selector.blockSignals(False)

        has_stats = len(self._stats_completions) > 0
        self._selector_label.setVisible(has_stats)
        self._selector.setVisible(has_stats)
        self._export_btn.setEnabled(has_stats)

        if has_stats:
            self._load_stats(0)
        else:
            self._summary.setText("No stats available. Use Import Stats to load a stats file.")
            self._table.setRowCount(0)
            self._table.setColumnCount(0)

    def _on_selection_changed(self, index: int) -> None:
        if index >= 0:
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

    def _on_import(self) -> None:
        """Open file picker, parse stats, and store on a completion."""
        path, _ = QFileDialog.getOpenFileName(
            self, "Import Stats File", "", _STATS_FILE_FILTER
        )
        if not path:
            return

        try:
            wad_stats = parse_stats_file(path)
        except (ValueError, OSError) as e:
            QMessageBox.warning(self, "Import Error", f"Could not parse stats file:\n{e}")
            return

        stats_json = stats_to_json(wad_stats)
        played_count = len(wad_stats.played_maps)

        # Decide where to attach the stats
        if not self._all_completions:
            # No completions: create one
            db.add_wad_completion(self._wad_id, stats_snapshot=stats_json)
            self._changed = True
            self._reload_completions()
            QMessageBox.information(
                self, "Stats Imported",
                f"Created a new completion with {played_count} map stats.",
            )
        else:
            # Find completions without stats
            no_stats = [c for c in self._all_completions if not c.get("stats_snapshot")]
            if no_stats:
                # Attach to most recent completion without stats
                target = no_stats[-1]
                db.update_wad_completion(target["id"], stats_snapshot=stats_json)
                date = target["completed_at"][:16].replace("T", " ") if target["completed_at"] else "-"
                self._changed = True
                self._reload_completions()
                QMessageBox.information(
                    self, "Stats Imported",
                    f"Attached {played_count} map stats to completion #{target['id']} ({date}).",
                )
            else:
                # All completions already have stats — ask what to do
                reply = QMessageBox.question(
                    self, "All Completions Have Stats",
                    "All existing completions already have stats attached.\n\n"
                    "Create a new completion with these stats?",
                    QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
                    QMessageBox.StandardButton.Yes,
                )
                if reply == QMessageBox.StandardButton.Yes:
                    db.add_wad_completion(self._wad_id, stats_snapshot=stats_json)
                    self._changed = True
                    self._reload_completions()
                    # Select the newly added one (last in list)
                    if self._stats_completions:
                        self._selector.setCurrentIndex(len(self._stats_completions) - 1)
                    QMessageBox.information(
                        self, "Stats Imported",
                        f"Created a new completion with {played_count} map stats.",
                    )

    def _on_export(self) -> None:
        """Export the currently viewed stats to a file."""
        idx = self._selector.currentIndex()
        if idx < 0 or idx >= len(self._stats_completions):
            return

        comp = self._stats_completions[idx]
        wad_stats = stats_from_json(comp["stats_snapshot"])

        # Suggest filename based on format
        ext = "stats.txt" if wad_stats.format == "stats_txt" else "levelstat.txt"
        default_name = f"{self._title}_{ext}".replace(" ", "_")

        path, _ = QFileDialog.getSaveFileName(
            self, "Export Stats", default_name, _STATS_FILE_FILTER
        )
        if not path:
            return

        try:
            text = format_stats(wad_stats)
            with open(path, "w") as f:
                f.write(text)
            QMessageBox.information(
                self, "Stats Exported",
                f"Stats exported to:\n{path}",
            )
        except OSError as e:
            QMessageBox.warning(self, "Export Error", f"Could not write file:\n{e}")

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

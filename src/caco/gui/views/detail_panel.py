"""Right sidebar detail panel showing WAD metadata and stats."""

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QPixmap
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QLabel,
    QProgressBar,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QFrame,
)

from caco import db
from caco.player import format_duration
from caco.utils import format_author_year, format_rating, truncate
from caco.gui.theme import (
    DOOM_PALETTE,
    get_status_color,
    get_status_display,
)
from caco.wad_stats import format_map_progress, get_map_progress


class DetailPanel(QScrollArea):
    """Right-side panel showing details of the selected WAD."""

    play_requested = Signal(int)
    edit_requested = Signal(int)
    delete_requested = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._wad_id: int | None = None

        self.setWidgetResizable(True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setMinimumWidth(280)
        self.setMaximumWidth(400)

        # Container widget
        container = QWidget()
        self._layout = QVBoxLayout(container)
        self._layout.setContentsMargins(12, 12, 12, 12)
        self._layout.setSpacing(6)

        # Thumbnail placeholder
        self._thumbnail = QLabel()
        self._thumbnail.setFixedHeight(160)
        self._thumbnail.setAlignment(Qt.AlignCenter)
        self._thumbnail.setStyleSheet(
            f"background-color: {DOOM_PALETTE['bg_medium']}; "
            f"border: 1px solid {DOOM_PALETTE['border']}; "
            "border-radius: 4px;"
        )
        self._thumbnail.setText("No Image")
        self._thumbnail.setStyleSheet(
            self._thumbnail.styleSheet()
            + f" color: {DOOM_PALETTE['text_secondary']}; font-size: 12px;"
        )
        self._layout.addWidget(self._thumbnail)

        # ID + Title row
        title_row = QHBoxLayout()
        title_row.setSpacing(6)
        self._id_label = QLabel()
        self._id_label.setStyleSheet(
            f"color: {DOOM_PALETTE['text_secondary']}; font-size: 11px; font-weight: bold;"
        )
        title_row.addWidget(self._id_label, 0, Qt.AlignTop)
        self._title = QLabel()
        self._title.setObjectName("detail_title")
        self._title.setWordWrap(True)
        title_row.addWidget(self._title, 1)
        self._layout.addLayout(title_row)

        # Author + year
        self._author = QLabel()
        self._author.setObjectName("detail_author")
        self._layout.addWidget(self._author)

        # Status badge
        self._status = QLabel()
        self._layout.addWidget(self._status)

        # Rating
        self._rating = QLabel()
        self._layout.addWidget(self._rating)

        # Separator
        sep = QFrame()
        sep.setFrameShape(QFrame.HLine)
        sep.setStyleSheet(f"color: {DOOM_PALETTE['border']};")
        self._layout.addWidget(sep)

        # Stats section
        stats_header = QLabel("Stats")
        stats_header.setObjectName("detail_section_header")
        self._layout.addWidget(stats_header)

        self._playtime_label = QLabel()
        self._layout.addWidget(self._playtime_label)

        self._sessions_label = QLabel()
        self._layout.addWidget(self._sessions_label)

        self._progress_label = QLabel()
        self._progress_label.setVisible(False)
        self._layout.addWidget(self._progress_label)

        self._progress_bar = QProgressBar()
        self._progress_bar.setTextVisible(True)
        self._progress_bar.setFixedHeight(16)
        self._progress_bar.setVisible(False)
        self._layout.addWidget(self._progress_bar)

        beaten_row = QHBoxLayout()
        beaten_row.setSpacing(8)
        self._beaten_label = QLabel()
        beaten_row.addWidget(self._beaten_label)

        self._stats_btn = QPushButton("Map Stats")
        self._stats_btn.setToolTip("View or import per-map statistics")
        self._stats_btn.clicked.connect(self._on_view_stats)
        self._stats_btn.setVisible(False)
        beaten_row.addWidget(self._stats_btn)
        beaten_row.addStretch()
        self._layout.addLayout(beaten_row)

        self._last_played_label = QLabel()
        self._layout.addWidget(self._last_played_label)

        # Tags section
        tags_header = QLabel("Tags")
        tags_header.setObjectName("detail_section_header")
        self._layout.addWidget(tags_header)

        self._tags_container = QWidget()
        self._tags_layout = QHBoxLayout(self._tags_container)
        self._tags_layout.setContentsMargins(0, 0, 0, 0)
        self._tags_layout.setSpacing(4)
        self._tags_layout.addStretch()
        self._layout.addWidget(self._tags_container)

        # Description
        desc_header = QLabel("Description")
        desc_header.setObjectName("detail_section_header")
        self._layout.addWidget(desc_header)

        self._description = QLabel()
        self._description.setWordWrap(True)
        self._description.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        self._layout.addWidget(self._description)

        # Companion files
        self._companion_label = QLabel()
        self._companion_label.setWordWrap(True)
        self._companion_label.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']}; font-size: 11px;")
        self._companion_label.setVisible(False)
        self._layout.addWidget(self._companion_label)

        # Separator
        sep2 = QFrame()
        sep2.setFrameShape(QFrame.HLine)
        sep2.setStyleSheet(f"color: {DOOM_PALETTE['border']};")
        self._layout.addWidget(sep2)

        # Source info
        self._source_label = QLabel()
        self._source_label.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']}; font-size: 11px;")
        self._source_label.setTextFormat(Qt.RichText)
        self._source_label.setOpenExternalLinks(True)
        self._source_label.setWordWrap(True)
        self._layout.addWidget(self._source_label)

        # Action buttons
        btn_layout = QHBoxLayout()
        btn_layout.setSpacing(8)

        self._play_btn = QPushButton("\u25b6 Play")
        self._play_btn.setObjectName("play_button")
        self._play_btn.clicked.connect(self._on_play)
        btn_layout.addWidget(self._play_btn)

        self._edit_btn = QPushButton("\u270e Edit")
        self._edit_btn.clicked.connect(self._on_edit)
        btn_layout.addWidget(self._edit_btn)

        self._delete_btn = QPushButton("\u2717 Delete")
        self._delete_btn.setObjectName("delete_button")
        self._delete_btn.clicked.connect(self._on_delete)
        btn_layout.addWidget(self._delete_btn)

        self._layout.addLayout(btn_layout)

        # Push everything up
        self._layout.addStretch()

        self.setWidget(container)

        # Start with empty state
        self.clear()

    def clear(self):
        """Show empty state."""
        self._wad_id = None
        self._id_label.setText("")
        self._title.setText("No WAD selected")
        self._author.setText("")
        self._status.setText("")
        self._rating.setText("")
        self._playtime_label.setText("")
        self._sessions_label.setText("")
        self._progress_label.setText("")
        self._progress_label.setVisible(False)
        self._progress_bar.setVisible(False)
        self._beaten_label.setText("")
        self._stats_btn.setVisible(False)
        self._last_played_label.setText("")
        self._description.setText("")
        self._companion_label.setVisible(False)
        self._source_label.setText("")
        self._clear_tags()
        self._play_btn.setEnabled(False)
        self._edit_btn.setEnabled(False)
        self._delete_btn.setEnabled(False)

    def update_wad(self, wad_id: int, stats: dict | None = None, wad: dict | None = None):
        """Update the panel with a WAD's information.

        Args:
            wad_id: WAD ID to display.
            stats: Optional pre-fetched stats dict with keys:
                   playtime, last_played, times_beaten, session_count.
            wad: Optional pre-fetched WAD dict. If None, fetches from DB.
        """
        if wad is None:
            wad = db.get_wad(wad_id)
        if not wad:
            self.clear()
            return

        self._wad_id = wad_id

        # Reset thumbnail to placeholder while loading
        self._thumbnail.setPixmap(QPixmap())
        self._thumbnail.setText("Loading...")

        # ID + Title
        self._id_label.setText(f"#{wad_id}")
        self._title.setText(wad["title"])

        # Author + year
        self._author.setText(format_author_year(wad.get("author"), wad.get("year")))

        # Status
        status = wad["status"]
        color = get_status_color(status)
        display = get_status_display(status)
        self._status.setText(display)
        self._status.setStyleSheet(f"color: {color.name()}; font-weight: bold;")

        # Rating
        rating = wad.get("rating")
        if rating:
            self._rating.setText(format_rating(rating))
            self._rating.setStyleSheet(f"color: {DOOM_PALETTE['yellow']}; font-size: 14px;")
        else:
            self._rating.setText("No rating")
            self._rating.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")

        # Stats
        if stats:
            playtime = stats.get("playtime", 0)
            last_played = stats.get("last_played")
            times_beaten = stats.get("times_beaten", 0)
            session_count = stats.get("session_count", 0)
        else:
            playtime = db.get_total_playtime(wad_id)
            sessions = db.get_sessions(wad_id)
            last_played = db.get_last_played(wad_id)
            times_beaten = db.get_times_beaten(wad_id)
            session_count = len(sessions)

        self._playtime_label.setText(
            f"Playtime: {format_duration(playtime)}" if playtime else "Playtime: -"
        )
        self._sessions_label.setText(f"Sessions: {session_count}")

        progress = get_map_progress(wad.get("stats_snapshot"))
        if progress:
            display = format_map_progress(progress)
            self._progress_label.setText(f"Progress: {display}")
            self._progress_label.setVisible(True)
            if progress.total is not None and progress.total > 0:
                self._progress_bar.setMaximum(progress.total)
                self._progress_bar.setValue(progress.played)
                self._progress_bar.setVisible(True)
            else:
                self._progress_bar.setVisible(False)
        else:
            self._progress_label.setText("")
            self._progress_label.setVisible(False)
            self._progress_bar.setVisible(False)

        self._beaten_label.setText(f"Beaten: {times_beaten}")

        # Always show stats button so users can import stats
        self._stats_btn.setVisible(True)
        self._last_played_label.setText(
            f"Last played: {last_played[:10]}" if last_played else "Last played: -"
        )

        # Tags
        self._clear_tags()
        tags = wad.get("tags", [])
        if tags:
            for tag in tags:
                tag_label = QLabel(tag)
                tag_label.setObjectName("tag_label")
                # Insert before the stretch
                self._tags_layout.insertWidget(self._tags_layout.count() - 1, tag_label)
        else:
            no_tags = QLabel("No tags")
            no_tags.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
            self._tags_layout.insertWidget(0, no_tags)

        # Description
        self._description.setText(truncate(wad.get("description") or "No description", 500))

        # Companion files
        companions = db.get_wad_companions(wad["id"])
        if companions:
            parts = []
            for c in companions:
                name = c["filename"]
                if not c["enabled"]:
                    name += " (off)"
                parts.append(name)
            self._companion_label.setText(f"Companion files: {', '.join(parts)}")
            self._companion_label.setVisible(True)
        else:
            self._companion_label.setVisible(False)

        # Source info (HTML for clickable links)
        source_parts = [f"Source: {wad.get('source_type', 'unknown')}"]
        if wad.get("source_url"):
            url = wad["source_url"]
            if url.startswith(("http://", "https://")):
                source_parts.append(self._format_source_link(url, wad.get("source_type")))
            else:
                source_parts.append(f"URL: {url}")
        if wad.get("idgames_id"):
            idg_id = wad["idgames_id"]
            idg_url = f"https://www.doomworld.com/idgames/?id={idg_id}"
            source_parts.append(
                f'idgames: <a href="{idg_url}">{idg_id}</a>'
            )
        if wad.get("filename"):
            source_parts.append(f"File: {wad['filename']}")
        if wad.get("version"):
            source_parts.append(f"Version: {wad['version']}")
        if wad.get("complevel") is not None:
            from caco.complevel import complevel_name
            source_parts.append(f"Complevel: {wad['complevel']} ({complevel_name(wad['complevel'])})")
        if wad.get("custom_iwad"):
            source_parts.append(f"IWAD: {wad['custom_iwad']}")
        if wad.get("custom_config"):
            source_parts.append(f"Config: {wad['custom_config']}")
        self._source_label.setText("<br>".join(source_parts))

        # Enable buttons
        self._play_btn.setEnabled(True)
        self._edit_btn.setEnabled(True)
        self._delete_btn.setEnabled(True)

    def set_thumbnail(self, pixmap: QPixmap):
        """Set the thumbnail image."""
        scaled = pixmap.scaled(
            self._thumbnail.width() - 4,
            self._thumbnail.height() - 4,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        )
        self._thumbnail.setPixmap(scaled)

    def _clear_tags(self):
        """Remove all tag labels from the tags container."""
        while self._tags_layout.count() > 1:  # Keep the stretch
            item = self._tags_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

    def _on_play(self):
        if self._wad_id is not None:
            self.play_requested.emit(self._wad_id)

    def _on_edit(self):
        if self._wad_id is not None:
            self.edit_requested.emit(self._wad_id)

    def _on_delete(self):
        if self._wad_id is not None:
            self.delete_requested.emit(self._wad_id)

    @staticmethod
    def _format_source_link(url: str, source_type: str | None = None) -> str:
        """Format a source URL as a clickable HTML link with a short label."""
        if source_type == "doomwiki" or "doomwiki.org" in url:
            label = "Doom Wiki"
        elif source_type == "doomworld" or "doomworld.com/forum" in url:
            label = "Doomworld"
        elif source_type == "idgames" or "doomworld.com/idgames" in url:
            label = "idgames"
        else:
            label = "Link"
        return f'{label}: <a href="{url}">{url}</a>'

    def _on_view_stats(self):
        if self._wad_id is None:
            return
        from caco.gui.dialogs.wad_stats_dialog import WadStatsDialog
        dlg = WadStatsDialog(self._wad_id, parent=self)
        dlg.exec()
        if dlg.changed:
            self.update_wad(self._wad_id)

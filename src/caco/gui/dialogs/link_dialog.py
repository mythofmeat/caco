"""WAD unavailable dialog — shown when play fails due to missing WAD file."""

import shutil
from pathlib import Path

from PySide6.QtCore import Qt, QUrl
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QFileDialog,
    QMessageBox,
)

from caco import db
from caco.config import get_cache_dir
from caco.gui.theme import DOOM_PALETTE


class WadUnavailableDialog(QDialog):
    """Dialog shown when a WAD has no cached file and can't be auto-downloaded.

    Offers the user three options:
    - Open the source URL in a browser (if available)
    - Link a local file (copies to cache)
    - Cancel
    """

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id

        wad = db.get_wad(wad_id)
        if not wad:
            self.reject()
            return

        self._wad = wad
        self.setWindowTitle("WAD File Unavailable")
        self.setMinimumWidth(400)

        layout = QVBoxLayout(self)
        layout.setSpacing(12)

        # Title
        title_label = QLabel("WAD File Unavailable")
        title_label.setStyleSheet(
            f"font-size: 16px; font-weight: bold; color: {DOOM_PALETTE['yellow']};"
        )
        layout.addWidget(title_label)

        # WAD info
        info_parts = [f"<b>{wad['title']}</b>"]
        if wad.get("author"):
            info_parts.append(f"by {wad['author']}")
        info_label = QLabel(" ".join(info_parts))
        info_label.setTextFormat(Qt.TextFormat.RichText)
        layout.addWidget(info_label)

        # Explanation
        explanation = QLabel(
            "No WAD file is linked. Download from the source URL, "
            "or link a file you already have."
        )
        explanation.setWordWrap(True)
        explanation.setStyleSheet(f"color: {DOOM_PALETTE['text_secondary']};")
        layout.addWidget(explanation)

        # Buttons
        btn_layout = QHBoxLayout()
        btn_layout.setSpacing(8)

        # Open URL button (only if source_url exists)
        source_url = wad.get("source_url")
        if source_url:
            source_type = wad.get("source_type", "")
            if source_type == "doomwiki":
                url_label = "Open Wiki Page"
            elif source_type == "doomworld":
                url_label = "Open Forum Thread"
            else:
                url_label = "Open Source URL"

            open_url_btn = QPushButton(url_label)
            open_url_btn.clicked.connect(lambda: QDesktopServices.openUrl(QUrl(source_url)))
            btn_layout.addWidget(open_url_btn)

        # Link local file button
        link_btn = QPushButton("Link Local File...")
        link_btn.setStyleSheet(f"color: {DOOM_PALETTE['green']};")
        link_btn.clicked.connect(self._on_link)
        btn_layout.addWidget(link_btn)

        # Cancel button
        cancel_btn = QPushButton("Cancel")
        cancel_btn.clicked.connect(self.reject)
        btn_layout.addWidget(cancel_btn)

        layout.addLayout(btn_layout)

    def _on_link(self):
        """Open file picker and copy selected file to cache."""
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            "Select WAD File",
            "",
            "WAD Files (*.wad *.pk3 *.pk7 *.zip);;All Files (*)",
        )
        if not file_path:
            return

        source_path = Path(file_path).resolve()
        cache_dir = get_cache_dir()
        cache_dir.mkdir(parents=True, exist_ok=True)

        dest_filename = f"{self._wad_id}_{source_path.name}"
        dest_path = cache_dir / dest_filename

        # Remove existing cached file if present
        if self._wad.get("cached_path"):
            existing = Path(self._wad["cached_path"])
            if existing.exists():
                existing.unlink()

        try:
            shutil.copy2(str(source_path), str(dest_path))
        except OSError as e:
            QMessageBox.warning(self, "Link Failed", f"Could not copy file:\n{e}")
            return

        db.update_wad(
            self._wad_id,
            cached_path=str(dest_path),
            filename=source_path.name,
        )

        self.accept()

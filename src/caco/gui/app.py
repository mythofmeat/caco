"""QApplication setup with dark Doom-inspired palette."""

import sys
from pathlib import Path
from typing import cast

from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QPalette, QColor, QIcon
from PySide6.QtCore import Qt

from caco.gui.theme import DOOM_PALETTE, APP_STYLESHEET
from caco.gui.main_window import MainWindow


def create_app(config: dict | None = None) -> tuple[QApplication, MainWindow]:
    """Create and configure the QApplication and MainWindow.

    Returns (app, window) tuple. Caller should call app.exec() to start.
    """
    existing = QApplication.instance()
    app = cast(QApplication, existing) if existing else QApplication(sys.argv)
    app.setApplicationName("Caco")
    app.setApplicationDisplayName("Caco - Doom WAD Library")
    app.setDesktopFileName("caco")

    # Set window icon (used on X11 and as fallback on Wayland)
    icon_path = Path(__file__).resolve().parent.parent.parent / "desktop" / "icon.png"
    if icon_path.exists():
        app.setWindowIcon(QIcon(str(icon_path)))

    # Apply dark palette
    palette = QPalette()
    p = DOOM_PALETTE

    palette.setColor(QPalette.ColorRole.Window, QColor(p["bg_dark"]))
    palette.setColor(QPalette.ColorRole.WindowText, QColor(p["text_primary"]))
    palette.setColor(QPalette.ColorRole.Base, QColor(p["bg_dark"]))
    palette.setColor(QPalette.ColorRole.AlternateBase, QColor(p["bg_medium"]))
    palette.setColor(QPalette.ColorRole.Text, QColor(p["text_primary"]))
    palette.setColor(QPalette.ColorRole.Button, QColor(p["bg_medium"]))
    palette.setColor(QPalette.ColorRole.ButtonText, QColor(p["text_primary"]))
    palette.setColor(QPalette.ColorRole.Highlight, QColor(p["bg_selected"]))
    palette.setColor(QPalette.ColorRole.HighlightedText, QColor(p["text_primary"]))
    palette.setColor(QPalette.ColorRole.ToolTipBase, QColor(p["bg_medium"]))
    palette.setColor(QPalette.ColorRole.ToolTipText, QColor(p["text_primary"]))
    palette.setColor(QPalette.ColorRole.PlaceholderText, QColor(p["text_secondary"]))
    palette.setColor(QPalette.ColorRole.Link, QColor(p["text_accent"]))

    app.setPalette(palette)
    app.setStyleSheet(APP_STYLESHEET)

    window = MainWindow(config=config)
    return app, window

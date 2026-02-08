"""Doom-inspired dark theme for the GUI."""

from PySide6.QtGui import QColor

# Doom-inspired color palette
DOOM_PALETTE = {
    "bg_dark": "#1a1a1a",
    "bg_medium": "#2a2a2a",
    "bg_light": "#3a3a3a",
    "bg_selected": "#4a2a2a",
    "text_primary": "#e0e0e0",
    "text_secondary": "#a0a0a0",
    "text_accent": "#ff6633",
    "border": "#555555",
    "red": "#cc3333",
    "green": "#33cc33",
    "blue": "#3366cc",
    "yellow": "#cccc33",
    "brown": "#8b6914",
    "magenta": "#cc33cc",
    "grey": "#808080",
}

STATUS_COLORS = {
    "to-play": QColor("#3366cc"),
    "playing": QColor("#33cc33"),
    "finished": QColor("#808080"),
    "backlog": QColor("#cccc33"),
    "abandoned": QColor("#cc3333"),
    "awaiting-update": QColor("#cc33cc"),
}

STATUS_DISPLAY = {
    "to-play": "To Play",
    "backlog": "Backlog",
    "playing": "Playing",
    "finished": "Finished",
    "abandoned": "Abandoned",
    "awaiting-update": "Awaiting Update",
}


def get_status_color(status: str) -> QColor:
    """Get QColor for a status string."""
    return STATUS_COLORS.get(status, QColor(DOOM_PALETTE["text_secondary"]))


def get_status_display(status: str) -> str:
    """Get human-readable display name for a status."""
    return STATUS_DISPLAY.get(status, status)


# Main application stylesheet
APP_STYLESHEET = """
QMainWindow, QWidget {
    background-color: %(bg_dark)s;
    color: %(text_primary)s;
}

QTabBar {
    background-color: %(bg_dark)s;
    border: none;
}

QTabBar::tab {
    background-color: %(bg_medium)s;
    color: %(text_secondary)s;
    padding: 8px 16px;
    margin-right: 2px;
    border: 1px solid %(border)s;
    border-bottom: none;
    border-top-left-radius: 4px;
    border-top-right-radius: 4px;
}

QTabBar::tab:selected {
    background-color: %(bg_dark)s;
    color: %(text_accent)s;
    border-bottom: 2px solid %(text_accent)s;
}

QTabBar::tab:hover:!selected {
    background-color: %(bg_light)s;
    color: %(text_primary)s;
}

QTableView {
    background-color: %(bg_dark)s;
    alternate-background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    gridline-color: %(border)s;
    selection-background-color: %(bg_selected)s;
    selection-color: %(text_primary)s;
}

QTableView::item:hover {
    background-color: %(bg_light)s;
}

QHeaderView::section {
    background-color: %(bg_medium)s;
    color: %(text_accent)s;
    padding: 6px;
    border: 1px solid %(border)s;
    font-weight: bold;
}

QLineEdit {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
    padding: 6px 10px;
    selection-background-color: %(bg_selected)s;
}

QLineEdit:focus {
    border-color: %(text_accent)s;
}

QComboBox {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
    padding: 4px 8px;
    min-width: 120px;
}

QComboBox::drop-down {
    border: none;
    width: 20px;
}

QComboBox::down-arrow {
    image: none;
    border-left: 4px solid transparent;
    border-right: 4px solid transparent;
    border-top: 6px solid %(text_secondary)s;
    margin-right: 6px;
}

QComboBox QAbstractItemView {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    selection-background-color: %(bg_selected)s;
}

QPushButton {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
    padding: 6px 16px;
}

QPushButton:hover {
    background-color: %(bg_light)s;
    border-color: %(text_accent)s;
}

QPushButton:pressed {
    background-color: %(bg_selected)s;
}

QPushButton#sort_dir_btn {
    padding: 4px 2px;
    font-size: 14px;
    font-weight: bold;
}

QPushButton#play_button {
    background-color: #2a4a2a;
    border-color: %(green)s;
    color: %(green)s;
    font-weight: bold;
}

QPushButton#play_button:hover {
    background-color: #3a5a3a;
}

QPushButton#delete_button {
    border-color: %(red)s;
    color: %(red)s;
}

QPushButton#delete_button:hover {
    background-color: #4a2a2a;
}

QToolBar {
    background-color: %(bg_medium)s;
    border-bottom: 1px solid %(border)s;
    spacing: 6px;
    padding: 4px;
}

QSplitter::handle {
    background-color: %(border)s;
    width: 2px;
}

QStatusBar {
    background-color: %(bg_medium)s;
    color: %(text_secondary)s;
    border-top: 1px solid %(border)s;
}

QScrollBar:vertical {
    background-color: %(bg_dark)s;
    width: 12px;
    border: none;
}

QScrollBar::handle:vertical {
    background-color: %(bg_light)s;
    border-radius: 4px;
    min-height: 20px;
    margin: 2px;
}

QScrollBar::handle:vertical:hover {
    background-color: %(border)s;
}

QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
    height: 0;
}

QScrollBar:horizontal {
    background-color: %(bg_dark)s;
    height: 12px;
    border: none;
}

QScrollBar::handle:horizontal {
    background-color: %(bg_light)s;
    border-radius: 4px;
    min-width: 20px;
    margin: 2px;
}

QScrollBar::handle:horizontal:hover {
    background-color: %(border)s;
}

QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal {
    width: 0;
}

QLabel {
    color: %(text_primary)s;
}

QLabel#detail_title {
    font-size: 16px;
    font-weight: bold;
    color: %(text_accent)s;
}

QLabel#detail_author {
    font-size: 13px;
    color: %(text_secondary)s;
}

QLabel#detail_section_header {
    font-size: 11px;
    font-weight: bold;
    color: %(text_accent)s;
    padding-top: 8px;
}

QLabel#tag_label {
    background-color: %(bg_light)s;
    color: %(text_primary)s;
    border-radius: 3px;
    padding: 2px 6px;
    font-size: 11px;
}

QToolTip {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    padding: 4px;
}

QDialog {
    background-color: %(bg_dark)s;
    color: %(text_primary)s;
}

QGroupBox {
    color: %(text_accent)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
    margin-top: 8px;
    padding-top: 16px;
}

QGroupBox::title {
    subcontrol-origin: margin;
    left: 10px;
    padding: 0 4px;
}

QTextEdit, QPlainTextEdit {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
}

QSpinBox {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
    padding: 4px;
}

QCheckBox {
    color: %(text_primary)s;
    spacing: 8px;
}

QCheckBox::indicator {
    width: 16px;
    height: 16px;
    border: 1px solid %(border)s;
    border-radius: 3px;
    background-color: %(bg_medium)s;
}

QCheckBox::indicator:checked {
    background-color: %(text_accent)s;
    border-color: %(text_accent)s;
}

QProgressBar {
    background-color: %(bg_medium)s;
    border: 1px solid %(border)s;
    border-radius: 4px;
    text-align: center;
    color: %(text_primary)s;
}

QProgressBar::chunk {
    background-color: %(text_accent)s;
    border-radius: 3px;
}

QMenu {
    background-color: %(bg_medium)s;
    color: %(text_primary)s;
    border: 1px solid %(border)s;
}

QMenu::item:selected {
    background-color: %(bg_selected)s;
}

QMenu::separator {
    height: 1px;
    background-color: %(border)s;
    margin: 4px 8px;
}
""" % DOOM_PALETTE

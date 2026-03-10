"""GUI view widgets."""

from PySide6.QtGui import QAction
from PySide6.QtWidgets import QMenu, QWidget


def build_wad_context_menu(owner: QWidget, wad_id: int) -> QMenu:
    """Build the standard WAD context menu for list/grid views.

    Expects ``owner`` to have the standard action signals:
    play_requested, sessions_requested, wad_stats_requested,
    edit_{metadata,notes,sourceport,companions}_requested, delete_requested.
    """
    menu = QMenu(owner)

    play_action = QAction("Play", owner)
    play_action.triggered.connect(lambda: owner.play_requested.emit(wad_id))
    menu.addAction(play_action)

    menu.addSeparator()

    sessions_action = QAction("Sessions...", owner)
    sessions_action.triggered.connect(lambda: owner.sessions_requested.emit(wad_id))
    menu.addAction(sessions_action)

    stats_action = QAction("Map Stats...", owner)
    stats_action.triggered.connect(lambda: owner.wad_stats_requested.emit(wad_id))
    menu.addAction(stats_action)

    edit_menu = menu.addMenu("Edit")
    for label, signal_name in [
        ("Metadata...", "edit_metadata_requested"),
        ("Notes...", "edit_notes_requested"),
        ("Sourceport Settings...", "edit_sourceport_requested"),
        ("Companion Files...", "edit_companions_requested"),
    ]:
        action = QAction(label, owner)
        action.triggered.connect(lambda _, s=signal_name: getattr(owner, s).emit(wad_id))
        edit_menu.addAction(action)

    menu.addSeparator()

    delete_action = QAction("Delete", owner)
    delete_action.triggered.connect(lambda: owner.delete_requested.emit(wad_id))
    menu.addAction(delete_action)

    return menu

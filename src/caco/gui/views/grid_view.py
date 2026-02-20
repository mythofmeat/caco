"""Card-based grid view using QListView in IconMode with a custom delegate."""

from PySide6.QtCore import Qt, Signal, QSize, QRect, QModelIndex, QPersistentModelIndex
from PySide6.QtGui import QPixmap, QPainter, QColor, QFont, QFontMetrics, QPen
from PySide6.QtWidgets import (
    QListView,
    QStyledItemDelegate,
    QAbstractItemView,
    QStyle,
    QMenu,
)

from caco.gui.theme import DOOM_PALETTE, get_status_color, get_status_display
from caco.gui.constants import Column


# Default card dimensions
DEFAULT_CARD_WIDTH = 200
DEFAULT_CARD_HEIGHT = 240
DEFAULT_THUMB_HEIGHT = 120
PADDING = 8
TEXT_LINE_HEIGHT = 18
# Fixed text area height below thumbnail (title + author + status row)
TEXT_AREA_HEIGHT = TEXT_LINE_HEIGHT * 2 + 16 + 6 + 4  # title + author + badge + gaps


class WadCardDelegate(QStyledItemDelegate):
    """Custom delegate that paints WAD cards in the grid view.

    Each card shows:
    - Thumbnail (or placeholder) at the top
    - Title (bold, truncated)
    - Author
    - Status badge (colored)

    Card size is adjustable via set_card_size(). The thumbnail area scales
    while the text area stays fixed.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self._thumbnails: dict[int, QPixmap] = {}
        self._card_width = DEFAULT_CARD_WIDTH
        self._card_height = DEFAULT_CARD_HEIGHT
        self._thumb_height = DEFAULT_THUMB_HEIGHT

    def set_thumbnail(self, wad_id: int, pixmap: QPixmap):
        """Cache a thumbnail for a wad_id."""
        self._thumbnails[wad_id] = pixmap

    def set_card_size(self, width: int):
        """Set card width; height and thumb area scale proportionally."""
        self._card_width = width
        self._thumb_height = int(width * 0.6)
        self._card_height = self._thumb_height + TEXT_AREA_HEIGHT + PADDING * 2

    def card_size(self) -> QSize:
        """Return the current card size."""
        return QSize(self._card_width, self._card_height)

    def sizeHint(self, option, index):
        return QSize(self._card_width, self._card_height)

    def paint(self, painter: QPainter, option, index: QModelIndex | QPersistentModelIndex):
        painter.save()
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        rect = option.rect
        card_w = self._card_width
        thumb_h = self._thumb_height
        is_selected = option.state & QStyle.StateFlag.State_Selected
        is_hover = option.state & QStyle.StateFlag.State_MouseOver

        # Background
        if is_selected:
            bg = QColor(DOOM_PALETTE["bg_selected"])
        elif is_hover:
            bg = QColor(DOOM_PALETTE["bg_light"])
        else:
            bg = QColor(DOOM_PALETTE["bg_medium"])

        painter.setBrush(bg)
        painter.setPen(QPen(QColor(DOOM_PALETTE["border"]), 1))
        painter.drawRoundedRect(rect.adjusted(2, 2, -2, -2), 6, 6)

        # Get WAD data from model
        model = index.model()
        wad_id = model.data(model.index(index.row(), 0), Qt.ItemDataRole.UserRole)

        # Thumbnail area
        thumb_rect = QRect(
            rect.x() + PADDING,
            rect.y() + PADDING,
            card_w - 2 * PADDING,
            thumb_h,
        )

        if wad_id in self._thumbnails:
            pixmap = self._thumbnails[wad_id]
            scaled = pixmap.scaled(
                thumb_rect.size(),
                Qt.AspectRatioMode.KeepAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )
            # Center within thumb_rect
            x = thumb_rect.x() + (thumb_rect.width() - scaled.width()) // 2
            y = thumb_rect.y() + (thumb_rect.height() - scaled.height()) // 2
            painter.drawPixmap(x, y, scaled)
        else:
            # Dark placeholder
            painter.setBrush(QColor(DOOM_PALETTE["bg_dark"]))
            painter.setPen(Qt.PenStyle.NoPen)
            painter.drawRoundedRect(thumb_rect, 4, 4)

        # Text area starts below thumbnail
        text_y = rect.y() + PADDING + thumb_h + 6

        # Title (bold)
        title_font = QFont()
        title_font.setPixelSize(13)
        title_font.setBold(True)
        painter.setFont(title_font)
        painter.setPen(QColor(DOOM_PALETTE["text_accent"]))

        # Get title from the model's display data
        title_val = model.data(model.index(index.row(), _col_index(model, Column.TITLE)), Qt.ItemDataRole.DisplayRole)
        title = str(title_val) if title_val else ""
        title_rect = QRect(rect.x() + PADDING, text_y, card_w - 2 * PADDING, TEXT_LINE_HEIGHT)
        painter.drawText(title_rect, Qt.AlignmentFlag.AlignLeft | Qt.TextFlag.TextSingleLine, _elide(painter.fontMetrics(), title, title_rect.width()))

        # Author
        text_y += TEXT_LINE_HEIGHT + 2
        author_font = QFont()
        author_font.setPixelSize(11)
        painter.setFont(author_font)
        painter.setPen(QColor(DOOM_PALETTE["text_secondary"]))

        author_val = model.data(model.index(index.row(), _col_index(model, Column.AUTHOR)), Qt.ItemDataRole.DisplayRole)
        author = str(author_val) if author_val else ""
        author_rect = QRect(rect.x() + PADDING, text_y, card_w - 2 * PADDING, TEXT_LINE_HEIGHT)
        painter.drawText(author_rect, Qt.AlignmentFlag.AlignLeft | Qt.TextFlag.TextSingleLine, _elide(painter.fontMetrics(), author, author_rect.width()))

        # Status badge
        text_y += TEXT_LINE_HEIGHT + 4
        status_val = model.data(model.index(index.row(), _col_index(model, Column.STATUS)), Qt.ItemDataRole.DisplayRole)
        status_text = str(status_val) if status_val else ""
        raw_status = ""
        # Get raw status for color lookup
        wad = model.get_wad(index.row()) if hasattr(model, 'get_wad') else None
        if wad:
            raw_status = wad.get("status", "")

        badge_font = QFont()
        badge_font.setPixelSize(10)
        badge_font.setBold(True)
        painter.setFont(badge_font)

        fm = painter.fontMetrics()
        badge_width = fm.horizontalAdvance(status_text) + 12
        badge_rect = QRect(rect.x() + PADDING, text_y, badge_width, 16)

        status_color = get_status_color(raw_status) if raw_status else QColor(DOOM_PALETTE["text_secondary"])
        painter.setBrush(status_color)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.drawRoundedRect(badge_rect, 3, 3)

        painter.setPen(QColor(DOOM_PALETTE["bg_dark"]))
        painter.drawText(badge_rect, Qt.AlignmentFlag.AlignCenter, status_text)

        # Playtime (bottom-right)
        playtime_val = model.data(model.index(index.row(), _col_index(model, Column.PLAYTIME)), Qt.ItemDataRole.DisplayRole)
        playtime = str(playtime_val) if playtime_val else ""
        if playtime and playtime != "-":
            pt_font = QFont()
            pt_font.setPixelSize(10)
            painter.setFont(pt_font)
            painter.setPen(QColor(DOOM_PALETTE["text_secondary"]))
            pt_rect = QRect(rect.x() + PADDING, text_y, card_w - 2 * PADDING, 16)
            painter.drawText(pt_rect, Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter, playtime)

        painter.restore()


def _col_index(model: object, column: Column) -> int:
    """Find the column index for a Column enum in the model."""
    columns = getattr(model, '_columns', None)
    if columns is not None:
        try:
            idx: int = columns.index(column)
            return idx
        except ValueError:
            pass
    return 0


def _elide(fm: QFontMetrics, text: str, width: int) -> str:
    """Elide text with ellipsis if it exceeds width."""
    if fm.horizontalAdvance(text) <= width:
        return text
    while len(text) > 1 and fm.horizontalAdvance(text + "...") > width:
        text = text[:-1]
    return text + "..."


class WadGridView(QListView):
    """Grid view displaying WAD cards with thumbnails."""

    wad_selected = Signal(int)
    wad_activated = Signal(int)
    selection_cleared = Signal()

    play_requested = Signal(int)
    edit_requested = Signal(int)
    delete_requested = Signal(int)
    sessions_requested = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)

        self._delegate = WadCardDelegate(self)
        self.setItemDelegate(self._delegate)

        self.setViewMode(QListView.IconMode)
        self.setResizeMode(QListView.Adjust)
        self.setUniformItemSizes(True)
        self._update_grid_size()
        self.setSpacing(4)
        self.setSelectionMode(QAbstractItemView.SingleSelection)
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.customContextMenuRequested.connect(self._show_context_menu)

    def set_card_size(self, width: int):
        """Set the card width — scales thumbnail area proportionally."""
        self._delegate.set_card_size(width)
        self._update_grid_size()
        self.viewport().update()

    def _update_grid_size(self):
        """Sync QListView grid size with the delegate's card size."""
        cs = self._delegate.card_size()
        self.setGridSize(QSize(cs.width() + 8, cs.height() + 8))

    def set_thumbnail(self, wad_id: int, pixmap: QPixmap):
        """Set a thumbnail in the delegate and trigger repaint."""
        self._delegate.set_thumbnail(wad_id, pixmap)
        self.viewport().update()

    def currentChanged(self, current: QModelIndex | QPersistentModelIndex, previous: QModelIndex | QPersistentModelIndex):
        super().currentChanged(current, previous)
        wad_id = self._wad_id_at(current)
        if wad_id is not None:
            self.wad_selected.emit(wad_id)
        else:
            self.selection_cleared.emit()

    def keyPressEvent(self, event):
        if event.key() in (Qt.Key_Return, Qt.Key_Enter):
            wad_id = self._wad_id_at(self.currentIndex())
            if wad_id is not None:
                self.wad_activated.emit(wad_id)
                return
        super().keyPressEvent(event)

    def mouseDoubleClickEvent(self, event):
        idx = self.indexAt(event.pos())
        wad_id = self._wad_id_at(idx)
        if wad_id is not None:
            self.wad_activated.emit(wad_id)
        else:
            super().mouseDoubleClickEvent(event)

    def _wad_id_at(self, index: QModelIndex | QPersistentModelIndex) -> int | None:
        if index.isValid():
            idx = self.model().index(index.row(), 0)
            result: int | None = self.model().data(idx, Qt.ItemDataRole.UserRole)
            return result
        return None

    def _show_context_menu(self, pos):
        idx = self.indexAt(pos)
        wad_id = self._wad_id_at(idx)
        if wad_id is None:
            return

        from PySide6.QtGui import QAction
        menu = QMenu(self)

        play_action = QAction("Play", self)
        play_action.triggered.connect(lambda: self.play_requested.emit(wad_id))
        menu.addAction(play_action)

        menu.addSeparator()

        sessions_action = QAction("Sessions...", self)
        sessions_action.triggered.connect(lambda: self.sessions_requested.emit(wad_id))
        menu.addAction(sessions_action)

        edit_action = QAction("Edit...", self)
        edit_action.triggered.connect(lambda: self.edit_requested.emit(wad_id))
        menu.addAction(edit_action)

        menu.addSeparator()

        delete_action = QAction("Delete", self)
        delete_action.triggered.connect(lambda: self.delete_requested.emit(wad_id))
        menu.addAction(delete_action)

        menu.exec(self.viewport().mapToGlobal(pos))

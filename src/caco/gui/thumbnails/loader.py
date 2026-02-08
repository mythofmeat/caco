"""Async QThreadPool-based thumbnail loader.

Sequence per WAD:
1. Check cache -> emit immediately if hit
2. Try extract_titlepic() if WAD has cached_path
3. Try fetch_wiki_image() if source is doomwiki
4. Generate colored placeholder with WAD initials as fallback
5. Emit thumbnail_ready(wad_id, QPixmap) signal
"""

from io import BytesIO

from PySide6.QtCore import QObject, QRunnable, QThreadPool, Signal, Slot
from PySide6.QtGui import QPixmap, QImage, QColor, QPainter, QFont

from caco.gui.thumbnails import cache as thumb_cache
from caco.gui.theme import DOOM_PALETTE


class ThumbnailSignals(QObject):
    """Signals emitted by thumbnail workers."""
    ready = Signal(int, QPixmap)  # (wad_id, thumbnail)


class ThumbnailWorker(QRunnable):
    """Load or generate a thumbnail for a single WAD."""

    def __init__(self, wad_id: int, cached_path: str | None, source_type: str | None,
                 source_url: str | None, title: str):
        super().__init__()
        self.wad_id = wad_id
        self.cached_path = cached_path
        self.source_type = source_type
        self.source_url = source_url
        self.title = title
        self.signals = ThumbnailSignals()

    @Slot()
    def run(self):
        pixmap = None

        # 1. Check cache
        cached_bytes = thumb_cache.load(self.wad_id)
        if cached_bytes:
            pixmap = _bytes_to_pixmap(cached_bytes)
            if pixmap and not pixmap.isNull():
                self.signals.ready.emit(self.wad_id, pixmap)
                return

        # 2. Try TITLEPIC extraction
        if self.cached_path:
            try:
                from caco.gui.thumbnails.extractor import extract_titlepic
                img = extract_titlepic(self.cached_path)
                if img:
                    buf = BytesIO()
                    img.save(buf, format="PNG")
                    png_bytes = buf.getvalue()
                    thumb_cache.save(self.wad_id, png_bytes)
                    pixmap = _bytes_to_pixmap(png_bytes)
                    if pixmap and not pixmap.isNull():
                        self.signals.ready.emit(self.wad_id, pixmap)
                        return
            except Exception:
                pass

        # 3. Try wiki scraping (direct URL for doomwiki, title search for others)
        try:
            from caco.gui.thumbnails.scraper import fetch_wiki_image, search_wiki_image
            img_bytes = None
            if self.source_type == "doomwiki" and self.source_url:
                img_bytes = fetch_wiki_image(self.source_url)
            if not img_bytes and self.title:
                img_bytes = search_wiki_image(self.title)
            if img_bytes:
                thumb_cache.save(self.wad_id, img_bytes)
                pixmap = _bytes_to_pixmap(img_bytes)
                if pixmap and not pixmap.isNull():
                    self.signals.ready.emit(self.wad_id, pixmap)
                    return
        except Exception:
            pass

        # 4. Generate placeholder
        pixmap = _generate_placeholder(self.title, self.wad_id)
        self.signals.ready.emit(self.wad_id, pixmap)


class ThumbnailLoader(QObject):
    """Manages async thumbnail loading with QThreadPool.

    Connect to thumbnail_ready to receive (wad_id, QPixmap) pairs as they load.
    """

    thumbnail_ready = Signal(int, QPixmap)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._pool = QThreadPool.globalInstance()
        self._pending: set[int] = set()

    def request(self, wad_id: int, cached_path: str | None = None,
                source_type: str | None = None, source_url: str | None = None,
                title: str = ""):
        """Request a thumbnail for a WAD. Results arrive via thumbnail_ready signal."""
        if wad_id in self._pending:
            return

        self._pending.add(wad_id)
        worker = ThumbnailWorker(wad_id, cached_path, source_type, source_url, title)
        worker.signals.ready.connect(self._on_ready)
        self._pool.start(worker)

    def _on_ready(self, wad_id: int, pixmap: QPixmap):
        self._pending.discard(wad_id)
        self.thumbnail_ready.emit(wad_id, pixmap)


def _bytes_to_pixmap(data: bytes) -> QPixmap | None:
    """Convert image bytes to QPixmap."""
    img = QImage()
    if img.loadFromData(data):
        return QPixmap.fromImage(img)
    return None


def _generate_placeholder(title: str, wad_id: int) -> QPixmap:
    """Generate a colored placeholder with WAD initials."""
    # Deterministic color from wad_id
    colors = [
        DOOM_PALETTE["red"], DOOM_PALETTE["green"], DOOM_PALETTE["blue"],
        DOOM_PALETTE["brown"], DOOM_PALETTE["magenta"], DOOM_PALETTE["yellow"],
    ]
    bg_color = QColor(colors[wad_id % len(colors)])

    # Get initials (up to 2 chars)
    words = title.split()
    if len(words) >= 2:
        initials = words[0][0].upper() + words[1][0].upper()
    elif words:
        initials = words[0][:2].upper()
    else:
        initials = "?"

    pixmap = QPixmap(160, 100)
    pixmap.fill(bg_color)

    painter = QPainter(pixmap)
    painter.setPen(QColor(DOOM_PALETTE["text_primary"]))
    font = QFont()
    font.setPixelSize(36)
    font.setBold(True)
    painter.setFont(font)
    painter.drawText(pixmap.rect(), 0x0084, initials)  # AlignCenter
    painter.end()

    return pixmap

"""Async QThreadPool-based thumbnail loader.

Sequence per WAD:
1. Try extract_titlepic() if WAD has cached_path (always preferred — actual WAD art)
2. Check thumbnail cache (may contain wiki image from earlier)
3. Try wiki scraping (direct URL for doomwiki, title search for others)
4. Generate colored placeholder with WAD initials as fallback
5. Emit thumbnail_ready(wad_id, QPixmap) signal

IMPORTANT: Worker threads use QImage (thread-safe) exclusively.
Conversion to QPixmap happens only on the main thread in _on_ready().
"""

import logging
import threading
from io import BytesIO

logger = logging.getLogger(__name__)

from PySide6.QtCore import QObject, QRunnable, QThreadPool, Signal, Slot
from PySide6.QtGui import QPixmap, QImage, QColor, QPainter, QFont

from caco.gui.thumbnails import cache as thumb_cache
from caco.gui.theme import DOOM_PALETTE


class ThumbnailSignals(QObject):
    """Signals emitted by thumbnail workers."""
    # Carries QImage (thread-safe) — converted to QPixmap on main thread
    ready = Signal(int, QImage)


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
        # Top-level try/except: signal MUST always emit so _pending is cleared
        try:
            image = self._load_image()
        except Exception as exc:
            logger.debug("Failed to load thumbnail for WAD %d: %s", self.wad_id, exc)
            image = None

        if image is None or image.isNull():
            image = _generate_placeholder_image(self.title, self.wad_id)

        self.signals.ready.emit(self.wad_id, image)

    def _load_image(self) -> QImage | None:
        """Try all image sources in priority order. Returns QImage or None.

        Priority: TITLEPIC (from WAD file) > cached thumbnail > wiki scrape.
        TITLEPIC always wins when available because it's the actual WAD art,
        and overwrites any previously cached wiki image.
        """

        # 1. Try TITLEPIC extraction first — always preferred when WAD is downloaded
        if self.cached_path:
            try:
                from caco.gui.thumbnails.extractor import extract_titlepic
                pil_img = extract_titlepic(self.cached_path)
                if pil_img:
                    buf = BytesIO()
                    pil_img.save(buf, format="PNG")
                    png_bytes = buf.getvalue()
                    thumb_cache.save(self.wad_id, png_bytes)
                    img = QImage()
                    if img.loadFromData(png_bytes):
                        return img
            except Exception as exc:
                logger.debug("Failed to extract TITLEPIC for WAD %d (%s): %s", self.wad_id, self.cached_path, exc)

        # 2. Check filesystem cache (wiki image or previous TITLEPIC)
        cached_bytes = thumb_cache.load(self.wad_id)
        if cached_bytes:
            img = QImage()
            if img.loadFromData(cached_bytes):
                return img

        # 3. Try wiki scraping (direct URL for doomwiki, title search for all)
        try:
            from caco.gui.thumbnails.scraper import fetch_wiki_image, search_wiki_image
            img_bytes = None
            if self.source_type == "doomwiki" and self.source_url:
                img_bytes = fetch_wiki_image(self.source_url)
            if not img_bytes and self.title:
                img_bytes = search_wiki_image(self.title)
            if img_bytes:
                thumb_cache.save(self.wad_id, img_bytes)
                img = QImage()
                if img.loadFromData(img_bytes):
                    return img
        except Exception as exc:
            logger.debug("Failed to scrape wiki thumbnail for WAD %d (%s): %s", self.wad_id, self.title, exc)

        return None


class ThumbnailLoader(QObject):
    """Manages async thumbnail loading with QThreadPool.

    Connect to thumbnail_ready to receive (wad_id, QPixmap) pairs as they load.
    """

    thumbnail_ready = Signal(int, QPixmap)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._pool = QThreadPool.globalInstance()
        self._pending: set[int] = set()
        self._lock = threading.Lock()

    def request(self, wad_id: int, cached_path: str | None = None,
                source_type: str | None = None, source_url: str | None = None,
                title: str = ""):
        """Request a thumbnail for a WAD. Results arrive via thumbnail_ready signal."""
        with self._lock:
            if wad_id in self._pending:
                return
            self._pending.add(wad_id)
        worker = ThumbnailWorker(wad_id, cached_path, source_type, source_url, title)
        worker.signals.ready.connect(self._on_ready)
        self._pool.start(worker)

    def _on_ready(self, wad_id: int, image: QImage):
        """Convert QImage to QPixmap on the main thread and emit."""
        with self._lock:
            self._pending.discard(wad_id)
        pixmap = QPixmap.fromImage(image)
        self.thumbnail_ready.emit(wad_id, pixmap)


def _generate_placeholder_image(title: str, wad_id: int) -> QImage:
    """Generate a colored placeholder with WAD initials using QImage (thread-safe)."""
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

    image = QImage(160, 100, QImage.Format_RGB32)
    image.fill(bg_color)

    painter = QPainter(image)
    painter.setPen(QColor(DOOM_PALETTE["text_primary"]))
    font = QFont()
    font.setPixelSize(36)
    font.setBold(True)
    painter.setFont(font)
    painter.drawText(image.rect(), 0x0084, initials)  # AlignCenter
    painter.end()

    return image

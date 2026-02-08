"""QRunnable worker for import operations."""

from PySide6.QtCore import QObject, QRunnable, Signal, Slot


class ImportSignals(QObject):
    """Signals for import workers."""
    finished = Signal(int)   # wad_id
    error = Signal(str)


class IdgamesImportWorker(QRunnable):
    """Import a WAD from idgames in a background thread."""

    def __init__(self, entry, tags: list[str] | None = None):
        super().__init__()
        self.entry = entry
        self.tags = tags
        self.signals = ImportSignals()

    @Slot()
    def run(self):
        try:
            from caco.sources.idgames import IdgamesSource
            with IdgamesSource() as source:
                wad_id = source.import_wad(self.entry, tags=self.tags)
            self.signals.finished.emit(wad_id)
        except Exception as e:
            self.signals.error.emit(str(e))


class DoomwikiImportWorker(QRunnable):
    """Import a WAD from Doom Wiki in a background thread."""

    def __init__(self, entry, tags: list[str] | None = None):
        super().__init__()
        self.entry = entry
        self.tags = tags
        self.signals = ImportSignals()

    @Slot()
    def run(self):
        try:
            from caco.sources.doomwiki import DoomwikiSource
            with DoomwikiSource() as source:
                wad_id = source.import_wad(self.entry, tags=self.tags)
            self.signals.finished.emit(wad_id)
        except Exception as e:
            self.signals.error.emit(str(e))


class DoomworldImportWorker(QRunnable):
    """Import a WAD from Doomworld in a background thread."""

    def __init__(
        self,
        thread,
        tags: list[str] | None = None,
        title: str | None = None,
        author: str | None = None,
        year: int | None = None,
    ):
        super().__init__()
        self.thread = thread
        self.tags = tags
        self.title = title
        self.author = author
        self.year = year
        self.signals = ImportSignals()

    @Slot()
    def run(self):
        try:
            from caco.sources.doomworld import DoomworldSource
            with DoomworldSource() as source:
                wad_id = source.import_wad(
                    self.thread,
                    tags=self.tags,
                    title=self.title,
                    author=self.author,
                    year=self.year,
                )
            self.signals.finished.emit(wad_id)
        except Exception as e:
            self.signals.error.emit(str(e))

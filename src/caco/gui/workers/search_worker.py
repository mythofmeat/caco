"""QRunnable workers for API searches."""

from PySide6.QtCore import QObject, QRunnable, Signal, Slot


class WorkerSignals(QObject):
    """Signals for search/import workers (QRunnable can't have signals directly)."""
    finished = Signal(list)
    error = Signal(str)


class IdgamesSearchWorker(QRunnable):
    """Search idgames archive in a background thread."""

    def __init__(self, query: str):
        super().__init__()
        self.query = query
        self.signals = WorkerSignals()

    @Slot()
    def run(self):
        try:
            from caco.sources.idgames import IdgamesSource
            with IdgamesSource() as source:
                results = source.search(self.query)
            self.signals.finished.emit(results)
        except Exception as e:
            self.signals.error.emit(str(e))


class DoomwikiSearchWorker(QRunnable):
    """Search Doom Wiki in a background thread."""

    def __init__(self, query: str):
        super().__init__()
        self.query = query
        self.signals = WorkerSignals()

    @Slot()
    def run(self):
        try:
            from caco.sources.doomwiki import DoomwikiSource
            with DoomwikiSource() as source:
                results = source.search(self.query)
            self.signals.finished.emit(results)
        except Exception as e:
            self.signals.error.emit(str(e))


class DoomworldFetchWorker(QRunnable):
    """Fetch a Doomworld forum thread in a background thread."""

    def __init__(self, url: str):
        super().__init__()
        self.url = url
        self.signals = WorkerSignals()

    @Slot()
    def run(self):
        try:
            from caco.sources.doomworld import DoomworldSource
            with DoomworldSource() as source:
                thread = source.get(self.url)
            # Wrap in list to match signal type
            self.signals.finished.emit([thread] if thread else [])
        except Exception as e:
            self.signals.error.emit(str(e))

"""Background thread for launching sourceport and tracking play sessions."""

from PySide6.QtCore import QThread, Signal

from caco.player import play


class PlayWorker(QThread):
    """Runs player.play() in a dedicated thread so the GUI stays responsive.

    The sourceport process blocks until the user exits; this thread ensures
    the main event loop keeps painting and handling events.
    """

    finished = Signal(int, object)  # (wad_id, duration_seconds | None)
    error = Signal(int, str)        # (wad_id, error_message)
    download_progress = Signal(int, int, str)  # (downloaded, total, filename)

    def __init__(self, wad_id: int, parent=None):
        super().__init__(parent)
        self._wad_id = wad_id

    def _on_progress(self, downloaded: int, total: int, filename: str):
        self.download_progress.emit(downloaded, total, filename)

    def run(self):
        try:
            duration = play(
                self._wad_id, console=None,
                progress_callback=self._on_progress,
            )
            self.finished.emit(self._wad_id, duration)
        except Exception as e:
            self.error.emit(self._wad_id, str(e))

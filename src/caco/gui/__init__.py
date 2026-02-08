"""PySide6-based GUI for caco."""

import signal

from caco.config import get_gui_config
from caco.gui.app import create_app


class CacoGuiApp:
    """Entry point for the GUI application."""

    def run(self) -> int:
        """Launch the GUI. Returns the exit code."""
        # Restore default SIGINT handler so Ctrl-C works in the terminal.
        # Qt's event loop installs its own handler that swallows the signal.
        signal.signal(signal.SIGINT, signal.SIG_DFL)

        config = get_gui_config()
        app, window = create_app(config=config)
        window.show()
        return app.exec()


__all__ = ["CacoGuiApp"]

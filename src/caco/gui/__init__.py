"""PySide6-based GUI for caco."""

from caco.config import get_gui_config
from caco.gui.app import create_app


class CacoGuiApp:
    """Entry point for the GUI application."""

    def run(self) -> int:
        """Launch the GUI. Returns the exit code."""
        config = get_gui_config()
        app, window = create_app(config=config)
        window.show()
        return app.exec()


__all__ = ["CacoGuiApp"]

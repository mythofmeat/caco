"""Local file import pane widget for the TUI."""

from pathlib import Path

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Button, Input, Static, TextArea


class LocalImportPane(Widget):
    """Form for importing local WAD files into the library."""

    BINDINGS = [
        Binding("ctrl+enter", "import_wad", "Import", show=True),
    ]

    class WadImported(Message):
        """Fired when a WAD is successfully imported."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    DEFAULT_CSS = """
    LocalImportPane {
        height: 100%;
        width: 100%;
    }

    LocalImportPane #form-area {
        width: 100%;
        height: 100%;
        padding: 1 2;
        align: center top;
    }

    LocalImportPane #form-box {
        width: 80;
        max-width: 100%;
        height: auto;
    }

    LocalImportPane #form-title {
        text-style: bold;
        margin-bottom: 1;
    }

    LocalImportPane .form-row {
        height: auto;
        margin-bottom: 1;
        align: left middle;
    }

    LocalImportPane .form-label {
        width: 12;
        color: $text-muted;
    }

    LocalImportPane .form-input {
        width: 1fr;
    }

    LocalImportPane .required {
        color: $error;
    }

    LocalImportPane #desc-area {
        height: 6;
        width: 1fr;
    }

    LocalImportPane #import-btn {
        margin-top: 1;
        width: 100%;
    }

    LocalImportPane #file-info {
        color: $text-muted;
        margin-bottom: 1;
    }

    LocalImportPane #status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(id="form-area"):
            with Vertical(id="form-box"):
                yield Static("Import Local File", id="form-title")

                # Path (required)
                with Horizontal(classes="form-row"):
                    yield Static("Path*:", classes="form-label")
                    yield Input(
                        id="path-input",
                        classes="form-input",
                        placeholder="/path/to/wad.wad or .pk3 or .zip"
                    )

                yield Static("", id="file-info")

                # Title (auto-inferred but editable)
                with Horizontal(classes="form-row"):
                    yield Static("Title*:", classes="form-label")
                    yield Input(id="title-input", classes="form-input", placeholder="WAD title (auto-filled from filename)")

                # Author
                with Horizontal(classes="form-row"):
                    yield Static("Author:", classes="form-label")
                    yield Input(id="author-input", classes="form-input", placeholder="Author name")

                # Year
                with Horizontal(classes="form-row"):
                    yield Static("Year:", classes="form-label")
                    yield Input(id="year-input", classes="form-input", placeholder="Release year", max_length=4)

                # Tags
                with Horizontal(classes="form-row"):
                    yield Static("Tags:", classes="form-label")
                    yield Input(id="tags-input", classes="form-input", placeholder="comma,separated,tags")

                # Description
                with Horizontal(classes="form-row"):
                    yield Static("Notes:", classes="form-label")
                    yield TextArea(id="desc-area")

                yield Button("Import", id="import-btn", variant="success")

        yield Static("Tab Navigate  Ctrl+Enter Import  1-5 Source", id="status")

    def on_input_changed(self, event: Input.Changed) -> None:
        """Auto-fill title when path changes."""
        if event.input.id == "path-input":
            self._update_from_path(event.value)

    def _update_from_path(self, path_str: str) -> None:
        """Update title and file info based on path."""
        path_str = path_str.strip()
        file_info = self.query_one("#file-info", Static)
        title_input = self.query_one("#title-input", Input)

        if not path_str:
            file_info.update("")
            return

        path = Path(path_str).expanduser()

        if path.exists():
            if path.is_file():
                size = path.stat().st_size
                from caco.utils import format_size
                size_str = format_size(size)
                file_info.update(f"File exists: {size_str}")

                # Auto-fill title from filename (without extension)
                if not title_input.value.strip():
                    stem = path.stem
                    # Clean up common naming patterns
                    title = stem.replace("_", " ").replace("-", " ")
                    title_input.value = title
            else:
                file_info.update("Path is a directory, not a file")
        else:
            file_info.update("File not found (will be recorded as reference)")



    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle import button click."""
        if event.button.id == "import-btn":
            self.action_import_wad()

    def action_import_wad(self) -> None:
        """Import the WAD with form values."""
        path_str = self.query_one("#path-input", Input).value.strip()
        title = self.query_one("#title-input", Input).value.strip()
        author = self.query_one("#author-input", Input).value.strip() or None
        year_str = self.query_one("#year-input", Input).value.strip()
        tags_str = self.query_one("#tags-input", Input).value.strip()
        description = self.query_one("#desc-area", TextArea).text.strip() or None

        # Validation
        if not path_str:
            self.notify("Path is required", severity="warning")
            return

        path = Path(path_str).expanduser().resolve()

        # Infer title from filename if not provided
        if not title:
            title = path.stem.replace("_", " ").replace("-", " ")

        if not title:
            self.notify("Title is required", severity="warning")
            return

        year = None
        if year_str:
            try:
                year = int(year_str)
            except ValueError:
                self.notify("Year must be a number", severity="warning")
                return

        tags = [t.strip() for t in tags_str.split(",") if t.strip()] if tags_str else None

        status = self.query_one("#status", Static)
        status.update(f"Importing {title}...")

        self.run_worker(
            self._do_import(title, path_str, author, year, description, tags),
            exclusive=False,
        )

    async def _do_import(
        self,
        title: str,
        path: str,
        author: str | None,
        year: int | None,
        description: str | None,
        tags: list[str] | None,
    ) -> None:
        """Perform the import in a worker."""
        from caco.services import ImportService

        result = ImportService().import_local(
            title, path, author=author, year=year,
            description=description, tags=tags,
        )

        status = self.query_one("#status", Static)
        if result.is_duplicate:
            self.notify(
                f"Already in library: {result.duplicate_title} (ID: {result.duplicate_id})",
                severity="warning",
            )
            status.update("File already exists in library")
        elif result.error:
            self.notify(f"Import failed: {result.error}", severity="error")
            status.update(f"Import failed: {result.error}")
        else:
            self.notify(f"Imported: {title} (ID: {result.wad_id})")
            status.update(f"Successfully imported as ID {result.wad_id}")
            self._clear_form()
            if result.wad_id is not None:
                self.post_message(self.WadImported(result.wad_id))

    def _clear_form(self) -> None:
        """Clear all form fields."""
        self.query_one("#path-input", Input).value = ""
        self.query_one("#title-input", Input).value = ""
        self.query_one("#author-input", Input).value = ""
        self.query_one("#year-input", Input).value = ""
        self.query_one("#tags-input", Input).value = ""
        self.query_one("#desc-area", TextArea).clear()
        self.query_one("#file-info", Static).update("")

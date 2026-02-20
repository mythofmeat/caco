"""Manual URL import pane widget for the TUI."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Button, Input, Static, TextArea


class UrlImportPane(Widget):
    """Simple form for manually importing WADs via URL."""

    BINDINGS = [
        Binding("ctrl+enter", "import_wad", "Import", show=True),
    ]

    class WadImported(Message):
        """Fired when a WAD is successfully imported."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    DEFAULT_CSS = """
    UrlImportPane {
        height: 100%;
        width: 100%;
    }

    UrlImportPane #form-area {
        width: 100%;
        height: 100%;
        padding: 1 2;
        align: center top;
    }

    UrlImportPane #form-box {
        width: 80;
        max-width: 100%;
        height: auto;
    }

    UrlImportPane #form-title {
        text-style: bold;
        margin-bottom: 1;
    }

    UrlImportPane .form-row {
        height: auto;
        margin-bottom: 1;
        align: left middle;
    }

    UrlImportPane .form-label {
        width: 12;
        color: $text-muted;
    }

    UrlImportPane .form-input {
        width: 1fr;
    }

    UrlImportPane .required {
        color: $error;
    }

    UrlImportPane #desc-area {
        height: 6;
        width: 1fr;
    }

    UrlImportPane #import-btn {
        margin-top: 1;
        width: 100%;
    }

    UrlImportPane #status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(id="form-area"):
            with Vertical(id="form-box"):
                yield Static("Import from URL", id="form-title")

                # Title (required)
                with Horizontal(classes="form-row"):
                    yield Static("Title*:", classes="form-label")
                    yield Input(id="title-input", classes="form-input", placeholder="WAD title")

                # URL (required)
                with Horizontal(classes="form-row"):
                    yield Static("URL*:", classes="form-label")
                    yield Input(id="url-input", classes="form-input", placeholder="Download URL")

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

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle import button click."""
        if event.button.id == "import-btn":
            self.action_import_wad()

    def action_import_wad(self) -> None:
        """Import the WAD with form values."""
        title = self.query_one("#title-input", Input).value.strip()
        url = self.query_one("#url-input", Input).value.strip()
        author = self.query_one("#author-input", Input).value.strip() or None
        year_str = self.query_one("#year-input", Input).value.strip()
        tags_str = self.query_one("#tags-input", Input).value.strip()
        description = self.query_one("#desc-area", TextArea).text.strip() or None

        # Validation
        if not title:
            self.notify("Title is required", severity="warning")
            return

        if not url:
            self.notify("URL is required", severity="warning")
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
            self._do_import(title, url, author, year, description, tags),
            exclusive=False,
        )

    async def _do_import(
        self,
        title: str,
        url: str,
        author: str | None,
        year: int | None,
        description: str | None,
        tags: list[str] | None,
    ) -> None:
        """Perform the import in a worker."""
        from caco.services import ImportService

        result = ImportService().import_url(
            title, url, author=author, year=year,
            description=description, tags=tags,
        )

        status = self.query_one("#status", Static)
        if result.is_duplicate:
            self.notify(
                f"Already in library: {result.duplicate_title} (ID: {result.duplicate_id})",
                severity="warning",
            )
            status.update("URL already exists in library")
        elif result.error:
            self.notify(f"Import failed: {result.error}", severity="error")
            status.update(f"Import failed: {result.error}")
        else:
            self.notify(f"Imported: {title} (ID: {result.wad_id})")
            status.update(f"Successfully imported as ID {result.wad_id}")
            self._clear_form()
            self.post_message(self.WadImported(result.wad_id))

    def _clear_form(self) -> None:
        """Clear all form fields."""
        self.query_one("#title-input", Input).value = ""
        self.query_one("#url-input", Input).value = ""
        self.query_one("#author-input", Input).value = ""
        self.query_one("#year-input", Input).value = ""
        self.query_one("#tags-input", Input).value = ""
        self.query_one("#desc-area", TextArea).clear()

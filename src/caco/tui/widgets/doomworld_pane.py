"""Doomworld forum URL pane widget for the TUI."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widget import Widget
from textual.widgets import Button, Input, Static
from textual.worker import Worker, get_current_worker

from caco.doomworld.models import ForumThread
from caco.sources.doomworld import DoomworldSource


class DoomworldUrlPane(Widget):
    """URL-based pane for importing WADs from Doomworld forum threads."""

    BINDINGS = [
        Binding("ctrl+enter", "import_wad", "Import", show=True),
    ]

    class WadImported(Message):
        """Fired when a WAD is successfully imported."""

        def __init__(self, wad_id: int) -> None:
            super().__init__()
            self.wad_id = wad_id

    DEFAULT_CSS = """
    DoomworldUrlPane {
        height: 100%;
        width: 100%;
    }

    DoomworldUrlPane #url-header {
        height: 3;
        width: 100%;
        padding: 0 1;
        align: left middle;
    }

    DoomworldUrlPane #url-input {
        width: 1fr;
    }

    DoomworldUrlPane #fetch-btn {
        margin-left: 1;
    }

    DoomworldUrlPane #content-area {
        height: 1fr;
    }

    DoomworldUrlPane #form-container {
        width: 1fr;
        height: 100%;
        padding: 1 2;
    }

    DoomworldUrlPane #preview-container {
        width: 1fr;
        height: 100%;
        border-left: solid $primary;
        padding: 1 2;
    }

    DoomworldUrlPane .form-row {
        height: auto;
        margin-bottom: 1;
        align: left middle;
    }

    DoomworldUrlPane .form-label {
        width: 10;
        color: $text-muted;
    }

    DoomworldUrlPane .form-input {
        width: 1fr;
    }

    DoomworldUrlPane #preview-header {
        text-style: bold;
        margin-bottom: 1;
    }

    DoomworldUrlPane #preview-meta {
        color: $text-muted;
        margin-bottom: 1;
    }

    DoomworldUrlPane #preview-excerpt {
        color: $text;
        height: 1fr;
        overflow: auto;
    }

    DoomworldUrlPane #import-btn {
        margin-top: 1;
    }

    DoomworldUrlPane #status {
        dock: bottom;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }

    DoomworldUrlPane #placeholder {
        width: 100%;
        height: 100%;
        content-align: center middle;
        color: $text-muted;
    }
    """

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._thread: ForumThread | None = None
        self._current_worker: Worker | None = None

    def compose(self) -> ComposeResult:
        with Horizontal(id="url-header"):
            yield Input(
                placeholder="Doomworld forum thread URL (e.g., https://www.doomworld.com/forum/topic/...)",
                id="url-input",
            )
            yield Button("Fetch", id="fetch-btn", variant="primary")
        with Horizontal(id="content-area"):
            with Vertical(id="form-container"):
                yield Static("", id="placeholder")
            with Vertical(id="preview-container"):
                yield Static("Thread Preview", id="preview-header")
                yield Static("", id="preview-meta")
                yield Static("", id="preview-excerpt")
        yield Static("Tab Enter form  Ctrl+Enter Import  1-5 Source", id="status")

    def on_mount(self) -> None:
        """Initial setup."""
        # Update the placeholder text (already created in compose)
        try:
            placeholder = self.query_one("#placeholder", Static)
            placeholder.update("Paste a Doomworld forum thread URL above")
        except Exception:
            pass

    def _show_placeholder(self, text: str) -> None:
        """Show placeholder text in form area."""
        form = self.query_one("#form-container", Vertical)
        # Check if placeholder already exists
        try:
            placeholder = form.query_one("#placeholder", Static)
            placeholder.update(text)
        except Exception:
            # No placeholder - clear form and create one
            form.remove_children()
            form.mount(Static(text, id="placeholder"))

    def _show_form(self) -> None:
        """Show the editable form."""
        form = self.query_one("#form-container", Vertical)
        form.remove_children()

        # Build form rows
        with form.batch_updates():
            # Title row
            title_row = Horizontal(classes="form-row")
            title_row.compose_add_child(Static("Title:", classes="form-label"))
            title_row.compose_add_child(Input(
                value=self._thread.title if self._thread else "",
                id="title-input",
                classes="form-input",
            ))
            form.mount(title_row)

            # Author row
            author_row = Horizontal(classes="form-row")
            author_row.compose_add_child(Static("Author:", classes="form-label"))
            author_row.compose_add_child(Input(
                value=self._thread.author if self._thread else "",
                id="author-input",
                classes="form-input",
            ))
            form.mount(author_row)

            # Year row
            year_value = ""
            if self._thread and self._thread.posted_date:
                try:
                    year_value = self._thread.posted_date[:4]
                except (ValueError, IndexError):
                    pass
            year_row = Horizontal(classes="form-row")
            year_row.compose_add_child(Static("Year:", classes="form-label"))
            year_row.compose_add_child(Input(
                value=year_value,
                id="year-input",
                classes="form-input",
                max_length=4,
            ))
            form.mount(year_row)

            # Tags row
            tags_row = Horizontal(classes="form-row")
            tags_row.compose_add_child(Static("Tags:", classes="form-label"))
            tags_row.compose_add_child(Input(
                placeholder="comma,separated,tags",
                id="tags-input",
                classes="form-input",
            ))
            form.mount(tags_row)

            # Import button
            form.mount(Button("Import", id="import-btn", variant="success"))

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle URL input submission."""
        if event.input.id == "url-input":
            self._do_fetch(event.value)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle button clicks."""
        if event.button.id == "fetch-btn":
            url = self.query_one("#url-input", Input).value
            self._do_fetch(url)
        elif event.button.id == "import-btn":
            self.action_import_wad()

    def _do_fetch(self, url: str) -> None:
        """Fetch the forum thread."""
        url = url.strip()
        if not url:
            self.notify("Please enter a URL", severity="warning")
            return

        if "doomworld.com/forum" not in url:
            self.notify("URL should be a Doomworld forum thread", severity="warning")
            return

        # Cancel any existing fetch
        if self._current_worker and self._current_worker.is_running:
            self._current_worker.cancel()

        status = self.query_one("#status", Static)
        status.update(f"Fetching thread...")

        self._current_worker = self.run_worker(
            self._fetch_thread(url),
            exclusive=True,
        )

    async def _fetch_thread(self, url: str) -> ForumThread | None:
        """Fetch thread in a worker."""
        worker = get_current_worker()

        try:
            with DoomworldSource() as source:
                if worker.is_cancelled:
                    return None
                thread = source.get(url)
                return thread
        except Exception as e:
            self.notify(f"Fetch error: {e}", severity="error")
            return None

    def on_worker_state_changed(self, event: Worker.StateChanged) -> None:
        """Handle fetch completion."""
        if event.state.name == "SUCCESS":
            thread = event.worker.result
            if thread:
                self._display_thread(thread)
            else:
                status = self.query_one("#status", Static)
                status.update("Could not fetch thread - check the URL")
                self._show_placeholder("Could not fetch thread")

    def _display_thread(self, thread: ForumThread) -> None:
        """Display fetched thread data."""
        self._thread = thread

        # Update preview
        header = self.query_one("#preview-header", Static)
        header.update(f"Thread: {thread.title[:50]}..." if len(thread.title) > 50 else f"Thread: {thread.title}")

        meta = self.query_one("#preview-meta", Static)
        meta_parts = []
        if thread.author:
            meta_parts.append(f"by {thread.author}")
        if thread.posted_date:
            meta_parts.append(f"({thread.posted_date[:10]})")
        meta.update(" ".join(meta_parts))

        excerpt = self.query_one("#preview-excerpt", Static)
        text = thread.first_post_text or "No content"
        if len(text) > 800:
            text = text[:800] + "..."
        excerpt.update(text)

        # Show form with pre-filled data
        self._show_form()

        status = self.query_one("#status", Static)
        status.update("Edit fields → Ctrl+Enter Import  |  1-5 Source")

    def action_import_wad(self) -> None:
        """Import the WAD with form values."""
        if not self._thread:
            self.notify("No thread loaded - fetch a URL first", severity="warning")
            return

        try:
            title_input = self.query_one("#title-input", Input)
            author_input = self.query_one("#author-input", Input)
            year_input = self.query_one("#year-input", Input)
            tags_input = self.query_one("#tags-input", Input)
        except Exception:
            self.notify("Form not ready", severity="warning")
            return

        title = title_input.value.strip()
        author = author_input.value.strip() or None
        year_str = year_input.value.strip()
        tags_str = tags_input.value.strip()

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
            self._do_import(title, author, year, tags),
            exclusive=False,
        )

    async def _do_import(
        self,
        title: str,
        author: str | None,
        year: int | None,
        tags: list[str] | None,
    ) -> None:
        """Perform the import in a worker."""
        from caco.services import ImportService

        thread = self._thread
        if not thread:
            return

        result = ImportService().import_doomworld(
            thread, tags=tags, title=title, author=author, year=year,
        )

        status = self.query_one("#status", Static)
        if result.is_duplicate:
            self.notify(
                f"Already in library: {result.duplicate_title} (ID: {result.duplicate_id})",
                severity="warning",
            )
            status.update("WAD already exists in library")
        elif result.error:
            self.notify(f"Import failed: {result.error}", severity="error")
            status.update(f"Import failed: {result.error}")
        else:
            self.notify(f"Imported: {title} (ID: {result.wad_id})")
            status.update(f"Successfully imported as ID {result.wad_id}")

            # Clear form for next import
            self._thread = None
            self._show_placeholder("Paste another URL to import")
            self.query_one("#url-input", Input).value = ""
            self.query_one("#preview-header", Static).update("Thread Preview")
            self.query_one("#preview-meta", Static).update("")
            self.query_one("#preview-excerpt", Static).update("")

            self.post_message(self.WadImported(result.wad_id))

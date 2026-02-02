"""WAD edit screen for modifying WAD metadata."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.screen import Screen
from textual.widgets import Button, Footer, Input, Label, Select, Static, TextArea

from caco import db
from caco.db import Status


# Status options for the select dropdown
STATUS_OPTIONS = [
    ("To Play", "to-play"),
    ("Backlog", "backlog"),
    ("Playing", "playing"),
    ("Finished", "finished"),
    ("Abandoned", "abandoned"),
    ("Awaiting Update", "awaiting-update"),
]

# Rating options (1-5 stars)
RATING_OPTIONS = [
    ("Not Rated", ""),
    ("★☆☆☆☆ (1)", "1"),
    ("★★☆☆☆ (2)", "2"),
    ("★★★☆☆ (3)", "3"),
    ("★★★★☆ (4)", "4"),
    ("★★★★★ (5)", "5"),
]


class WadEditScreen(Screen):
    """Screen for editing WAD metadata."""

    BINDINGS = [
        Binding("ctrl+s", "save", "Save", show=True),
        Binding("escape", "cancel", "Cancel", show=True),
    ]

    DEFAULT_CSS = """
    WadEditScreen {
        align: center middle;
    }

    WadEditScreen #edit-container {
        width: 80%;
        max-width: 100;
        height: auto;
        max-height: 90%;
        background: $surface;
        border: thick $primary;
        padding: 1 2;
    }

    WadEditScreen #edit-header {
        text-style: bold;
        margin-bottom: 1;
        text-align: center;
    }

    WadEditScreen #edit-form {
        height: auto;
        max-height: 100%;
    }

    WadEditScreen .form-row {
        height: auto;
        margin-bottom: 1;
    }

    WadEditScreen .form-label {
        width: 16;
        color: $text-muted;
    }

    WadEditScreen .form-input {
        width: 1fr;
    }

    WadEditScreen .form-section {
        text-style: bold;
        margin-top: 1;
        margin-bottom: 1;
        border-bottom: solid $primary-darken-2;
    }

    WadEditScreen TextArea {
        height: 6;
    }

    WadEditScreen #button-row {
        height: 3;
        margin-top: 1;
        align: center middle;
    }

    WadEditScreen #save-btn {
        margin-right: 2;
    }
    """

    def __init__(self, wad_id: int) -> None:
        super().__init__()
        self.wad_id = wad_id
        self._wad: dict | None = None

    def compose(self) -> ComposeResult:
        with Vertical(id="edit-container"):
            yield Static("Edit WAD", id="edit-header")
            with VerticalScroll(id="edit-form"):
                # Basic Info Section
                yield Static("Basic Info", classes="form-section")

                with Horizontal(classes="form-row"):
                    yield Label("Title:", classes="form-label")
                    yield Input(id="input-title", classes="form-input")

                with Horizontal(classes="form-row"):
                    yield Label("Author:", classes="form-label")
                    yield Input(id="input-author", classes="form-input")

                with Horizontal(classes="form-row"):
                    yield Label("Year:", classes="form-label")
                    yield Input(
                        id="input-year",
                        classes="form-input",
                        type="integer",
                        max_length=4,
                    )

                with Horizontal(classes="form-row"):
                    yield Label("Status:", classes="form-label")
                    yield Select(
                        options=STATUS_OPTIONS,
                        id="select-status",
                        classes="form-input",
                        allow_blank=False,
                    )

                with Horizontal(classes="form-row"):
                    yield Label("Rating:", classes="form-label")
                    yield Select(
                        options=RATING_OPTIONS,
                        id="select-rating",
                        classes="form-input",
                        allow_blank=False,
                    )

                with Horizontal(classes="form-row"):
                    yield Label("Tags:", classes="form-label")
                    yield Input(
                        id="input-tags",
                        classes="form-input",
                        placeholder="Comma-separated tags",
                    )

                # Text Fields Section
                yield Static("Text Fields", classes="form-section")

                with Vertical(classes="form-row"):
                    yield Label("Notes:", classes="form-label")
                    yield TextArea(id="textarea-notes")

                with Vertical(classes="form-row"):
                    yield Label("Description:", classes="form-label")
                    yield TextArea(id="textarea-description")

                # Launch Config Section
                yield Static("Launch Config", classes="form-section")

                with Horizontal(classes="form-row"):
                    yield Label("Custom IWAD:", classes="form-label")
                    yield Input(
                        id="input-iwad",
                        classes="form-input",
                        placeholder="e.g., doom2.wad",
                    )

                with Horizontal(classes="form-row"):
                    yield Label("Sourceport:", classes="form-label")
                    yield Input(
                        id="input-sourceport",
                        classes="form-input",
                        placeholder="e.g., gzdoom, dsda-doom",
                    )

                with Horizontal(classes="form-row"):
                    yield Label("Extra Args:", classes="form-label")
                    yield Input(
                        id="input-args",
                        classes="form-input",
                        placeholder="e.g., -fast -nomonsters",
                    )

                # Buttons
                with Horizontal(id="button-row"):
                    yield Button("Save (Ctrl+S)", id="save-btn", variant="primary")
                    yield Button("Cancel (Esc)", id="cancel-btn", variant="default")

        yield Footer()

    def on_mount(self) -> None:
        """Load WAD data and populate form."""
        self._wad = db.get_wad(self.wad_id)
        if not self._wad:
            self.notify("WAD not found", severity="error")
            self.dismiss(False)
            return

        self._populate_form()
        # Focus the title input
        self.query_one("#input-title", Input).focus()

    def _populate_form(self) -> None:
        """Fill form fields with current WAD data."""
        wad = self._wad
        if not wad:
            return

        # Basic fields
        self.query_one("#input-title", Input).value = wad.get("title") or ""
        self.query_one("#input-author", Input).value = wad.get("author") or ""

        year_input = self.query_one("#input-year", Input)
        if wad.get("year"):
            year_input.value = str(wad["year"])

        # Status
        status_select = self.query_one("#select-status", Select)
        status_select.value = wad.get("status") or "backlog"

        # Rating
        rating_select = self.query_one("#select-rating", Select)
        rating = wad.get("rating")
        rating_select.value = str(rating) if rating else ""

        # Tags
        tags = wad.get("tags") or []
        self.query_one("#input-tags", Input).value = ", ".join(tags)

        # Text areas
        self.query_one("#textarea-notes", TextArea).text = wad.get("notes") or ""
        self.query_one("#textarea-description", TextArea).text = (
            wad.get("description") or ""
        )

        # Launch config
        self.query_one("#input-iwad", Input).value = wad.get("custom_iwad") or ""
        self.query_one("#input-sourceport", Input).value = (
            wad.get("custom_sourceport") or ""
        )
        self.query_one("#input-args", Input).value = wad.get("custom_args") or ""

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle button presses."""
        if event.button.id == "save-btn":
            self.action_save()
        elif event.button.id == "cancel-btn":
            self.action_cancel()

    def action_save(self) -> None:
        """Save changes to the WAD."""
        if not self._wad:
            return

        # Collect form values
        title = self.query_one("#input-title", Input).value.strip()
        author = self.query_one("#input-author", Input).value.strip() or None
        year_str = self.query_one("#input-year", Input).value.strip()
        status = self.query_one("#select-status", Select).value
        rating_str = self.query_one("#select-rating", Select).value
        tags_str = self.query_one("#input-tags", Input).value.strip()
        notes = self.query_one("#textarea-notes", TextArea).text.strip() or None
        description = (
            self.query_one("#textarea-description", TextArea).text.strip() or None
        )
        custom_iwad = self.query_one("#input-iwad", Input).value.strip() or None
        custom_sourceport = (
            self.query_one("#input-sourceport", Input).value.strip() or None
        )
        custom_args = self.query_one("#input-args", Input).value.strip() or None

        # Validate
        if not title:
            self.notify("Title is required", severity="error")
            return

        # Parse year
        year = None
        if year_str:
            try:
                year = int(year_str)
                if year < 1993 or year > 2100:
                    self.notify("Year must be between 1993 and 2100", severity="error")
                    return
            except ValueError:
                self.notify("Invalid year", severity="error")
                return

        # Parse rating
        rating = None
        if rating_str and rating_str != Select.BLANK:
            try:
                rating = int(rating_str)
            except (ValueError, TypeError):
                pass

        # Parse tags
        new_tags = []
        if tags_str:
            new_tags = [t.strip().lower() for t in tags_str.split(",") if t.strip()]

        # Update WAD
        db.update_wad(
            self.wad_id,
            title=title,
            author=author,
            year=year,
            status=status,
            rating=rating,
            notes=notes,
            description=description,
            custom_iwad=custom_iwad,
            custom_sourceport=custom_sourceport,
            custom_args=custom_args,
        )

        # Update tags (sync: remove old, add new)
        old_tags = set(self._wad.get("tags") or [])
        new_tags_set = set(new_tags)

        for tag in old_tags - new_tags_set:
            db.remove_tag(self.wad_id, tag)
        for tag in new_tags_set - old_tags:
            db.add_tag(self.wad_id, tag)

        self.notify(f"Saved changes to {title}")
        self.dismiss(True)

    def action_cancel(self) -> None:
        """Cancel editing and return."""
        self.dismiss(False)

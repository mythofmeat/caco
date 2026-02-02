"""Sort selection widget with dropdown and direction toggle."""

from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.message import Message
from textual.widgets import Button, Select


# Sort fields configuration
SORT_OPTIONS = [
    ("ID", "id"),
    ("Title", "title"),
    ("Author", "author"),
    ("Playtime", "playtime"),
    ("Last Played", "last_played"),
]


class SortSelect(Horizontal):
    """Sort selection widget with dropdown and direction toggle."""

    class SortChanged(Message):
        """Fired when sort field or direction changes."""

        def __init__(self, sort_by: str, sort_desc: bool) -> None:
            super().__init__()
            self.sort_by = sort_by
            self.sort_desc = sort_desc

    DEFAULT_CSS = """
    SortSelect {
        height: 3;
        width: auto;
        align: right middle;
    }

    SortSelect Select {
        width: 16;
    }

    SortSelect #sort-dir-btn {
        width: 3;
        min-width: 3;
        margin-left: 1;
    }
    """

    def __init__(
        self,
        sort_by: str = "id",
        sort_desc: bool = False,
        **kwargs,
    ) -> None:
        super().__init__(**kwargs)
        self._sort_by = sort_by
        self._sort_desc = sort_desc
        self._initialized = False  # Track if we've finished mounting

    def on_mount(self) -> None:
        """Mark as initialized after mount completes."""
        self._initialized = True

    def compose(self) -> ComposeResult:
        yield Select(
            options=SORT_OPTIONS,
            value=self._sort_by,
            allow_blank=False,
            id="sort-select",
        )
        yield Button(
            "↓" if self._sort_desc else "↑",
            id="sort-dir-btn",
            variant="default",
        )

    @property
    def sort_by(self) -> str:
        """Current sort field."""
        return self._sort_by

    @property
    def sort_desc(self) -> bool:
        """Whether sorting is descending."""
        return self._sort_desc

    def on_select_changed(self, event: Select.Changed) -> None:
        """Handle sort field change."""
        if event.value != Select.BLANK:
            new_value = str(event.value)
            # Only fire if value actually changed (skip initial mount)
            if new_value != self._sort_by:
                self._sort_by = new_value
                self.post_message(self.SortChanged(self._sort_by, self._sort_desc))

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle direction toggle."""
        if event.button.id == "sort-dir-btn":
            self._sort_desc = not self._sort_desc
            event.button.label = "↓" if self._sort_desc else "↑"
            self.post_message(self.SortChanged(self._sort_by, self._sort_desc))

    def set_sort(self, sort_by: str, sort_desc: bool) -> None:
        """Programmatically set sort settings (e.g., from keyboard shortcut)."""
        self._sort_by = sort_by
        self._sort_desc = sort_desc

        select = self.query_one("#sort-select", Select)
        select.value = sort_by

        btn = self.query_one("#sort-dir-btn", Button)
        btn.label = "↓" if sort_desc else "↑"

    def cycle_sort(self) -> None:
        """Cycle to next sort field (for keyboard shortcut)."""
        current_idx = next(
            (i for i, (_, val) in enumerate(SORT_OPTIONS) if val == self._sort_by),
            0,
        )
        next_idx = (current_idx + 1) % len(SORT_OPTIONS)
        self._sort_by = SORT_OPTIONS[next_idx][1]

        select = self.query_one("#sort-select", Select)
        select.value = self._sort_by

        self.post_message(self.SortChanged(self._sort_by, self._sort_desc))

    def toggle_direction(self) -> None:
        """Toggle sort direction (for keyboard shortcut)."""
        self._sort_desc = not self._sort_desc
        btn = self.query_one("#sort-dir-btn", Button)
        btn.label = "↓" if self._sort_desc else "↑"
        self.post_message(self.SortChanged(self._sort_by, self._sort_desc))

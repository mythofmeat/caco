"""WAD detail view screen."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Vertical
from textual.screen import Screen
from textual.widgets import Static, Footer

from caco import db
from caco.player import format_duration


# Status display names
STATUS_DISPLAY = {
    "to-play": "To Play",
    "backlog": "Backlog",
    "playing": "Playing",
    "finished": "Finished",
    "abandoned": "Abandoned",
    "awaiting-update": "Awaiting Update",
}


class WadDetailScreen(Screen):
    """Full WAD detail view."""

    BINDINGS = [
        Binding("q", "back", "Back", show=True),
        Binding("escape", "back", "Back", show=False),
        Binding("enter", "play_wad", "Play", show=True),
        Binding("h", "show_history", "History", show=True),
        Binding("e", "edit_wad", "Edit", show=True),
    ]

    def __init__(self, wad_id: int) -> None:
        super().__init__()
        self.wad_id = wad_id

    def compose(self) -> ComposeResult:
        yield Static("", id="detail-header")
        yield Vertical(id="detail-content")
        yield Footer()

    def on_mount(self) -> None:
        """Load WAD details."""
        self._load_details()

    def _load_details(self) -> None:
        """Load and display WAD details."""
        wad = db.get_wad(self.wad_id)
        if not wad:
            header = self.query_one("#detail-header", Static)
            header.update("[red]WAD not found[/red]")
            return

        # Header
        header = self.query_one("#detail-header", Static)
        header.update(f"[bold]{wad['title']}[/bold]")

        # Build content
        content = self.query_one("#detail-content", Vertical)
        content.remove_children()

        # Basic info section
        content.mount(Static("[bold]Basic Info[/bold]", classes="detail-section"))

        if wad.get("author"):
            content.mount(self._make_row("Author", wad["author"]))
        if wad.get("year"):
            content.mount(self._make_row("Year", str(wad["year"])))

        status = wad["status"]
        status_name = STATUS_DISPLAY.get(status, status)
        content.mount(self._make_row("Status", status_name))

        if wad.get("rating"):
            stars = "★" * wad["rating"] + "☆" * (5 - wad["rating"])
            content.mount(self._make_row("Rating", stars))

        # Source info
        content.mount(Static(""))
        content.mount(Static("[bold]Source[/bold]", classes="detail-section"))
        content.mount(self._make_row("Type", wad["source_type"]))
        if wad.get("source_url"):
            content.mount(self._make_row("URL", wad["source_url"]))
        if wad.get("filename"):
            content.mount(self._make_row("Filename", wad["filename"]))

        # Play stats
        content.mount(Static(""))
        content.mount(Static("[bold]Play Stats[/bold]", classes="detail-section"))

        playtime = db.get_total_playtime(self.wad_id)
        content.mount(self._make_row("Playtime", format_duration(playtime) if playtime else "Never played"))

        sessions = db.get_sessions(self.wad_id)
        content.mount(self._make_row("Sessions", str(len(sessions))))

        last_played = db.get_last_played(self.wad_id)
        if last_played:
            content.mount(self._make_row("Last Played", last_played[:10]))

        times_beaten = db.get_times_beaten(self.wad_id)
        content.mount(self._make_row("Times Beaten", str(times_beaten)))

        # Tags
        if wad.get("tags"):
            content.mount(Static(""))
            content.mount(Static("[bold]Tags[/bold]", classes="detail-section"))
            content.mount(Static(", ".join(wad["tags"])))

        # Custom config
        if wad.get("custom_iwad") or wad.get("custom_sourceport") or wad.get("custom_args"):
            content.mount(Static(""))
            content.mount(Static("[bold]Custom Config[/bold]", classes="detail-section"))
            if wad.get("custom_iwad"):
                content.mount(self._make_row("IWAD", wad["custom_iwad"]))
            if wad.get("custom_sourceport"):
                content.mount(self._make_row("Sourceport", wad["custom_sourceport"]))
            if wad.get("custom_args"):
                content.mount(self._make_row("Args", wad["custom_args"]))

        # Description
        if wad.get("description"):
            content.mount(Static(""))
            content.mount(Static("[bold]Description[/bold]", classes="detail-section"))
            # Truncate very long descriptions
            desc = wad["description"]
            if len(desc) > 500:
                desc = desc[:500] + "..."
            content.mount(Static(desc))

        # Notes
        if wad.get("notes"):
            content.mount(Static(""))
            content.mount(Static("[bold]Notes[/bold]", classes="detail-section"))
            content.mount(Static(wad["notes"]))

    def _make_row(self, label: str, value: str) -> Static:
        """Create a label-value row as a single Static widget."""
        # Using a single Static with inline formatting avoids mount timing issues
        return Static(f"[dim]{label}:[/dim] {value}")

    def action_back(self) -> None:
        """Go back to library."""
        self.app.pop_screen()

    def action_play_wad(self) -> None:
        """Play this WAD."""
        from caco.player import play

        wad = db.get_wad(self.wad_id)
        if not wad:
            self.notify("WAD not found", severity="error")
            return

        self.run_worker(self._play_and_refresh())

    async def _play_and_refresh(self) -> None:
        """Play WAD and refresh."""
        wad = db.get_wad(self.wad_id)
        if not wad:
            return

        from caco.player import play

        error = None
        with self.app.suspend():
            try:
                play(self.wad_id)
            except ValueError as e:
                error = str(e)

        if error:
            self.notify(error, severity="error", timeout=10)
        else:
            self.notify(f"Finished playing {wad['title']}")

        self._load_details()

    def action_show_history(self) -> None:
        """Show session history."""
        from caco.tui.screens.sessions import SessionsScreen
        self.app.push_screen(SessionsScreen(self.wad_id))

    def action_edit_wad(self) -> None:
        """Open WAD edit screen."""
        from caco.tui.screens.wad_edit import WadEditScreen

        def on_dismiss(result: bool | None) -> None:
            if result:
                self._load_details()

        self.app.push_screen(WadEditScreen(self.wad_id), on_dismiss)

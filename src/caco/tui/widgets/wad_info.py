"""WAD info panel widget."""

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.widgets import Static

from caco import db
from caco.player import format_duration


# Status display names and CSS classes
STATUS_DISPLAY = {
    "to-play": ("To Play", "status-to-play"),
    "backlog": ("Backlog", "status-backlog"),
    "playing": ("Playing", "status-playing"),
    "finished": ("Finished", "status-finished"),
    "abandoned": ("Abandoned", "status-abandoned"),
    "awaiting-update": ("Awaiting Update", "status-awaiting-update"),
}


class WadInfoPanel(Vertical):
    """Side panel showing details of selected WAD."""

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self._wad_id: int | None = None

    def compose(self) -> ComposeResult:
        yield Static("", id="info-title")
        yield Static("", id="info-author")
        yield Static("", id="info-status")
        yield Static("", id="info-details")

    def update_wad(self, wad_id: int | None) -> None:
        """Update panel with WAD info."""
        self._wad_id = wad_id

        title_widget = self.query_one("#info-title", Static)
        author_widget = self.query_one("#info-author", Static)
        status_widget = self.query_one("#info-status", Static)
        details_widget = self.query_one("#info-details", Static)

        if wad_id is None:
            title_widget.update("No WAD selected")
            author_widget.update("")
            status_widget.update("")
            details_widget.update("")
            return

        wad = db.get_wad(wad_id)
        if not wad:
            title_widget.update("WAD not found")
            author_widget.update("")
            status_widget.update("")
            details_widget.update("")
            return

        # Title
        title_widget.update(wad["title"])

        # Author and year
        author_parts = []
        if wad.get("author"):
            author_parts.append(wad["author"])
        if wad.get("year"):
            author_parts.append(f"({wad['year']})")
        author_widget.update(" ".join(author_parts) if author_parts else "Unknown author")

        # Status with color
        status = wad["status"]
        status_name, status_class = STATUS_DISPLAY.get(status, (status, ""))
        status_widget.update(f"Status: [{status_class}]{status_name}[/]" if status_class else f"Status: {status_name}")

        # Details
        playtime = db.get_total_playtime(wad_id)
        last_played = db.get_last_played(wad_id)
        map_stats = db.get_map_completion_stats(wad_id)
        times_beaten = db.get_times_beaten(wad_id)
        sessions = db.get_sessions(wad_id)

        details_lines = []

        # Rating (stars)
        if wad.get("rating"):
            rating = wad["rating"]
            stars = "★" * rating + "☆" * (5 - rating)
            details_lines.append(f"Rating: {stars}")

        # Playtime
        if playtime:
            details_lines.append(f"Playtime: {format_duration(playtime)}")

        # Sessions
        details_lines.append(f"Sessions: {len(sessions)}")

        # Maps completed
        if map_stats["unique_maps"]:
            details_lines.append(f"Maps: {map_stats['unique_maps']} completed")

        # Times beaten
        if times_beaten:
            details_lines.append(f"Beaten: {times_beaten}x")

        # Last played
        if last_played:
            details_lines.append(f"Last: {last_played[:10]}")

        # Tags
        if wad.get("tags"):
            details_lines.append(f"Tags: {', '.join(wad['tags'])}")

        # Source
        details_lines.append(f"Source: {wad['source_type']}")

        details_widget.update("\n".join(details_lines))

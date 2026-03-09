"""WAD info panel widget."""

from pathlib import Path

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.widgets import Static

from caco import db
from caco.player import format_duration
from caco.tui.theme import get_status_display, get_status_css_class
from caco.utils import format_author_year, format_rating, truncate
from caco.wad_stats import get_progress_display


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

    def update_wad(self, wad_id: int | None, stats: dict | None = None, wad: dict | None = None) -> None:
        """Update panel with WAD info.

        Args:
            wad_id: WAD ID to display, or None to clear.
            stats: Optional pre-fetched stats dict with keys:
                   playtime, last_played, times_beaten, session_count.
                   If None, falls back to individual DB queries.
            wad: Optional pre-fetched WAD dict. If None, fetches from DB.
        """
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

        if wad is None:
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
        author_widget.update(format_author_year(wad.get("author"), wad.get("year")))

        # Status with color
        status = wad["status"]
        status_name = get_status_display(status)
        status_class = get_status_css_class(status)
        status_widget.update(f"Status: [{status_class}]{status_name}[/]" if status_class else f"Status: {status_name}")

        # Use pre-fetched stats if available, otherwise fall back to individual queries
        if stats is not None:
            playtime = stats.get("playtime", 0)
            last_played = stats.get("last_played")
            times_beaten = stats.get("times_beaten", 0)
            session_count = stats.get("session_count", 0)
        else:
            playtime = db.get_total_playtime(wad_id)
            last_played = db.get_last_played(wad_id)
            times_beaten = db.get_times_beaten(wad_id)
            sessions = db.get_sessions(wad_id)
            session_count = len(sessions)

        details_lines = []

        # Rating (stars)
        if wad.get("rating"):
            details_lines.append(f"[yellow]{format_rating(wad['rating'])}[/yellow]")

        # Playtime
        if playtime:
            details_lines.append(f"Playtime: {format_duration(playtime)}")

        # Sessions
        details_lines.append(f"Sessions: {session_count}")

        # Map progress
        progress_display = get_progress_display(wad.get("stats_snapshot"), width=15)
        if progress_display:
            details_lines.append(f"Progress: [green]{progress_display}[/green]")

        # Times beaten
        if times_beaten:
            details_lines.append(f"Beaten: {times_beaten}x")

        # Last played
        if last_played:
            details_lines.append(f"Last: {last_played[:10]}")

        # Tags
        if wad.get("tags"):
            tag_chips = " ".join(f"[on dark_blue] {t} [/]" for t in wad["tags"])
            details_lines.append(f"Tags: {tag_chips}")

        # Complevel
        if wad.get("complevel") is not None:
            from caco.complevel import complevel_name
            details_lines.append(f"CL: {wad['complevel']} ({complevel_name(wad['complevel'])})")

        # Custom IWAD
        if wad.get("custom_iwad"):
            details_lines.append(f"IWAD: {wad['custom_iwad']}")

        # Config profile
        if wad.get("custom_config"):
            details_lines.append(f"Config: {wad['custom_config']}")

        # Description snippet
        if wad.get("description"):
            details_lines.append(f"\n[dim]{truncate(wad['description'], 120)}[/dim]")

        # Companion files
        companions = db.get_wad_companions(wad["id"])
        if companions:
            names = ", ".join(c["filename"] for c in companions if c["enabled"])
            if names:
                details_lines.append(f"Files: {names}")

        # Source
        details_lines.append(f"[dim]Source: {wad['source_type']}[/dim]")

        details_widget.update("\n".join(details_lines))

"""Per-map WAD statistics screen."""

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Static

from caco import db
from caco.wad_stats import (
    format_time_secs,
    format_time_tics,
    skill_name,
    stats_from_json,
)


class WadStatsScreen(Screen):
    """Per-map statistics viewer for a WAD's completion records."""

    BINDINGS = [
        Binding("q", "back", "Back", show=True),
        Binding("escape", "back", "Back", show=False),
        Binding("j", "cursor_down", "Down", show=False),
        Binding("k", "cursor_up", "Up", show=False),
        Binding("n", "next_completion", "Next", show=True),
        Binding("p", "prev_completion", "Prev", show=True),
    ]

    def __init__(self, wad_id: int) -> None:
        super().__init__()
        self.wad_id = wad_id
        self._completions: list[dict] = []
        self._current_index = 0

    def compose(self) -> ComposeResult:
        yield Static("", id="stats-header")
        yield Static("", id="stats-summary")
        yield DataTable(id="stats-table")
        yield Footer()

    def on_mount(self) -> None:
        """Load completion data."""
        wad = db.get_wad(self.wad_id)
        header = self.query_one("#stats-header", Static)

        if not wad:
            header.update("[red]WAD not found[/red]")
            return

        completions = db.get_wad_completions(self.wad_id)
        self._completions = [
            c for c in completions if c.get("stats_snapshot")
        ]

        if not self._completions:
            header.update(f"[bold]{wad['title']}[/bold]")
            summary = self.query_one("#stats-summary", Static)
            summary.update("[dim]No completions with stats found[/dim]")
            return

        header.update(f"[bold]Map Statistics: {wad['title']}[/bold]")
        self._load_completion(0)

        table = self.query_one("#stats-table", DataTable)
        table.focus()

    def _load_completion(self, index: int) -> None:
        self._current_index = index
        comp = self._completions[index]
        wad_stats = stats_from_json(comp["stats_snapshot"])
        played = wad_stats.played_maps

        # Update summary
        summary = self.query_one("#stats-summary", Static)
        date = comp["completed_at"][:16].replace("T", " ") if comp["completed_at"] else "-"
        nav = ""
        if len(self._completions) > 1:
            nav = f" [dim]({index + 1}/{len(self._completions)}, n/p to switch)[/dim]"
        summary.update(
            f"Completion #{comp['id']} ({date}) | "
            f"Format: {wad_stats.format} | "
            f"Maps: {len(played)} | "
            f"Time: {wad_stats.total_time_display}"
            f"{nav}"
        )

        # Populate table
        table = self.query_one("#stats-table", DataTable)
        table.clear(columns=True)
        table.cursor_type = "row"
        table.zebra_stripes = True

        if wad_stats.format == "stats_txt":
            self._populate_stats_txt(table, played)
        else:
            self._populate_levelstat(table, played)

    def _populate_stats_txt(self, table: DataTable, maps: list) -> None:
        table.add_column("Map", key="map", width=8)
        table.add_column("Skill", key="skill", width=6)
        table.add_column("Time", key="time", width=10)
        table.add_column("Max Time", key="max_time", width=10)
        table.add_column("NM Time", key="nm_time", width=10)
        table.add_column("Exits", key="exits", width=6)
        table.add_column("K", key="kills", width=10)
        table.add_column("I", key="items", width=10)
        table.add_column("S", key="secrets", width=8)

        for m in maps:
            k = f"{m.kills}/{m.total_kills}" if m.total_kills >= 0 else str(m.kills)
            i = f"{m.items}/{m.total_items}" if m.total_items >= 0 else str(m.items)
            s = f"{m.secrets}/{m.total_secrets}" if m.total_secrets >= 0 else str(m.secrets)

            table.add_row(
                m.lump,
                skill_name(m.best_skill),
                format_time_tics(m.best_time),
                format_time_tics(m.best_max_time),
                format_time_tics(m.best_nm_time),
                str(m.total_exits),
                k, i, s,
            )

    def _populate_levelstat(self, table: DataTable, maps: list) -> None:
        table.add_column("Map", key="map", width=8)
        table.add_column("Time", key="time", width=12)
        table.add_column("Total Time", key="total_time", width=12)
        table.add_column("K", key="kills", width=10)
        table.add_column("I", key="items", width=10)
        table.add_column("S", key="secrets", width=8)

        for m in maps:
            table.add_row(
                m.lump,
                format_time_secs(m.time_secs),
                format_time_secs(m.total_time_secs),
                f"{m.kills}/{m.total_kills}",
                f"{m.items}/{m.total_items}",
                f"{m.secrets}/{m.total_secrets}",
            )

    def action_back(self) -> None:
        self.app.pop_screen()

    def action_next_completion(self) -> None:
        if self._current_index < len(self._completions) - 1:
            self._load_completion(self._current_index + 1)

    def action_prev_completion(self) -> None:
        if self._current_index > 0:
            self._load_completion(self._current_index - 1)

    def action_cursor_down(self) -> None:
        table = self.query_one("#stats-table", DataTable)
        if table.cursor_row is not None and table.cursor_row < table.row_count - 1:
            table.move_cursor(row=table.cursor_row + 1)

    def action_cursor_up(self) -> None:
        table = self.query_one("#stats-table", DataTable)
        if table.cursor_row is not None and table.cursor_row > 0:
            table.move_cursor(row=table.cursor_row - 1)

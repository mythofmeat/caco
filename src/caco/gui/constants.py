"""Column definitions and sort fields for the GUI."""

from enum import Enum


class Column(Enum):
    """Table column definitions: (display_name, width)."""
    ID = ("ID", 50)
    TITLE = ("Title", 250)
    AUTHOR = ("Author", 150)
    YEAR = ("Year", 60)
    STATUS = ("Status", 100)
    RATING = ("Rating", 90)
    BEATEN = ("Beaten", 60)
    PLAYTIME = ("Playtime", 80)
    LAST_PLAYED = ("Last Played", 100)
    TAGS = ("Tags", 150)
    SOURCE = ("Source", 80)

    @property
    def header(self) -> str:
        return self.value[0]

    @property
    def default_width(self) -> int:
        return self.value[1]


# All available columns (for column picker)
ALL_COLUMNS = list(Column)

# Default visible columns in list view
DEFAULT_COLUMNS = [
    Column.ID,
    Column.TITLE,
    Column.AUTHOR,
    Column.STATUS,
    Column.RATING,
    Column.PLAYTIME,
    Column.LAST_PLAYED,
]

SORT_FIELDS = {
    "ID": "id",
    "Title": "title",
    "Author": "author",
    "Playtime": "playtime",
    "Last Played": "last_played",
    "Year": "year",
    "Rating": "rating",
    "Created": "created",
}

# Tab definitions: (label, query_filter)
# query_filter is None for "All", or a status query string
STATUS_TABS = [
    ("All", None),
    ("Playing", "status:playing"),
    ("To-Play", "status:to-play"),
    ("Finished", "status:finished"),
    ("Backlog", "status:backlog"),
    ("Other", "status:abandoned , status:awaiting-update"),
]

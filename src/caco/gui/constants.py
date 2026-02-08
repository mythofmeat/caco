"""Column definitions and sort fields for the GUI."""

from enum import Enum


class Column(Enum):
    """Table column definitions: (display_name, min_width, weight).

    Weight determines the proportional width of each column.
    On resize, each column gets: total_width * (weight / sum_of_weights).
    """
    ID = ("ID", 40, 4)
    TITLE = ("Title", 120, 22)
    AUTHOR = ("Author", 80, 14)
    YEAR = ("Year", 45, 5)
    STATUS = ("Status", 60, 9)
    RATING = ("Rating", 60, 8)
    BEATEN = ("Beaten", 45, 5)
    PLAYTIME = ("Playtime", 55, 7)
    LAST_PLAYED = ("Last Played", 70, 9)
    TAGS = ("Tags", 80, 13)
    SOURCE = ("Source", 50, 7)

    @property
    def header(self) -> str:
        return self.value[0]

    @property
    def min_width(self) -> int:
        return self.value[1]

    @property
    def weight(self) -> int:
        return self.value[2]

    @property
    def default_width(self) -> int:
        """Legacy: returns min_width for compatibility."""
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

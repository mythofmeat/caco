"""Centralized status colors and display configuration for the TUI."""

# Canonical status configuration: (display_name, rich_color, css_class)
STATUS_CONFIG = {
    "to-play": ("To Play", "dodger_blue1", "status-to-play"),
    "backlog": ("Backlog", "yellow", "status-backlog"),
    "playing": ("Playing", "green1", "status-playing"),
    "finished": ("Finished", "grey50", "status-finished"),
    "abandoned": ("Abandoned", "red", "status-abandoned"),
    "awaiting-update": ("Awaiting Update", "magenta", "status-awaiting-update"),
}


def get_status_display(status: str) -> str:
    """Get human-readable display name for a status."""
    cfg = STATUS_CONFIG.get(status)
    return cfg[0] if cfg else status


def get_status_color(status: str) -> str:
    """Get Rich color string for a status."""
    cfg = STATUS_CONFIG.get(status)
    return cfg[1] if cfg else ""


def get_status_css_class(status: str) -> str:
    """Get CSS class name for a status."""
    cfg = STATUS_CONFIG.get(status)
    return cfg[2] if cfg else ""

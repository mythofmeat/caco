"""Centralized status colors and display configuration for the TUI.

Derived from db.STATUS_METADATA — single source of truth for all status info.
"""

from caco.db import STATUS_METADATA

# Canonical status configuration: (display_name, rich_color, css_class)
STATUS_CONFIG = {
    status: (meta[0], meta[2], meta[3])
    for status, meta in STATUS_METADATA.items()
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

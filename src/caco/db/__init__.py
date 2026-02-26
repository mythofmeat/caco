"""SQLite database for WAD library.

This package exposes the same public API that the original db.py module did.
All symbols are re-exported here for backward compatibility — consumers can
continue to use ``from caco import db`` or ``from caco.db import Status``.
"""

# Re-export get_db_path so that patch("caco.db.get_db_path") still works in tests
from caco.config import get_db_path  # noqa: F401

# -- Models & constants --
from caco.db._models import (  # noqa: F401
    ALLOWED_UPDATE_FIELDS,
    AndGroup,
    OR_SEPARATOR,
    ParsedQuery,
    QueryTerm,
    SourceType,
    STATUS_METADATA,
    STATUS_SHORTCUTS,
    Status,
    WadRecord,
)

# -- Connection plumbing --
from caco.db._connection import get_connection  # noqa: F401

# -- Schema & migrations --
from caco.db._schema import SCHEMA, init_db  # noqa: F401

# -- Query engine --
from caco.db._query import (  # noqa: F401
    _build_query_sql,
    _build_term_sql,
    _glob_to_like,
    _is_glob_pattern,
    _parse_and_group,
    _split_or_groups,
    find_duplicate,
    normalize_status,
    parse_query,
    search_wads,
)

# -- WAD CRUD & tags --
from caco.db._wads import (  # noqa: F401
    add_tag,
    add_wad,
    delete_wad,
    get_all_tags,
    get_tag_counts,
    get_wad,
    purge_all_deleted,
    remove_tag,
    restore_wad,
    update_wad,
)

# -- IWAD registry --
from caco.db._iwads import (  # noqa: F401
    IWAD_ALIASES,
    KNOWN_IWAD_FILENAMES,
    KNOWN_IWADS,
    add_iwad,
    get_all_iwads,
    get_iwad,
    get_iwad_by_path,
    identify_iwad,
    normalize_iwad_name,
    remove_iwad,
    resolve_iwad_from_db,
)

# -- Sessions, stats, completions, cache --
from caco.db._sessions import (  # noqa: F401
    StatsSnapshot,
    add_wad_completion,
    clear_all_cached_paths,
    clear_cached_path,
    delete_wad_completion,
    end_session,
    update_wad_completion,
    get_cached_wads,
    get_completion_rate,
    get_last_played,
    get_last_played_batch,
    get_library_stats,
    get_most_recently_played,
    get_session_count_batch,
    get_sessions,
    get_stats_snapshot,
    get_times_beaten,
    get_times_beaten_batch,
    get_total_playtime,
    get_total_playtime_batch,
    get_wad_by_cached_filename,
    get_wad_completions,
    get_wad_stats,
    get_wad_stats_batch,
    get_wads_played_by_period,
    set_wad_completion_count,
    start_session,
)

__all__ = [
    # Models & constants
    "ALLOWED_UPDATE_FIELDS",
    "AndGroup",
    "OR_SEPARATOR",
    "ParsedQuery",
    "QueryTerm",
    "SourceType",
    "STATUS_METADATA",
    "STATUS_SHORTCUTS",
    "Status",
    "WadRecord",
    # Connection
    "get_connection",
    "get_db_path",
    # Schema
    "SCHEMA",
    "init_db",
    # Query engine
    "find_duplicate",
    "normalize_status",
    "parse_query",
    "search_wads",
    # WAD CRUD & tags
    "add_tag",
    "add_wad",
    "delete_wad",
    "get_all_tags",
    "get_tag_counts",
    "get_wad",
    "purge_all_deleted",
    "remove_tag",
    "restore_wad",
    "update_wad",
    # IWAD registry
    "IWAD_ALIASES",
    "KNOWN_IWAD_FILENAMES",
    "KNOWN_IWADS",
    "add_iwad",
    "get_all_iwads",
    "get_iwad",
    "get_iwad_by_path",
    "identify_iwad",
    "normalize_iwad_name",
    "remove_iwad",
    "resolve_iwad_from_db",
    # Sessions, stats, completions, cache
    "StatsSnapshot",
    "add_wad_completion",
    "clear_all_cached_paths",
    "clear_cached_path",
    "delete_wad_completion",
    "end_session",
    "get_cached_wads",
    "get_completion_rate",
    "get_last_played",
    "get_last_played_batch",
    "get_library_stats",
    "get_most_recently_played",
    "get_session_count_batch",
    "get_sessions",
    "get_stats_snapshot",
    "get_times_beaten",
    "get_times_beaten_batch",
    "get_total_playtime",
    "get_total_playtime_batch",
    "get_wad_by_cached_filename",
    "get_wad_completions",
    "get_wad_stats",
    "get_wad_stats_batch",
    "get_wads_played_by_period",
    "set_wad_completion_count",
    "start_session",
    "update_wad_completion",
]

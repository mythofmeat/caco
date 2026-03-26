pub mod connection;
pub mod id24;
pub mod iwads;
pub mod models;
pub mod query;
pub mod schema;
pub mod sessions;
pub mod wads;

// Re-export commonly used items
pub use connection::{
    attach_tags, batch_query_i64, batch_query_string, fetch_tags, fetch_tags_batch,
    open_connection, open_memory, SQLITE_MAX_VARS,
};
pub use id24::{
    add_id24, get_all_id24, get_id24, get_id24_by_path, identify_id24, remove_id24,
    remove_id24_with_paths, Id24Record, KNOWN_ID24_FILENAMES, KNOWN_ID24_WADS,
};
pub use iwads::{
    add_iwad, get_all_iwads, get_family_iwads, get_iwad, get_iwad_by_path, get_iwad_priority,
    get_iwad_variant, identify_iwad, managed_iwad_filename, normalize_iwad_name, remove_iwad,
    remove_iwad_with_paths, resolve_iwad_from_db, IwadRecord, DEFAULT_IWAD_PRIORITY,
    FAMILY_FALLBACKS, IWAD_ALIASES, KNOWN_IWAD_FILENAMES, KNOWN_IWADS,
};
pub use models::{
    AndGroup, ParsedQuery, QueryTerm, SourceType, Status, StatusMeta, WadRecord,
    ALLOWED_UPDATE_FIELDS, OR_SEPARATOR, STATUS_METADATA, STATUS_SHORTCUTS,
};
pub use query::{find_duplicate, normalize_status, parse_query, search_wads};
pub use schema::init_db;
pub use sessions::{
    add_wad_completion, clear_all_cached_paths, clear_cached_path,
    delete_wad_completion, delete_wad_completion_by_timestamp, end_session,
    find_completion_by_timestamp, get_cached_wads, get_last_played, get_last_played_batch,
    get_most_recently_played, get_session_count_batch, get_sessions, get_stats_snapshot,
    get_times_beaten, get_times_beaten_batch, get_total_playtime, get_total_playtime_batch,
    get_wad_by_cached_filename, get_wad_completions, get_wad_stats, get_wad_stats_batch,
    get_wads_played_by_period, set_wad_completion_count, start_session, update_session_demo,
    update_session_stats, update_wad_completion, ActivityPeriod, CompletionRecord, SessionRecord,
    StatsSnapshot, WadStats,
};
pub use wads::{
    add_tag, add_wad, delete_wad, get_all_tags, get_tag_counts, get_wad, purge_all_deleted,
    remove_all_tags, remove_tag, restore_wad, update_wad, FieldValue, NewWad, WadUpdate,
};

pub mod analysis;
pub mod collections;
pub mod companions;
pub mod connection;
pub mod id24;
pub mod iwads;
pub mod models;
pub mod playthroughs;
pub mod query;
pub mod schema;
pub mod sessions;
pub mod wads;

// Re-export commonly used items
pub use analysis::{get_analysis, save_analysis};
pub use collections::{
    CollectionRecord, create_collection, delete_collection, get_all_collections, get_collection,
    run_collection, update_collection,
};
pub use companions::{
    CompanionRecord, WadCompanionRecord, add_companion, find_companion_by_md5, get_all_companions,
    get_companions_batch, get_companions_for_wad, get_orphaned_companions, is_orphan,
    link_companion_to_wad, remove_companion, remove_companion_with_path, set_companion_enabled,
    unlink_companion_from_wad, would_be_orphan,
};
pub use connection::{
    SQLITE_MAX_VARS, attach_tags, batch_query_i64, batch_query_string, fetch_tags,
    fetch_tags_batch, open_connection, open_memory, with_transaction,
};
pub use id24::{
    Id24Record, KNOWN_ID24_FILENAMES, KNOWN_ID24_WADS, add_id24, get_all_id24, get_id24,
    get_id24_by_path, identify_id24, remove_id24, remove_id24_with_paths,
};
pub use iwads::{
    DEFAULT_IWAD_PRIORITY, FAMILY_FALLBACKS, IWAD_ALIASES, IwadRecord, KNOWN_IWAD_FILENAMES,
    KNOWN_IWADS, add_iwad, get_all_iwads, get_family_iwads, get_iwad, get_iwad_by_path,
    get_iwad_priority, get_iwad_variant, identify_iwad, managed_iwad_filename, normalize_iwad_name,
    remove_iwad, remove_iwad_with_paths, resolve_iwad_from_db,
};
pub use models::{
    ALLOWED_UPDATE_FIELDS, AndGroup, Availability, OR_SEPARATOR, ParsedQuery, QueryTerm,
    STATUS_METADATA, STATUS_SHORTCUTS, SourceType, Status, StatusMeta, WadRecord,
};
pub use playthroughs::{
    PlaythroughRecord, complete_playthrough, delete_playthrough, derive_status, ensure_playthrough,
    get_active_playthrough, get_playthrough, get_playthroughs, get_times_completed,
    get_times_completed_batch, start_playthrough,
};
pub use query::{compose_and, find_duplicate, normalize_status, parse_query, search_wads};
pub use schema::init_db;
pub use sessions::{
    ActivityPeriod, CompletionRecord, SessionRecord, StatsSnapshot, WadStats, add_wad_completion,
    clear_all_cached_paths, clear_cached_path, delete_wad_completion,
    delete_wad_completion_by_timestamp, end_session, find_completion_by_timestamp, get_cached_wads,
    get_last_played, get_last_played_batch, get_most_recently_played, get_session_count_batch,
    get_sessions, get_stats_snapshot, get_times_beaten, get_times_beaten_batch, get_total_playtime,
    get_total_playtime_batch, get_wad_by_cached_filename, get_wad_completions, get_wad_stats,
    get_wad_stats_batch, get_wads_played_by_period, set_wad_completion_count, start_session,
    update_session_demo, update_session_stats, update_wad_completion,
};
pub use wads::{
    FieldValue, NewWad, WadUpdate, add_tag, add_wad, delete_wad, get_all_tags, get_status_counts,
    get_tag_counts, get_wad, purge_all_deleted, remove_all_tags, remove_tag, restore_wad,
    update_wad,
};

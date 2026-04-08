//! `caco import` — unified import from idgames, Doomwiki, Doomworld, URL, or local file.

use std::path::Path;

use clap::Args;
use rusqlite::Connection;

use caco_core::db;
use caco_core::resource_service;
use caco_sources::idgames::IdgamesClient;
use caco_sources::doomwiki::DoomwikiClient;
use caco_sources::doomworld::DoomworldClient;
use caco_sources::doomworld::llm;
use caco_sources::import_service::{ImportResult, ImportService};
use caco_sources::json_import::{self, JsonSource};
use crate::picker;

#[derive(Args)]
pub struct ImportArgs {
    /// Source query, ID, URL, or path
    source: Vec<String>,

    /// Force idgames search/ID
    #[arg(long)]
    idgames: bool,

    /// Force Doom Wiki search
    #[arg(long)]
    doomwiki: bool,

    /// Force Doomworld forum URL
    #[arg(long)]
    doomworld: bool,

    /// Force local file
    #[arg(long)]
    local: bool,

    /// Force URL import (SOURCE becomes title)
    #[arg(long)]
    url: Option<String>,

    // Metadata overrides
    /// Title override
    #[arg(short = 't', long)]
    title: Option<String>,

    /// Author
    #[arg(short = 'a', long)]
    author: Option<String>,

    /// Year
    #[arg(long)]
    year: Option<i32>,

    /// Add tags (repeatable)
    #[arg(long)]
    tag: Vec<String>,

    /// Description (--url only)
    #[arg(short = 'd', long)]
    description: Option<String>,

    /// Force import even if duplicate
    #[arg(short = 'f', long)]
    force: bool,

    /// Multi-select from search results (requires fzf)
    #[arg(short = 'm', long)]
    multi: bool,

    // Doomworld LLM
    /// LLM-powered metadata extraction (Doomworld only)
    #[arg(long, conflicts_with = "no_smart")]
    smart: bool,

    /// Disable auto-LLM extraction (when [llm] is configured)
    #[arg(long)]
    no_smart: bool,

    /// LLM backend
    #[arg(long)]
    llm_backend: Option<String>,

    /// LLM model override
    #[arg(long)]
    llm_model: Option<String>,
}

/// Print the outcome of a batch import. Returns true if the WAD was imported.
fn print_import_result(result: &ImportResult, display_name: &str, suffix: &str) -> bool {
    if result.is_duplicate {
        eprintln!(
            "Duplicate: '{}' already exists as ID {}.",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
        false
    } else if let Some(wad_id) = result.wad_id {
        println!("Imported '{display_name}' (ID: {wad_id}){suffix}");
        true
    } else if let Some(ref err) = result.error {
        eprintln!("Error importing '{display_name}': {err}");
        false
    } else {
        false
    }
}

pub fn run(conn: &Connection, args: &ImportArgs) -> Result<(), String> {
    let source_str = args.source.join(" ");

    // Determine source type
    let source_type = detect_source(args, &source_str)?;

    let tags = if args.tag.is_empty() {
        None
    } else {
        Some(args.tag.iter().map(|t| t.to_lowercase()).collect::<Vec<_>>())
    };

    match source_type {
        SourceKind::IdgamesSearch(query) => import_idgames_search(conn, &query, tags, args),
        SourceKind::IdgamesId(id) => import_idgames_id(conn, id, tags, args.force),
        SourceKind::Doomwiki(query) => import_doomwiki_search(conn, &query, tags, args),
        SourceKind::Doomworld(url) => import_doomworld(conn, &url, tags, args),
        SourceKind::Url(url) => import_url(conn, &url, &source_str, tags, args),
        SourceKind::Local(path) => import_local(conn, &path, tags, args),
        SourceKind::JsonFile(path, hint) => import_json(conn, &path, hint, tags, args),
    }
}

enum SourceKind {
    IdgamesSearch(String),
    IdgamesId(i64),
    Doomwiki(String),
    Doomworld(String),
    Url(String),
    Local(String),
    /// JSON file import with optional source hint (idgames/doomwiki).
    JsonFile(String, Option<JsonSource>),
}

fn detect_source(args: &ImportArgs, source_str: &str) -> Result<SourceKind, String> {
    let is_json_file = source_str.ends_with(".json") && Path::new(source_str).exists();

    // Explicit flags — if a .json file is given with --idgames or --doomwiki,
    // route to JSON import with the appropriate source hint.
    if args.idgames {
        if is_json_file {
            return Ok(SourceKind::JsonFile(source_str.to_string(), Some(JsonSource::Idgames)));
        }
        if let Ok(id) = source_str.parse::<i64>() {
            return Ok(SourceKind::IdgamesId(id));
        }
        return Ok(SourceKind::IdgamesSearch(source_str.to_string()));
    }
    if args.doomwiki {
        if is_json_file {
            return Ok(SourceKind::JsonFile(source_str.to_string(), Some(JsonSource::Doomwiki)));
        }
        return Ok(SourceKind::Doomwiki(source_str.to_string()));
    }
    if args.doomworld {
        return Ok(SourceKind::Doomworld(source_str.to_string()));
    }
    if args.local {
        return Ok(SourceKind::Local(source_str.to_string()));
    }
    if let Some(ref url) = args.url {
        return Ok(SourceKind::Url(url.clone()));
    }

    if source_str.is_empty() {
        return Err("No source specified.".to_string());
    }

    // Auto-detect: .json files get routed to JSON import (auto-detect source)
    if is_json_file {
        return Ok(SourceKind::JsonFile(source_str.to_string(), None));
    }

    if source_str.contains("doomwiki.org") {
        return Ok(SourceKind::Doomwiki(source_str.to_string()));
    }
    if source_str.contains("doomworld.com") {
        return Ok(SourceKind::Doomworld(source_str.to_string()));
    }
    if source_str.starts_with("http://") || source_str.starts_with("https://") {
        return Ok(SourceKind::Url(source_str.to_string()));
    }
    if Path::new(source_str).exists() {
        return Ok(SourceKind::Local(source_str.to_string()));
    }
    if let Ok(id) = source_str.parse::<i64>() {
        return Ok(SourceKind::IdgamesId(id));
    }

    // Default to idgames search
    Ok(SourceKind::IdgamesSearch(source_str.to_string()))
}

fn import_idgames_search(
    conn: &Connection,
    query: &str,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let client = IdgamesClient::new();
    let results = match client.search(query, None, None, None) {
        Ok(r) => r,
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            print_api_hint("idgames", query);
            return Err("idgames API blocked by Cloudflare challenge.".to_string());
        }
        Err(e) => return Err(e.to_string()),
    };

    if results.is_empty() {
        return Err(format!("No idgames results for '{query}'."));
    }

    // Convert to WadRecords for picker display
    let wad_records: Vec<db::WadRecord> = results
        .iter()
        .enumerate()
        .map(|(i, entry)| picker_wad_record(
            i,
            entry.title.clone(),
            Some(entry.author.clone()),
            caco_core::utils::extract_year(&entry.date),
            if entry.description.is_empty() { None } else { Some(entry.description.clone()) },
            "idgames",
            Some(entry.id.to_string()),
            None,
            Some(entry.filename.clone()),
        ))
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService;
    let mut imported = 0;
    for idx in &selected {
        let entry = &results[*idx];
        let result = svc.import_idgames(conn, entry, tags.clone(), args.force);
        if print_import_result(&result, &entry.title, "") {
            imported += 1;
        }
    }

    if imported > 0 && selected.len() > 1 {
        println!("Imported {imported} WAD(s).");
    }
    Ok(())
}

fn import_idgames_id(
    conn: &Connection,
    id: i64,
    tags: Option<Vec<String>>,
    force: bool,
) -> Result<(), String> {
    let client = IdgamesClient::new();
    let entry = match client.get(Some(id), None) {
        Ok(e) => e,
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            print_api_hint("idgames", &id.to_string());
            return Err("idgames API blocked by Cloudflare challenge.".to_string());
        }
        Err(e) => return Err(e.to_string()),
    };

    let svc = ImportService;
    let result = svc.import_idgames(conn, &entry, tags, force);

    if result.is_duplicate {
        println!(
            "Already in library: '{}' (ID: {})",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
    } else if let Some(wad_id) = result.wad_id {
        println!("Imported '{}' (ID: {wad_id})", entry.title);
    } else if let Some(ref err) = result.error {
        return Err(format!("Import error: {err}"));
    }
    Ok(())
}

fn import_doomwiki_search(
    conn: &Connection,
    query: &str,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let client = DoomwikiClient::new();
    let results = match client.search_wads(query, 20) {
        Ok(r) => r,
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            print_api_hint("doomwiki", query);
            return Err("Doom Wiki blocked the request (WAF challenge).".to_string());
        }
        Err(e) => return Err(e.to_string()),
    };

    if results.is_empty() {
        return Err(format!("No Doom Wiki results for '{query}'."));
    }

    // Convert to WadRecords for picker
    let wad_records: Vec<db::WadRecord> = results
        .iter()
        .enumerate()
        .map(|(i, entry)| picker_wad_record(
            i,
            entry.display_name().to_string(),
            if entry.author.is_empty() { None } else { Some(entry.author.clone()) },
            entry.year,
            if entry.description.is_empty() { None } else { Some(entry.description.clone()) },
            "doomwiki",
            Some(entry.page_id.to_string()),
            Some(entry.wiki_url.clone()),
            None,
        ))
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService;
    let mut imported = 0;
    for idx in &selected {
        let entry = &results[*idx];
        let result = svc.import_doomwiki(conn, entry, tags.clone(), args.force);
        if print_import_result(&result, entry.display_name(), "") {
            imported += 1;
        }
    }

    if imported > 0 && selected.len() > 1 {
        println!("Imported {imported} WAD(s).");
    }
    Ok(())
}

fn import_url(
    conn: &Connection,
    url: &str,
    source_str: &str,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let title = args
        .title
        .as_deref()
        .unwrap_or(source_str);

    let svc = ImportService;
    let result = svc.import_url(
        conn,
        title,
        url,
        args.author.as_deref(),
        args.year,
        args.description.as_deref(),
        tags,
        args.force,
    );

    if result.is_duplicate {
        println!(
            "Already in library: '{}' (ID: {})",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
    } else if let Some(wad_id) = result.wad_id {
        println!("Imported '{title}' (ID: {wad_id})");
    } else if let Some(ref err) = result.error {
        return Err(format!("Import error: {err}"));
    }
    Ok(())
}

fn import_doomworld(
    conn: &Connection,
    url: &str,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let client = DoomworldClient::new();
    let thread = client.get_thread(url).map_err(|e| e.to_string())?;

    // Determine if LLM extraction should run
    let llm_metadata = resolve_llm_metadata(&thread, args);

    // Merge: CLI flags > LLM extraction > regex extraction (thread fields)
    let final_title = args.title.as_deref()
        .or(llm_metadata.as_ref().and_then(|m| m.title.as_deref()));
    let final_author = args.author.as_deref()
        .or(llm_metadata.as_ref().and_then(|m| m.author.as_deref()));
    let final_year = args.year;
    let final_version = llm_metadata.as_ref().and_then(|m| m.version.as_deref());
    let final_complevel = llm_metadata.as_ref().and_then(|m| m.complevel);

    let svc = ImportService;
    let result = svc.import_doomworld(
        conn,
        &thread,
        tags,
        final_title,
        final_author,
        final_year,
        final_version,
        final_complevel,
        args.force,
    );

    if result.is_duplicate {
        println!(
            "Already in library: '{}' (ID: {})",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
    } else if let Some(wad_id) = result.wad_id {
        println!("Imported '{}' (ID: {wad_id})", final_title.unwrap_or(&thread.title));

        // Show extracted metadata (prefer LLM values, fall back to regex)
        let iwad = llm_metadata.as_ref().and_then(|m| m.iwad.as_deref())
            .or(thread.iwad.as_deref());
        let port = llm_metadata.as_ref().and_then(|m| m.sourceport.as_deref())
            .or(thread.sourceport.as_deref());
        let cl = final_complevel.or(thread.complevel);

        if let Some(iwad) = iwad {
            println!("  IWAD: {iwad}");
        }
        if let Some(port) = port {
            println!("  Sourceport: {port}");
        }
        if let Some(cl) = cl {
            println!("  Complevel: {cl}");
        }
        if !thread.download_links.is_empty() {
            println!("  Download links: {}", thread.download_links.len());
        }
        if let Some(ref meta) = llm_metadata {
            if let Some(ref desc) = meta.description {
                println!("  Description: {desc}");
            }
            if !meta.themes.is_empty() {
                println!("  Themes: {}", meta.themes.join(", "));
            }
        }
    } else if let Some(ref err) = result.error {
        return Err(format!("Import error: {err}"));
    }
    Ok(())
}

/// Determine if LLM extraction should run and execute it.
///
/// Logic:
/// - `--no-smart`: skip LLM entirely
/// - `--smart`: explicitly enable (error if no backend available)
/// - Neither flag: auto-enable if config has `[llm]` backend configured
///
/// On LLM error: warn and return None (never fail the import).
fn resolve_llm_metadata(
    thread: &caco_sources::doomworld::ForumThread,
    args: &ImportArgs,
) -> Option<llm::LlmExtractedMetadata> {
    if args.no_smart {
        return None;
    }

    let cfg = caco_core::config::load_config();
    let should_use_llm = if args.smart {
        true
    } else {
        // Auto-enable if config has LLM backend configured
        cfg.llm.is_configured()
    };

    if !should_use_llm {
        return None;
    }

    // Resolve backend/model/api_key: CLI flags > config values > auto-detect
    let backend = args.llm_backend.as_deref()
        .or_else(|| Some(cfg.llm.backend.as_str()).filter(|s| !s.is_empty()));
    let model = args.llm_model.as_deref()
        .or_else(|| Some(cfg.llm.model.as_str()).filter(|s| !s.is_empty()));
    let api_key = Some(cfg.llm.api_key.as_str()).filter(|s| !s.is_empty());

    let parser = match llm::get_parser(backend, model, api_key) {
        Ok(p) => p,
        Err(e) => {
            if args.smart {
                // Explicit --smart: show error prominently
                eprintln!("Warning: {e}");
            }
            return None;
        }
    };

    eprintln!("Using LLM backend: {}", parser.name());

    match parser.parse(&thread.first_post_text) {
        Ok(meta) => Some(meta),
        Err(e) => {
            eprintln!("Warning: LLM extraction failed: {e}");
            eprintln!("Falling back to regex-only extraction.");
            None
        }
    }
}

fn import_json(
    conn: &Connection,
    path_str: &str,
    source_hint: Option<JsonSource>,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let path = Path::new(path_str);

    // Determine source type: explicit hint > auto-detect
    let source = match source_hint {
        Some(s) => s,
        None => json_import::detect_json_source(path)
            .ok_or("Unrecognized JSON format. Expected idgames or Doom Wiki API response.\nHint: use --idgames or --doomwiki to specify the source.")?,
    };

    match source {
        JsonSource::Idgames => import_json_idgames(conn, path, tags, args),
        JsonSource::Doomwiki => import_json_doomwiki(conn, path, tags, args),
    }
}

fn import_json_idgames(
    conn: &Connection,
    path: &Path,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let entries = json_import::parse_idgames_json(path).map_err(|e| e.to_string())?;
    if entries.is_empty() {
        return Err("No file entries found in JSON.".to_string());
    }

    // Convert to WadRecords for picker display
    let wad_records: Vec<db::WadRecord> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| picker_wad_record(
            i,
            entry.title.clone(),
            Some(entry.author.clone()),
            caco_core::utils::extract_year(&entry.date),
            if entry.description.is_empty() { None } else { Some(entry.description.clone()) },
            "idgames",
            Some(entry.id.to_string()),
            None,
            Some(entry.filename.clone()),
        ))
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService;
    let mut imported = 0;
    for idx in &selected {
        let entry = &entries[*idx];
        let result = svc.import_idgames(conn, entry, tags.clone(), args.force);
        if print_import_result(&result, &entry.title, " [from JSON]") {
            imported += 1;
        }
    }

    if imported > 0 && selected.len() > 1 {
        println!("Imported {imported} WAD(s).");
    }
    Ok(())
}

fn import_json_doomwiki(
    conn: &Connection,
    path: &Path,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let entries = json_import::parse_doomwiki_json(path).map_err(|e| e.to_string())?;
    if entries.is_empty() {
        return Err("No WAD pages found in JSON (only pages with {{Wad}} infobox are imported).".to_string());
    }

    // Convert to WadRecords for picker
    let wad_records: Vec<db::WadRecord> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| picker_wad_record(
            i,
            entry.display_name().to_string(),
            if entry.author.is_empty() { None } else { Some(entry.author.clone()) },
            entry.year,
            if entry.description.is_empty() { None } else { Some(entry.description.clone()) },
            "doomwiki",
            Some(entry.page_id.to_string()),
            Some(entry.wiki_url.clone()),
            None,
        ))
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService;
    let mut imported = 0;
    for idx in &selected {
        let entry = &entries[*idx];
        let result = svc.import_doomwiki(conn, entry, tags.clone(), args.force);
        if print_import_result(&result, entry.display_name(), " [from JSON]") {
            imported += 1;
        }
    }

    if imported > 0 && selected.len() > 1 {
        println!("Imported {imported} WAD(s).");
    }
    Ok(())
}

fn import_local(
    conn: &Connection,
    path_str: &str,
    tags: Option<Vec<String>>,
    args: &ImportArgs,
) -> Result<(), String> {
    let path = Path::new(path_str);
    if !path.exists() {
        return Err(format!("File not found: {path_str}"));
    }

    // Check for IWAD first
    let resolved = path.canonicalize().map_err(|e| format!("Cannot resolve path: {e}"))?;
    if let Ok(Some((family, variant, title))) = try_register_iwad(conn, &resolved) {
        println!("Registered IWAD: {title} ({family}/{variant})");
        return Ok(());
    }

    // Check for id24
    if let Ok(Some((name, _version, title))) = try_register_id24(conn, &resolved) {
        println!("Registered id24 WAD: {title} ({name})");
        return Ok(());
    }

    // Regular local import
    let title = args
        .title
        .as_deref()
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
        })
        .unwrap_or("Unknown WAD");

    let svc = ImportService;
    let result = svc.import_local(
        conn,
        title,
        &resolved,
        args.author.as_deref(),
        args.year,
        args.description.as_deref(),
        tags,
        args.force,
    );

    if result.is_duplicate {
        println!(
            "Already in library: '{}' (ID: {})",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
    } else if let Some(wad_id) = result.wad_id {
        println!("Imported '{title}' (ID: {wad_id})");
    } else if let Some(ref err) = result.error {
        return Err(format!("Import error: {err}"));
    }
    Ok(())
}

/// Print a hint about downloading JSON manually when an API is blocked.
fn print_api_hint(source_type: &str, query: &str) {
    match source_type {
        "idgames" => {
            let url = json_import::idgames_api_url(query);
            eprintln!();
            eprintln!("Workaround: open this URL in your browser and save the JSON:");
            eprintln!("  {url}");
            eprintln!("Then import from the saved file:");
            eprintln!("  caco import saved.json --idgames");
        }
        "doomwiki" => {
            let url = json_import::doomwiki_api_url(query);
            eprintln!();
            eprintln!("Workaround: open this URL in your browser and save the JSON:");
            eprintln!("  {url}");
            eprintln!("Then import from the saved file:");
            eprintln!("  caco import saved.json --doomwiki");
        }
        _ => {}
    }
}

fn try_register_iwad(
    conn: &Connection,
    path: &Path,
) -> Result<Option<(String, String, String)>, String> {
    match resource_service::register_iwad(conn, path) {
        Ok(Some((family, variant, title))) => Ok(Some((family, variant, title))),
        Ok(None) => Ok(None),
        Err(_) => Ok(None),
    }
}

fn try_register_id24(
    conn: &Connection,
    path: &Path,
) -> Result<Option<(String, String, String)>, String> {
    match resource_service::register_id24(conn, path) {
        Ok(Some((name, version, title))) => Ok(Some((name, version, title))),
        Ok(None) => Ok(None),
        Err(_) => Ok(None),
    }
}

/// Build a display-only WadRecord for the picker. Not persisted to DB.
#[allow(clippy::too_many_arguments)]
fn picker_wad_record(
    index: usize,
    title: String,
    author: Option<String>,
    year: Option<i32>,
    description: Option<String>,
    source_type: &str,
    source_id: Option<String>,
    source_url: Option<String>,
    filename: Option<String>,
) -> db::WadRecord {
    db::WadRecord {
        id: index as i64 + 1,
        title,
        author,
        year,
        status: "unplayed".to_string(),
        source_type: source_type.to_string(),
        description,
        availability: "unavailable".to_string(),
        rating: None,
        notes: None,
        source_id,
        source_url,
        idgames_id: None,
        filename,
        cached_path: None,
        custom_iwad: None,
        custom_sourceport: None,
        custom_args: None,
        companion_files: None,
        custom_config: None,
        version: None,
        complevel: None,
        zdoom_required: None,
        stats_snapshot: None,
        gc_ignore: false,
        deleted_at: None,
        created_at: String::new(),
        updated_at: String::new(),
        tags: Vec::new(),
    }
}

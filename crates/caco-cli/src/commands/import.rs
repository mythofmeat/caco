//! `caco import` — unified import from idgames, Doomwiki, Doomworld, URL, or local file.

use std::path::Path;

use clap::Args;
use rusqlite::Connection;

use crate::picker;
use caco_core::db;
use caco_core::resource_service;
use caco_sources::doomwiki::DoomwikiClient;
use caco_sources::doomworld::DoomworldClient;
use caco_sources::idgames::IdgamesClient;
use caco_sources::import_service::{ImportResult, ImportService};
use caco_sources::json_import::{self, JsonSource};

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

    /// Import the WAD referenced by a Cacoward entry ID (e.g.
    /// `c.2023.winner.10` from `caco ls cacoward:...`). Pulls from the
    /// entry's idgames URL when available, otherwise its Doom Wiki page.
    #[arg(long, value_name = "ID")]
    cacoward: Option<String>,
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
    // --cacoward short-circuits everything else: it resolves a cacoward ID
    // to its idgames or doomwiki URL and re-uses the existing import
    // routines under the hood.
    if let Some(ref id) = args.cacoward {
        if !args.source.is_empty() {
            return Err(
                "--cacoward consumes the entry id directly; don't also pass a source argument."
                    .to_string(),
            );
        }
        let tags = if args.tag.is_empty() {
            None
        } else {
            Some(
                args.tag
                    .iter()
                    .map(|t| t.to_lowercase())
                    .collect::<Vec<_>>(),
            )
        };
        return import_cacoward(conn, id, tags, args.force);
    }

    let source_str = args.source.join(" ");

    // Determine source type
    let source_type = detect_source(args, &source_str)?;

    let tags = if args.tag.is_empty() {
        None
    } else {
        Some(
            args.tag
                .iter()
                .map(|t| t.to_lowercase())
                .collect::<Vec<_>>(),
        )
    };

    match source_type {
        SourceKind::IdgamesSearch(query) => import_idgames_search(conn, &query, tags, args),
        SourceKind::IdgamesId(id) => import_idgames_id(conn, id, tags, args.force),
        SourceKind::IdgamesPath(path) => import_idgames_path(conn, &path, tags, args.force),
        SourceKind::Doomwiki(query) => import_doomwiki_search(conn, &query, tags, args),
        SourceKind::DoomwikiPage(title) => import_doomwiki_page(conn, &title, tags, args.force),
        SourceKind::Doomworld(url) => import_doomworld(conn, &url, tags, args),
        SourceKind::Url(url) => import_url(conn, &url, &source_str, tags, args),
        SourceKind::Local(path) => import_local(conn, &path, tags, args),
        SourceKind::JsonFile(path, hint) => import_json(conn, &path, hint, tags, args),
    }
}

enum SourceKind {
    IdgamesSearch(String),
    IdgamesId(i64),
    /// Archive-path lookup (e.g. `levels/doom2/Ports/v-z/witchinghour`) —
    /// resolved against the idgames API's `file=` parameter.
    IdgamesPath(String),
    Doomwiki(String),
    /// Direct Doom Wiki page fetch by title (no picker).
    DoomwikiPage(String),
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
            return Ok(SourceKind::JsonFile(
                source_str.to_string(),
                Some(JsonSource::Idgames),
            ));
        }
        if let Ok(id) = source_str.parse::<i64>() {
            return Ok(SourceKind::IdgamesId(id));
        }
        if source_str.contains("doomworld.com/idgames") {
            return resolve_idgames_url(source_str);
        }
        return Ok(SourceKind::IdgamesSearch(source_str.to_string()));
    }
    if args.doomwiki {
        if is_json_file {
            return Ok(SourceKind::JsonFile(
                source_str.to_string(),
                Some(JsonSource::Doomwiki),
            ));
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
        if let Some(title) = caco_sources::doomwiki::extract_doomwiki_title_from_url(source_str) {
            return Ok(SourceKind::DoomwikiPage(title));
        }
        return Ok(SourceKind::Doomwiki(source_str.to_string()));
    }
    // idgames URLs are hosted under doomworld.com/idgames/... — route them
    // to idgames before the generic doomworld forum match so they don't
    // get rejected as invalid forum URLs.
    if source_str.contains("doomworld.com/idgames") {
        return resolve_idgames_url(source_str);
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

/// Classify a `doomworld.com/idgames/...` URL as either an id lookup
/// (`?id=N`) or an archive-path lookup (`/idgames/<path>`).
fn resolve_idgames_url(url: &str) -> Result<SourceKind, String> {
    if let Some(id) = caco_sources::idgames::extract_idgames_id_from_url(url) {
        return Ok(SourceKind::IdgamesId(id));
    }
    if let Some(path) = caco_sources::idgames::extract_idgames_file_path_from_url(url) {
        return Ok(SourceKind::IdgamesPath(path));
    }
    Err(format!("Unrecognized idgames URL: {url}"))
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
        .map(|(i, entry)| {
            picker_wad_record(
                i,
                entry.title.clone(),
                Some(entry.author.clone()),
                caco_core::utils::extract_year(&entry.date),
                if entry.description.is_empty() {
                    None
                } else {
                    Some(entry.description.clone())
                },
                db::SourceType::Idgames,
                Some(entry.id.to_string()),
                None,
                Some(entry.filename.clone()),
            )
        })
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService::new();
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

    let svc = ImportService::new();
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

fn import_idgames_path(
    conn: &Connection,
    file_path: &str,
    tags: Option<Vec<String>>,
    force: bool,
) -> Result<(), String> {
    let client = IdgamesClient::new();
    let entry = match client.get_by_path(file_path) {
        Ok(e) => e,
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            print_api_hint("idgames", file_path);
            return Err("idgames API blocked by Cloudflare challenge.".to_string());
        }
        Err(e) => return Err(format!("idgames lookup failed for '{file_path}': {e}")),
    };

    let svc = ImportService::new();
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
        .map(|(i, entry)| {
            picker_wad_record(
                i,
                entry.display_name().to_string(),
                if entry.author.is_empty() {
                    None
                } else {
                    Some(entry.author.clone())
                },
                entry.year,
                if entry.description.is_empty() {
                    None
                } else {
                    Some(entry.description.clone())
                },
                db::SourceType::Doomwiki,
                Some(entry.page_id.to_string()),
                Some(entry.wiki_url.clone()),
                None,
            )
        })
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService::new();
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

/// Import the WAD referenced by a Cacoward entry ID. Prefers the entry's
/// idgames link (more reliable metadata + a real download URL); falls back
/// to the Doom Wiki page when there's no idgames link. After a successful
/// import we run `db::link_wad` so the cacoward row points at the new wad
/// without waiting for the next `enrich --cacowards` cycle.
fn import_cacoward(
    conn: &Connection,
    id_str: &str,
    tags: Option<Vec<String>>,
    force: bool,
) -> Result<(), String> {
    let id_ref = db::parse_cacoward_id(id_str).ok_or_else(|| {
        format!(
            "invalid cacoward id '{id_str}' — expected `c.YEAR.CATEGORY.RANK` (e.g. \
             c.2023.winner.10) or `c.<pk>`"
        )
    })?;
    let record = db::resolve_cacoward_ref(conn, &id_ref)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no cacoward entry matches '{id_str}'"))?;

    // Already linked? Refuse unless --force, otherwise re-running this
    // command would silently duplicate-import.
    if let Some(wad_id) = record.wad_id
        && !force
    {
        return Err(format!(
            "{} is already linked to library WAD #{wad_id} \
             (run `caco play {wad_id}` to play, or --force to re-import)",
            record.wad_title,
        ));
    }

    let svc = ImportService::new();

    // Path 1: idgames URL → numeric id → existing import_idgames.
    if let Some(ref url) = record.idgames_url
        && let Some(idgames_id) = caco_sources::idgames::extract_idgames_id_from_url(url)
    {
        let client = IdgamesClient::new();
        let entry = match client.get(Some(idgames_id), None) {
            Ok(e) => e,
            Err(caco_sources::SourceError::WafBlocked { .. }) => {
                print_api_hint("idgames", &idgames_id.to_string());
                return Err("idgames API blocked by Cloudflare challenge.".to_string());
            }
            Err(e) => return Err(e.to_string()),
        };
        let result = svc.import_idgames(conn, &entry, tags, force);
        return finish_cacoward_import(conn, &record, &result, &entry.title);
    }

    // Path 2: Doom Wiki page. Always present (the scraper builds it from the
    // wikilink), so this is the universal fallback for non-/idgames entries.
    let Some(ref wiki_url) = record.doomwiki_url else {
        return Err(format!(
            "{} has neither an idgames link nor a Doom Wiki URL — import manually",
            record.wad_title,
        ));
    };
    let title = caco_sources::doomwiki::extract_doomwiki_title_from_url(wiki_url)
        .ok_or_else(|| format!("could not parse a wiki title from {wiki_url}"))?;
    let client = DoomwikiClient::new();
    let (entry, has_infobox) = match client.get_entry_permissive(&title) {
        Ok(Some(pair)) => pair,
        Ok(None) => return Err(format!("Doom Wiki page not found: '{title}'")),
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            print_api_hint("doomwiki", &title);
            return Err("Doom Wiki blocked the request (WAF challenge).".to_string());
        }
        Err(e) => return Err(e.to_string()),
    };
    if !has_infobox && !force {
        return Err(format!(
            "Doom Wiki page '{title}' has no {{{{Wad}}}} infobox — looks like an \
             IWAD or disambig page. Retry with --force to import anyway."
        ));
    }
    let result = svc.import_doomwiki(conn, &entry, tags, force);
    finish_cacoward_import(conn, &record, &result, entry.display_name())
}

/// Common tail for `import_cacoward`: print the outcome and, on success,
/// link the cacoward entry to the newly imported WAD.
fn finish_cacoward_import(
    conn: &Connection,
    record: &db::CacowardRecord,
    result: &ImportResult,
    display_name: &str,
) -> Result<(), String> {
    if result.is_duplicate {
        let wad_id = result.duplicate_id.unwrap_or(0);
        println!(
            "Already in library: '{}' (ID: {wad_id})",
            result.duplicate_title.as_deref().unwrap_or("?"),
        );
        // Link the cacoward to the existing dup if it isn't already.
        if record.wad_id.is_none() && wad_id > 0 {
            db::link_wad(conn, record.id, wad_id, false).map_err(|e| e.to_string())?;
            println!("Linked cacoward entry → existing WAD #{wad_id}.");
        }
        return Ok(());
    }
    let Some(wad_id) = result.wad_id else {
        if let Some(ref err) = result.error {
            return Err(format!("Import error: {err}"));
        }
        return Err("Import produced no WAD id (unknown reason).".to_string());
    };
    println!("Imported '{display_name}' (ID: {wad_id})");
    db::link_wad(conn, record.id, wad_id, false).map_err(|e| e.to_string())?;
    println!("Linked cacoward {} → WAD #{wad_id}.", record.wad_title);
    Ok(())
}

fn import_doomwiki_page(
    conn: &Connection,
    title: &str,
    tags: Option<Vec<String>>,
    force: bool,
) -> Result<(), String> {
    let client = DoomwikiClient::new();
    let (entry, has_infobox) = match client.get_entry_permissive(title) {
        Ok(Some(pair)) => pair,
        Ok(None) => return Err(format!("Doom Wiki page not found: '{title}'")),
        Err(caco_sources::SourceError::WafBlocked { .. }) => {
            print_api_hint("doomwiki", title);
            return Err("Doom Wiki blocked the request (WAF challenge).".to_string());
        }
        Err(e) => return Err(e.to_string()),
    };

    // Refuse pages without a {{Wad}} infobox (IWAD articles, disambigs, etc.)
    // unless the user explicitly overrides with --force.
    if !has_infobox && !force {
        return Err(format!(
            "'{title}' has no {{{{Wad}}}} infobox — this looks like an IWAD \
             article or non-WAD page, not a WAD release. Retry with --force \
             to import anyway."
        ));
    }

    let svc = ImportService::new();
    let result = svc.import_doomwiki(conn, &entry, tags, force);

    if result.is_duplicate {
        println!(
            "Already in library: '{}' (ID: {})",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
    } else if let Some(wad_id) = result.wad_id {
        println!("Imported '{}' (ID: {wad_id})", entry.display_name());
    } else if let Some(ref err) = result.error {
        return Err(format!("Import error: {err}"));
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
    let title = args.title.as_deref().unwrap_or(source_str);

    let svc = ImportService::new();
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

    // CLI flags override the regex-extracted thread fields. Everything else
    // comes straight from `thread.*`.
    let final_title = args.title.as_deref();
    let final_author = args.author.as_deref();
    let final_year = args.year;

    let svc = ImportService::new();
    let result = svc.import_doomworld(
        conn,
        &thread,
        tags,
        final_title,
        final_author,
        final_year,
        args.force,
    );

    if result.is_duplicate {
        println!(
            "Already in library: '{}' (ID: {})",
            result.duplicate_title.as_deref().unwrap_or("?"),
            result.duplicate_id.unwrap_or(0),
        );
    } else if let Some(wad_id) = result.wad_id {
        println!(
            "Imported '{}' (ID: {wad_id})",
            final_title.unwrap_or(&thread.title)
        );

        if let Some(iwad) = thread.iwad.as_deref() {
            println!("  IWAD: {iwad}");
        }
        if let Some(port) = thread.sourceport.as_deref() {
            println!("  Sourceport: {port}");
        }
        if let Some(cl) = thread.complevel {
            println!("  Complevel: {cl}");
        }
        if let Some(ref v) = thread.version {
            println!("  Version: {v}");
        }
        if !thread.download_links.is_empty() {
            println!("  Download links: {}", thread.download_links.len());
            for link in &thread.download_links {
                println!("    - {link}");
            }
        }
    } else if let Some(ref err) = result.error {
        return Err(format!("Import error: {err}"));
    }
    Ok(())
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
        .map(|(i, entry)| {
            picker_wad_record(
                i,
                entry.title.clone(),
                Some(entry.author.clone()),
                caco_core::utils::extract_year(&entry.date),
                if entry.description.is_empty() {
                    None
                } else {
                    Some(entry.description.clone())
                },
                db::SourceType::Idgames,
                Some(entry.id.to_string()),
                None,
                Some(entry.filename.clone()),
            )
        })
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService::new();
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
        return Err(
            "No WAD pages found in JSON (only pages with {{Wad}} infobox are imported)."
                .to_string(),
        );
    }

    // Convert to WadRecords for picker
    let wad_records: Vec<db::WadRecord> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            picker_wad_record(
                i,
                entry.display_name().to_string(),
                if entry.author.is_empty() {
                    None
                } else {
                    Some(entry.author.clone())
                },
                entry.year,
                if entry.description.is_empty() {
                    None
                } else {
                    Some(entry.description.clone())
                },
                db::SourceType::Doomwiki,
                Some(entry.page_id.to_string()),
                Some(entry.wiki_url.clone()),
                None,
            )
        })
        .collect();

    let selected = picker::pick_wads(&wad_records, args.multi);
    if selected.is_empty() {
        return Err("No selection made.".to_string());
    }

    let svc = ImportService::new();
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
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {e}"))?;
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
        .or_else(|| path.file_stem().and_then(|s| s.to_str()))
        .unwrap_or("Unknown WAD");

    let svc = ImportService::new();
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
    source_type: db::SourceType,
    source_id: Option<String>,
    source_url: Option<String>,
    filename: Option<String>,
) -> db::WadRecord {
    db::WadRecord {
        id: index as i64 + 1,
        title,
        author,
        year,
        status: db::Status::Unplayed,
        source_type,
        description,
        availability: db::Availability::Unavailable,
        rating: None,
        notes: None,
        source_id,
        source_url,
        idgames_id: None,
        filename,
        cached_path: None,
        custom_iwad: None,
        custom_sourceport: None,
        required_sourceport_family: None,
        custom_args: None,
        companion_files: None,
        custom_config: None,
        version: None,
        complevel: None,
        zdoom_required: None,
        download_urls: None,
        stats_snapshot: None,
        gc_ignore: false,
        deleted_at: None,
        created_at: String::new(),
        updated_at: String::new(),
        tags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_args() -> ImportArgs {
        ImportArgs {
            source: Vec::new(),
            idgames: false,
            doomwiki: false,
            doomworld: false,
            local: false,
            url: None,
            title: None,
            author: None,
            year: None,
            tag: Vec::new(),
            description: None,
            force: false,
            multi: false,
            cacoward: None,
        }
    }

    #[test]
    fn detect_source_routes_idgames_url_to_idgames_id() {
        let args = default_args();
        let url = "https://www.doomworld.com/idgames/?id=18184";
        match detect_source(&args, url).unwrap() {
            SourceKind::IdgamesId(id) => assert_eq!(id, 18184),
            _ => panic!("expected IdgamesId"),
        }
    }

    #[test]
    fn detect_source_routes_idgames_index_php_url() {
        let args = default_args();
        let url = "https://www.doomworld.com/idgames/index.php?id=18184";
        match detect_source(&args, url).unwrap() {
            SourceKind::IdgamesId(id) => assert_eq!(id, 18184),
            _ => panic!("expected IdgamesId"),
        }
    }

    #[test]
    fn detect_source_still_routes_forum_url_to_doomworld() {
        let args = default_args();
        let url = "https://www.doomworld.com/forum/topic/123-something/";
        match detect_source(&args, url).unwrap() {
            SourceKind::Doomworld(got) => assert_eq!(got, url),
            _ => panic!("expected Doomworld"),
        }
    }

    #[test]
    fn detect_source_rejects_malformed_idgames_url() {
        let args = default_args();
        // A query-string URL with neither an id= param nor an archive path
        // portion should still be rejected.
        let url = "https://www.doomworld.com/idgames/?no-id-param=1";
        assert!(detect_source(&args, url).is_err());
    }

    #[test]
    fn detect_source_routes_idgames_slug_url_to_path() {
        let args = default_args();
        let url = "https://www.doomworld.com/idgames/levels/doom2/Ports/v-z/witchinghour";
        match detect_source(&args, url).unwrap() {
            SourceKind::IdgamesPath(path) => {
                // Extractor normalizes slug → .zip for the idgames API.
                assert_eq!(path, "levels/doom2/Ports/v-z/witchinghour.zip");
            }
            _ => panic!("expected IdgamesPath"),
        }
    }

    #[test]
    fn detect_source_routes_idgames_zip_url_to_path() {
        let args = default_args();
        let url = "https://www.doomworld.com/idgames/levels/doom2/megawads/scythe.zip";
        match detect_source(&args, url).unwrap() {
            SourceKind::IdgamesPath(path) => {
                assert_eq!(path, "levels/doom2/megawads/scythe.zip");
            }
            _ => panic!("expected IdgamesPath"),
        }
    }

    #[test]
    fn detect_source_idgames_flag_accepts_url() {
        let mut args = default_args();
        args.idgames = true;
        let url = "https://www.doomworld.com/idgames/levels/doom2/Ports/v-z/witchinghour";
        match detect_source(&args, url).unwrap() {
            SourceKind::IdgamesPath(path) => {
                assert_eq!(path, "levels/doom2/Ports/v-z/witchinghour.zip");
            }
            _ => panic!("expected IdgamesPath"),
        }
    }

    #[test]
    fn detect_source_routes_doomwiki_wiki_url_to_page() {
        let args = default_args();
        let url = "https://doomwiki.org/wiki/DBP31:_Santa%27s_Outback_Bender";
        match detect_source(&args, url).unwrap() {
            SourceKind::DoomwikiPage(title) => {
                assert_eq!(title, "DBP31: Santa's Outback Bender")
            }
            _ => panic!("expected DoomwikiPage"),
        }
    }

    #[test]
    fn detect_source_falls_back_to_search_for_non_wiki_doomwiki_url() {
        let args = default_args();
        let url = "https://doomwiki.org/w/index.php?search=foo";
        match detect_source(&args, url).unwrap() {
            SourceKind::Doomwiki(got) => assert_eq!(got, url),
            _ => panic!("expected Doomwiki search fallback"),
        }
    }
}

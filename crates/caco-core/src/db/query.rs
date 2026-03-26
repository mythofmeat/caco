use rusqlite::Connection;

use super::connection::{attach_tags, fetch_tags_batch};
use super::models::{AndGroup, ParsedQuery, QueryTerm, SourceType, WadRecord, STATUS_SHORTCUTS};
use crate::complevel::parse_complevel;
use crate::Result;

/// Convert a glob pattern to SQL LIKE pattern.
fn glob_to_like(pattern: &str) -> String {
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern.to_string();
    }
    let result = pattern.replace('%', r"\%").replace('_', r"\_");
    result.replace('*', "%").replace('?', "_")
}

/// Check if a string contains glob wildcards.
fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// Split query by OR_SEPARATOR (" , ") respecting quoted strings.
fn split_or_groups(query: &str) -> Vec<String> {
    let sep = super::models::OR_SEPARATOR;
    let sep_bytes = sep.as_bytes();
    let sep_len = sep.len();
    let bytes = query.as_bytes();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    let mut in_quotes = false;
    let mut quote_char = 0u8;

    while i < bytes.len() {
        let ch = bytes[i];

        if ch == b'"' || ch == b'\'' {
            if !in_quotes {
                in_quotes = true;
                quote_char = ch;
            } else if ch == quote_char {
                in_quotes = false;
            }
            current.push(ch as char);
            i += 1;
            continue;
        }

        // Check for OR_SEPARATOR pattern (not inside quotes)
        if !in_quotes && i + sep_len <= bytes.len() && &bytes[i..i + sep_len] == sep_bytes {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                parts.push(trimmed);
            }
            current.clear();
            i += sep_len;
            continue;
        }

        current.push(ch as char);
        i += 1;
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }

    parts
}

/// Simple shell-like split respecting quoted strings.
fn shell_split(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';

    for ch in s.chars() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Parse a single AND group into terms.
fn parse_and_group(group_str: &str) -> Vec<QueryTerm> {
    let tokens = shell_split(group_str);
    let mut terms = Vec::new();

    for mut token in tokens {
        let mut negated = false;

        if (token.starts_with('-') || token.starts_with('^')) && token.len() > 1 {
            negated = true;
            token = token[1..].to_string();
        }

        if let Some((field, value)) = token.split_once(':') {
            let mut field = field.to_lowercase();
            if field == "name" {
                field = "title".to_string();
            }
            terms.push(QueryTerm {
                field: Some(field),
                value: value.to_string(),
                negated,
            });
        } else {
            terms.push(QueryTerm {
                field: None,
                value: token,
                negated,
            });
        }
    }

    terms
}

/// Parse beets-style query into structured form.
///
/// Syntax:
/// - Field queries: `field:value`, `field:"quoted value"`
/// - Free text: `word` (searches title/author/description)
/// - Negation: `-field:value`, `^field:value`
/// - OR groups: `term1 term2 , term3 term4` (comma surrounded by spaces)
/// - Field aliases: `name:` -> `title:`
pub fn parse_query(query: &str) -> ParsedQuery {
    if query.trim().is_empty() {
        return ParsedQuery::default();
    }

    let or_parts = split_or_groups(query);
    let mut or_groups = Vec::new();

    for part in or_parts {
        let terms = parse_and_group(&part);
        if !terms.is_empty() {
            or_groups.push(AndGroup { terms });
        }
    }

    ParsedQuery { or_groups }
}

/// Normalize status value, expanding shortcuts.
pub fn normalize_status(value: &str) -> String {
    let lower = value.to_lowercase();
    STATUS_SHORTCUTS
        .get(lower.as_str())
        .map(|s| s.to_string())
        .unwrap_or(lower)
}

/// Boxed SQL parameter for dynamic dispatch.
type SqlParam = Box<dyn rusqlite::types::ToSql>;

/// Build SQL clause for a single QueryTerm.
fn build_term_sql(term: &QueryTerm) -> (String, Vec<SqlParam>) {
    let (clause, params): (String, Vec<SqlParam>) = match term.field.as_deref() {
        None => {
            let like = format!("%{}%", term.value);
            (
                "(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)".into(),
                vec![Box::new(like.clone()), Box::new(like.clone()), Box::new(like)],
            )
        }

        Some("id") => {
            if let Ok(id) = term.value.parse::<i64>() {
                ("wads.id = ?".into(), vec![Box::new(id)])
            } else {
                return (String::new(), Vec::new());
            }
        }

        Some("title") => (
            "wads.title LIKE ?".into(),
            vec![Box::new(format!("%{}%", term.value))],
        ),

        Some("author") => (
            "wads.author LIKE ?".into(),
            vec![Box::new(format!("%{}%", term.value))],
        ),

        Some("year") => {
            if let Ok(year) = term.value.parse::<i32>() {
                ("wads.year = ?".into(), vec![Box::new(year)])
            } else {
                return (String::new(), Vec::new());
            }
        }

        Some("filename") => (
            "wads.filename LIKE ?".into(),
            vec![Box::new(format!("%{}%", term.value))],
        ),

        Some("status") => {
            let normalized = normalize_status(&term.value);
            ("wads.status = ?".into(), vec![Box::new(normalized)])
        }

        Some("source") => (
            "wads.source_type = ?".into(),
            vec![Box::new(term.value.to_lowercase())],
        ),

        Some("tag") => {
            let tag_pattern = term.value.to_lowercase();
            if is_glob_pattern(&tag_pattern) {
                let like_pattern = glob_to_like(&tag_pattern);
                (
                    r"wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ? ESCAPE '\')".into(),
                    vec![Box::new(like_pattern)],
                )
            } else {
                let escaped = tag_pattern
                    .replace('\\', r"\\")
                    .replace('%', r"\%")
                    .replace('_', r"\_");
                (
                    r"wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ? ESCAPE '\')".into(),
                    vec![Box::new(format!("%{escaped}%"))],
                )
            }
        }

        Some("iwad") => (
            "wads.custom_iwad LIKE ?".into(),
            vec![Box::new(format!("%{}%", term.value))],
        ),

        Some("complevel") => {
            if let Some(cl) = parse_complevel(&term.value) {
                ("wads.complevel = ?".into(), vec![Box::new(cl)])
            } else {
                return (String::new(), Vec::new());
            }
        }

        Some("config") => (
            "wads.custom_config LIKE ?".into(),
            vec![Box::new(format!("%{}%", term.value))],
        ),

        Some(_) => {
            // Unknown field — treat as free text
            let like = format!("%{}%", term.value);
            (
                "(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)".into(),
                vec![Box::new(like.clone()), Box::new(like.clone()), Box::new(like)],
            )
        }
    };

    if term.negated && !clause.is_empty() {
        (format!("NOT ({clause})"), params)
    } else {
        (clause, params)
    }
}

/// Build SQL WHERE clause from a ParsedQuery.
fn build_query_sql(parsed: &ParsedQuery) -> (String, Vec<SqlParam>) {
    if parsed.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut or_clauses = Vec::new();
    let mut all_params: Vec<SqlParam> = Vec::new();

    for and_group in &parsed.or_groups {
        let mut and_clauses = Vec::new();

        for term in &and_group.terms {
            let (clause, term_params) = build_term_sql(term);
            if !clause.is_empty() {
                and_clauses.push(clause);
                all_params.extend(term_params);
            }
        }

        if !and_clauses.is_empty() {
            or_clauses.push(format!("({})", and_clauses.join(" AND ")));
        }
    }

    if or_clauses.is_empty() {
        return (String::new(), Vec::new());
    }

    (or_clauses.join(" OR "), all_params)
}

/// Allowed sort fields for `search_wads`.
const ALLOWED_SORT_FIELDS: &[&str] = &[
    "id", "playtime", "rating", "created", "title", "author", "last_played", "year", "random",
];

/// Search WADs with beets-style query syntax.
///
/// If `include_deleted` is `true`, only shows deleted WADs (trash view).
/// Otherwise, excludes deleted WADs.
pub fn search_wads(
    conn: &Connection,
    query: Option<&str>,
    sort_by: Option<&str>,
    sort_desc: bool,
    include_deleted: bool,
    limit: usize,
) -> Result<Vec<WadRecord>> {
    if let Some(field) = sort_by
        && !ALLOWED_SORT_FIELDS.contains(&field)
    {
        return Err(crate::Error::InvalidField(format!(
            "Invalid sort field: {field}"
        )));
    }

    let mut conditions = Vec::new();
    let mut params: Vec<SqlParam> = Vec::new();

    if include_deleted {
        conditions.push("wads.deleted_at IS NOT NULL".to_string());
    } else {
        conditions.push("wads.deleted_at IS NULL".to_string());
    }

    if let Some(q) = query
        && !q.trim().is_empty()
    {
        let parsed = parse_query(q);
        if !parsed.is_empty() {
            let (query_sql, query_params) = build_query_sql(&parsed);
            if !query_sql.is_empty() {
                conditions.push(format!("({query_sql})"));
                params.extend(query_params);
            }
        }
    }

    let where_clause = if conditions.is_empty() {
        "1=1".to_string()
    } else {
        conditions.join(" AND ")
    };

    let direction = if sort_desc { "DESC" } else { "ASC" };
    let reverse_dir = if sort_desc { "ASC" } else { "DESC" };
    let nulls = if sort_desc {
        "NULLS LAST"
    } else {
        "NULLS FIRST"
    };
    let reverse_nulls = if sort_desc {
        "NULLS FIRST"
    } else {
        "NULLS LAST"
    };

    let (order_by, use_group_by) = match sort_by {
        Some("id") => (format!("wads.id {reverse_dir}"), false),
        Some("playtime") => (
            format!("COALESCE(SUM(sessions.duration_seconds), 0) {direction}"),
            true,
        ),
        Some("rating") => (format!("wads.rating {direction} {nulls}"), false),
        Some("created") => (format!("wads.created_at {direction}"), false),
        Some("title") => (format!("LOWER(wads.title) {reverse_dir}"), false),
        Some("author") => (
            format!("LOWER(wads.author) {reverse_dir} {reverse_nulls}"),
            false,
        ),
        Some("last_played") => (
            format!("MAX(sessions.started_at) {direction} {nulls}"),
            true,
        ),
        Some("year") => (format!("wads.year {direction} {nulls}"), false),
        Some("random") => ("RANDOM()".to_string(), false),
        _ => ("wads.id ASC".to_string(), false),
    };

    let limit_clause = if limit > 0 {
        format!(" LIMIT {limit}")
    } else {
        String::new()
    };

    let sql = if use_group_by {
        format!(
            "SELECT wads.* FROM wads \
             LEFT JOIN sessions ON sessions.wad_id = wads.id \
             WHERE {where_clause} \
             GROUP BY wads.id \
             ORDER BY {order_by}{limit_clause}"
        )
    } else {
        format!(
            "SELECT wads.* FROM wads \
             WHERE {where_clause} \
             ORDER BY {order_by}{limit_clause}"
        )
    };

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let mut results: Vec<WadRecord> = stmt
        .query_map(param_refs.as_slice(), WadRecord::from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Batch-fetch tags
    if !results.is_empty() {
        let wad_ids: Vec<i64> = results.iter().map(|w| w.id).collect();
        let tags_by_wad = fetch_tags_batch(conn, &wad_ids)?;
        for wad in &mut results {
            if let Some(tags) = tags_by_wad.get(&wad.id) {
                wad.tags.clone_from(tags);
            }
        }
    }

    Ok(results)
}

/// Find a potential duplicate WAD in the library.
///
/// Detection strategy (in priority order):
/// 1. idgames/doomwiki/doomworld: exact match on source_id
/// 2. URL/local: exact match on source_url
/// 3. Fallback: normalized filename + author match
pub fn find_duplicate(
    conn: &Connection,
    source_type: SourceType,
    source_id: Option<&str>,
    source_url: Option<&str>,
    filename: Option<&str>,
    author: Option<&str>,
) -> Result<Option<WadRecord>> {
    // Strategy 1-3: Match by source_type + source_id
    if let Some(sid) = source_id
        && matches!(
            source_type,
            SourceType::Idgames | SourceType::Doomwiki | SourceType::Doomworld
        )
    {
        let mut stmt = conn.prepare(
            "SELECT * FROM wads WHERE source_type = ? AND source_id = ?",
        )?;
        match stmt.query_row(
            rusqlite::params![source_type.as_str(), sid],
            WadRecord::from_row,
        ) {
            Ok(mut wad) => {
                attach_tags(conn, &mut wad)?;
                return Ok(Some(wad));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }
    }

    // Strategy 4: Match by source_url
    if let Some(url) = source_url
        && matches!(source_type, SourceType::Url | SourceType::Local)
    {
        let mut stmt = conn.prepare(
            "SELECT * FROM wads WHERE source_type = ? AND source_url = ?",
        )?;
        match stmt.query_row(
            rusqlite::params![source_type.as_str(), url],
            WadRecord::from_row,
        ) {
            Ok(mut wad) => {
                attach_tags(conn, &mut wad)?;
                return Ok(Some(wad));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }
    }

    // Strategy 5: Fuzzy match on normalized filename + author
    if let Some(fname) = filename {
        let mut normalized = fname.to_lowercase();
        for ext in &[".zip", ".wad", ".pk3", ".pk7"] {
            if normalized.ends_with(ext) {
                normalized.truncate(normalized.len() - ext.len());
                break;
            }
        }

        let row_result = if let Some(auth) = author {
            let mut stmt = conn.prepare(
                "SELECT * FROM wads WHERE LOWER(filename) LIKE ? AND LOWER(author) LIKE ?",
            )?;
            stmt.query_row(
                rusqlite::params![
                    format!("%{normalized}%"),
                    format!("%{}%", auth.to_lowercase())
                ],
                WadRecord::from_row,
            )
        } else {
            let mut stmt =
                conn.prepare("SELECT * FROM wads WHERE LOWER(filename) LIKE ?")?;
            stmt.query_row(
                rusqlite::params![format!("%{normalized}%")],
                WadRecord::from_row,
            )
        };

        match row_result {
            Ok(mut wad) => {
                attach_tags(conn, &mut wad)?;
                return Ok(Some(wad));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::open_memory;
    use crate::db::models::Status;
    use crate::db::schema::init_db;
    use crate::db::wads::{add_wad, NewWad};

    fn setup() -> Connection {
        let conn = open_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn add_test_wads(conn: &Connection) {
        add_wad(
            conn,
            &NewWad::new("Scythe", SourceType::Idgames)
                .author("Erik Alm")
                .year(2003)
                .source_id("12345")
                .tags(vec!["megawad".into(), "cacoward".into()]),
        )
        .unwrap();

        add_wad(
            conn,
            &NewWad::new("Ancient Aliens", SourceType::Idgames)
                .author("skillsaw")
                .year(2016)
                .status(Status::Playing)
                .tags(vec!["megawad".into(), "cacoward".into()]),
        )
        .unwrap();

        add_wad(
            conn,
            &NewWad::new("Sunlust", SourceType::Doomwiki)
                .author("Ribbiks & Dannebubinga")
                .year(2015)
                .status(Status::Finished)
                .tags(vec!["megawad".into(), "slaughter".into()]),
        )
        .unwrap();
    }

    #[test]
    fn test_parse_query_empty() {
        assert!(parse_query("").is_empty());
        assert!(parse_query("   ").is_empty());
    }

    #[test]
    fn test_parse_query_simple_field() {
        let q = parse_query("status:playing");
        assert_eq!(q.or_groups.len(), 1);
        assert_eq!(q.or_groups[0].terms.len(), 1);
        assert_eq!(q.or_groups[0].terms[0].field.as_deref(), Some("status"));
        assert_eq!(q.or_groups[0].terms[0].value, "playing");
        assert!(!q.or_groups[0].terms[0].negated);
    }

    #[test]
    fn test_parse_query_negation() {
        let q = parse_query("-status:finished");
        assert!(q.or_groups[0].terms[0].negated);
        assert_eq!(q.or_groups[0].terms[0].field.as_deref(), Some("status"));

        let q = parse_query("^tag:megawad");
        assert!(q.or_groups[0].terms[0].negated);
        assert_eq!(q.or_groups[0].terms[0].field.as_deref(), Some("tag"));
    }

    #[test]
    fn test_parse_query_or_groups() {
        let q = parse_query("status:playing , status:to-play");
        assert_eq!(q.or_groups.len(), 2);
    }

    #[test]
    fn test_parse_query_free_text() {
        let q = parse_query("scythe");
        assert_eq!(q.or_groups[0].terms[0].field, None);
        assert_eq!(q.or_groups[0].terms[0].value, "scythe");
    }

    #[test]
    fn test_parse_query_quoted() {
        let q = parse_query("\"ancient aliens\"");
        assert_eq!(q.or_groups[0].terms.len(), 1);
        assert_eq!(q.or_groups[0].terms[0].value, "ancient aliens");
    }

    #[test]
    fn test_parse_query_field_alias() {
        let q = parse_query("name:scythe");
        assert_eq!(q.or_groups[0].terms[0].field.as_deref(), Some("title"));
    }

    #[test]
    fn test_normalize_status() {
        assert_eq!(normalize_status("p"), "playing");
        assert_eq!(normalize_status("f"), "finished");
        assert_eq!(normalize_status("playing"), "playing");
        assert_eq!(normalize_status("unknown"), "unknown");
    }

    #[test]
    fn test_glob_to_like() {
        assert_eq!(glob_to_like("caco*"), "caco%");
        assert_eq!(glob_to_like("test?"), "test_");
        assert_eq!(glob_to_like("exact"), "exact");
        assert_eq!(glob_to_like("a%b"), "a%b"); // not a glob, return as-is
        assert_eq!(glob_to_like("a%b*"), r"a\%b%"); // glob with existing %
    }

    #[test]
    fn test_search_wads_all() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, None, None, true, false, 0).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_wads_by_status() {
        let conn = setup();
        add_test_wads(&conn);

        let results =
            search_wads(&conn, Some("status:playing"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Ancient Aliens");
    }

    #[test]
    fn test_search_wads_by_author() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, Some("author:alm"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Scythe");
    }

    #[test]
    fn test_search_wads_free_text() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, Some("sunlust"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_wads_negation() {
        let conn = setup();
        add_test_wads(&conn);

        let results =
            search_wads(&conn, Some("-status:finished"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_wads_or() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(
            &conn,
            Some("status:playing , status:finished"),
            None,
            true,
            false,
            0,
        )
        .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_wads_tag() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, Some("tag:slaughter"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Sunlust");
    }

    #[test]
    fn test_search_wads_tag_glob() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, Some("tag:caco*"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_wads_sort() {
        let conn = setup();
        add_test_wads(&conn);

        // sort_desc=true is the default; for title, reverse_dir=ASC → A-Z
        let results = search_wads(&conn, None, Some("title"), true, false, 0).unwrap();
        assert_eq!(results[0].title, "Ancient Aliens");
        assert_eq!(results[2].title, "Sunlust");
    }

    #[test]
    fn test_search_wads_limit() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, None, None, true, false, 1).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_wads_invalid_sort() {
        let conn = setup();
        let result = search_wads(&conn, None, Some("invalid"), true, false, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_wads_with_tags() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, None, None, true, false, 0).unwrap();
        for wad in &results {
            assert!(!wad.tags.is_empty());
        }
    }

    #[test]
    fn test_search_wads_status_shortcut() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, Some("status:p"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Ancient Aliens");
    }

    #[test]
    fn test_find_duplicate_by_source_id() {
        let conn = setup();
        add_test_wads(&conn);

        let dup =
            find_duplicate(&conn, SourceType::Idgames, Some("12345"), None, None, None).unwrap();
        assert!(dup.is_some());
        assert_eq!(dup.unwrap().title, "Scythe");
    }

    #[test]
    fn test_find_duplicate_no_match() {
        let conn = setup();
        add_test_wads(&conn);

        let dup =
            find_duplicate(&conn, SourceType::Idgames, Some("99999"), None, None, None).unwrap();
        assert!(dup.is_none());
    }

    #[test]
    fn test_find_duplicate_by_filename() {
        let conn = setup();
        add_wad(
            &conn,
            &NewWad::new("Test WAD", SourceType::Local)
                .filename("test.wad")
                .author("TestAuthor"),
        )
        .unwrap();

        let dup = find_duplicate(
            &conn,
            SourceType::Local,
            None,
            None,
            Some("test.wad"),
            Some("TestAuthor"),
        )
        .unwrap();
        assert!(dup.is_some());
    }

    #[test]
    fn test_find_duplicate_by_source_url() {
        let conn = setup();
        add_wad(
            &conn,
            &NewWad::new("URL WAD", SourceType::Url)
                .source_url("https://example.com/test.wad"),
        )
        .unwrap();

        let dup = find_duplicate(
            &conn,
            SourceType::Url,
            None,
            Some("https://example.com/test.wad"),
            None,
            None,
        )
        .unwrap();
        assert!(dup.is_some());
        assert_eq!(dup.unwrap().title, "URL WAD");
    }

    #[test]
    fn test_search_wads_and() {
        let conn = setup();
        add_test_wads(&conn);

        // Both terms must match
        let results = search_wads(
            &conn,
            Some("author:alm year:2003"),
            None,
            true,
            false,
            0,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Scythe");

        // Contradictory terms
        let results = search_wads(
            &conn,
            Some("author:alm year:2016"),
            None,
            true,
            false,
            0,
        )
        .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_search_wads_by_id() {
        let conn = setup();
        add_test_wads(&conn);

        let results = search_wads(&conn, Some("id:1"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Scythe");
    }

    #[test]
    fn test_search_wads_complevel() {
        let conn = setup();
        let id = add_wad(
            &conn,
            &NewWad::new("Boom WAD", SourceType::Local).author("Test"),
        )
        .unwrap();
        // Set complevel via raw SQL since WadUpdate doesn't expose it directly as int easily
        conn.execute("UPDATE wads SET complevel = 9 WHERE id = ?", [id])
            .unwrap();

        let results =
            search_wads(&conn, Some("complevel:boom"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Boom WAD");

        let results = search_wads(&conn, Some("complevel:9"), None, true, false, 0).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_deleted_wads() {
        let conn = setup();
        add_test_wads(&conn);

        // Soft-delete one WAD
        use crate::db::wads::delete_wad;
        delete_wad(&conn, 1, false).unwrap();

        // Normal search excludes deleted
        let results = search_wads(&conn, None, None, true, false, 0).unwrap();
        assert_eq!(results.len(), 2);

        // Trash view shows only deleted
        let results = search_wads(&conn, None, None, true, true, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Scythe");
    }
}

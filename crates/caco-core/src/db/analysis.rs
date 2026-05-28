use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use super::connection::SQLITE_MAX_VARS;
use crate::Result;
use crate::wad_analysis::{ANALYSIS_VERSION, WadAnalysis, analyze_path};

/// Get stored WAD analysis, if any.
///
/// Returns `None` if no analysis exists or if the stored analysis was
/// produced by an older version of the detection logic (triggering
/// automatic re-analysis by the caller).
pub fn get_analysis(conn: &Connection, wad_id: i64) -> Result<Option<WadAnalysis>> {
    let mut stmt = conn.prepare("SELECT analysis_json FROM wad_analysis WHERE wad_id = ?1")?;
    match stmt.query_row([wad_id], |row| row.get::<_, Option<String>>(0)) {
        Ok(Some(json)) => {
            let analysis: WadAnalysis = serde_json::from_str(&json).map_err(|e| {
                crate::Error::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
            })?;
            if analysis.version < ANALYSIS_VERSION {
                // Stale analysis from older detection logic — re-analyze.
                Ok(None)
            } else {
                Ok(Some(analysis))
            }
        }
        Ok(None) => Ok(None),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Fetch fresh `WadAnalysis` for each WAD in a single query.
///
/// Stale rows (`version < ANALYSIS_VERSION`) are filtered out — callers see
/// the same "no analysis yet" signal they would for a never-analyzed WAD,
/// and can trigger [`ensure_fresh_analysis`] to refresh.
pub fn get_analyses_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, WadAnalysis>> {
    let mut out: HashMap<i64, WadAnalysis> = HashMap::new();
    if wad_ids.is_empty() {
        return Ok(out);
    }
    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders: String = (0..chunk.len())
            .map(|i| if i > 0 { ",?" } else { "?" })
            .collect();
        let sql = format!(
            "SELECT wad_id, analysis_json FROM wad_analysis WHERE wad_id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            let wad_id: i64 = row.get(0)?;
            let json: Option<String> = row.get(1)?;
            Ok((wad_id, json))
        })?;
        for row in rows {
            let (wad_id, json_opt) = row?;
            let Some(json) = json_opt else { continue };
            let Ok(analysis) = serde_json::from_str::<WadAnalysis>(&json) else {
                continue;
            };
            if analysis.version >= ANALYSIS_VERSION {
                out.insert(wad_id, analysis);
            }
        }
    }
    Ok(out)
}

/// Fetch `required_maps` for each WAD in a single query.
///
/// Backed by [`get_analyses_batch`] so the same staleness filter applies —
/// WADs without a fresh analysis are simply absent from the map. (Callers
/// that previously read the `required_maps` column directly were silently
/// trusting analyses produced by older detection logic.)
pub fn get_required_maps_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, usize>> {
    let analyses = get_analyses_batch(conn, wad_ids)?;
    Ok(analyses
        .into_iter()
        .map(|(id, a)| (id, a.required_maps))
        .collect())
}

/// Return a fresh [`WadAnalysis`] for `wad_id`, re-analyzing the file at
/// `wad_path` and persisting the new row when the cache is missing or stale.
///
/// Returns `None` only when the file cannot be analyzed (missing on disk,
/// not a valid WAD, no maps detected). Save failures are logged and the
/// in-memory analysis is still returned, so a transient DB error doesn't
/// poison the verdict.
pub fn ensure_fresh_analysis(
    conn: &Connection,
    wad_id: i64,
    wad_path: &Path,
) -> Option<WadAnalysis> {
    match get_analysis(conn, wad_id) {
        Ok(Some(a)) => return Some(a),
        Ok(None) => {}
        Err(_) => return None,
    }
    let analysis = analyze_path(wad_path)?;
    if let Err(e) = save_analysis(conn, wad_id, &analysis) {
        tracing::warn!("failed to save wad analysis for wad {wad_id}: {e}");
    }
    Some(analysis)
}

/// Store WAD analysis results.
pub fn save_analysis(conn: &Connection, wad_id: i64, analysis: &WadAnalysis) -> Result<()> {
    let json = serde_json::to_string(analysis).map_err(|e| {
        crate::Error::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
    })?;
    conn.execute(
        "INSERT OR REPLACE INTO wad_analysis
             (wad_id, total_maps, required_maps, secret_maps, terminal_map,
              has_umapinfo, analysis_json, analyzed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
        rusqlite::params![
            wad_id,
            analysis.total_maps as i64,
            analysis.required_maps as i64,
            serde_json::to_string(&analysis.secret_maps).unwrap_or_default(),
            analysis.terminal_map,
            analysis.has_umapinfo as i32,
            json,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;
    use crate::wad_analysis::{MapClassification, MapInfo};

    fn open_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::init_db(&conn).unwrap();
        conn
    }

    fn fresh_analysis(maps: &[(&str, MapClassification)]) -> WadAnalysis {
        let infos: Vec<MapInfo> = maps
            .iter()
            .map(|(lump, c)| MapInfo {
                lump: lump.to_string(),
                classification: *c,
            })
            .collect();
        let required_maps = infos
            .iter()
            .filter(|m| m.classification == MapClassification::Required)
            .count();
        WadAnalysis {
            version: ANALYSIS_VERSION,
            total_maps: infos.len(),
            required_maps,
            secret_maps: vec![],
            terminal_map: None,
            has_umapinfo: false,
            maps: infos,
        }
    }

    fn insert_test_wad(conn: &Connection, id: i64, title: &str) {
        conn.execute(
            "INSERT INTO wads (id, title, source_type) VALUES (?1, ?2, 'manual')",
            rusqlite::params![id, title],
        )
        .unwrap();
    }

    #[test]
    fn batch_filters_stale_rows() {
        let conn = open_test_db();
        insert_test_wad(&conn, 1, "fresh");
        insert_test_wad(&conn, 2, "stale");

        let fresh = fresh_analysis(&[
            ("MAP01", MapClassification::Required),
            ("MAP02", MapClassification::Required),
        ]);
        save_analysis(&conn, 1, &fresh).unwrap();

        // Hand-roll a stale row with version=0 so a future bump can never
        // accidentally make it look fresh.
        let stale_json = r#"{"version":0,"maps":[],"total_maps":7,"required_maps":7,"secret_maps":[],"terminal_map":null,"has_umapinfo":false}"#;
        conn.execute(
            "INSERT INTO wad_analysis (wad_id, total_maps, required_maps, analysis_json)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![2_i64, 7_i64, 7_i64, stale_json],
        )
        .unwrap();

        let analyses = get_analyses_batch(&conn, &[1, 2]).unwrap();
        assert!(analyses.contains_key(&1));
        assert!(
            !analyses.contains_key(&2),
            "stale v0 row must be filtered out"
        );

        let counts = get_required_maps_batch(&conn, &[1, 2]).unwrap();
        assert_eq!(counts.get(&1), Some(&2));
        assert!(
            !counts.contains_key(&2),
            "required_maps must mirror analyses_batch staleness"
        );
    }
}

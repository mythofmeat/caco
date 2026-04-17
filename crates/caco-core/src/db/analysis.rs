use std::collections::HashMap;

use rusqlite::Connection;

use super::connection::SQLITE_MAX_VARS;
use crate::Result;
use crate::wad_analysis::{ANALYSIS_VERSION, WadAnalysis};

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

/// Fetch `required_maps` for each WAD in a single query.
///
/// Reads directly from the stored column so we avoid deserializing the full
/// analysis JSON. WADs without analysis are simply absent from the map.
pub fn get_required_maps_batch(conn: &Connection, wad_ids: &[i64]) -> Result<HashMap<i64, usize>> {
    let mut out: HashMap<i64, usize> = HashMap::new();
    if wad_ids.is_empty() {
        return Ok(out);
    }
    for chunk in wad_ids.chunks(SQLITE_MAX_VARS) {
        let placeholders: String = (0..chunk.len())
            .map(|i| if i > 0 { ",?" } else { "?" })
            .collect();
        let sql = format!(
            "SELECT wad_id, required_maps FROM wad_analysis WHERE wad_id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            let wad_id: i64 = row.get(0)?;
            let required: i64 = row.get(1)?;
            Ok((wad_id, required.max(0) as usize))
        })?;
        for row in rows {
            let (wad_id, required) = row?;
            out.insert(wad_id, required);
        }
    }
    Ok(out)
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

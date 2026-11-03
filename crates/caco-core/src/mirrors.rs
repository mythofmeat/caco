//! Mirrors: stores of WAD files the user controls (#54).
//!
//! A large share of recent Cacoward entries never reach idgames, and the share
//! links on their forum threads rot. The keep dir protects such a WAD only on the
//! machine that downloaded it. A mirror is what protects it everywhere else: a
//! directory or an HTTP file store the user configures, which caco reads from
//! before anything else and offers to write to whenever it gets a WAD it could not
//! fetch again. caco ships none, because many WAD text files restrict
//! redistribution: a private archive of files you downloaded is your business,
//! where a default URL in caco would be caco's.
//!
//! This module is the mirror's format, and nothing here touches a network. The
//! transport is in `caco_sources::mirror`, and what a mirror is known to hold is
//! in [`crate::db::mirrors`].
//!
//! # Layout
//!
//! ```text
//! <root>/index/index.json
//! <root>/<sha256>/<filename>
//! ```
//!
//! Content-addressed, the idea behind `companions/{md5[:12]}_{filename}`:
//! deduplication and verification for free, with the original file name kept for
//! the sourceport.
//!
//! The index sits one directory down so that it has the same shape as every file.
//! Gitea's generic package registry only has paths of the form
//! `<package>/<version>/<file>`, and it answers anything shallower with a 405. With
//! `<root>` as the package, each `<sha256>` is a version and `index` is one too, so
//! the same layout serves a static file server, a NAS mount and a registry, with
//! nothing to configure.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::MirrorConfig;
use crate::db::{self, WadRecord};
use crate::identity::FileIdentity;
use crate::source_url::{canonical_page, doomwiki_page_from_url, extract_topic_id_from_url};

/// Where a mirror's index is, under its root.
pub const INDEX_PATH: &str = "index/index.json";

/// The newest index format this build understands.
const INDEX_VERSION: u64 = 1;

/// One file on a mirror, as its index describes it.
///
/// Everything but the first three fields is optional, so that an index written by
/// hand or by a script only has to say what it knows. The identifiers use the
/// Cacowards dataset's words for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirrorEntry {
    /// Of the file as it is stored: what a download is checked against.
    pub sha256: String,
    pub size: u64,
    /// The file's own name, which it gets back when it is downloaded.
    pub filename: String,
    /// Its name on the mirror, when the mirror refused [`filename`](Self::filename).
    /// Gitea's registry does that to a name with an apostrophe, a comma or anything
    /// outside ASCII. See [`safe_stored_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stored_as: Option<String>,
    /// What a sourceport loads from it: see [`crate::identity`]. So that a mirror
    /// holding a re-zip of a WAD still counts as holding that WAD. Taken to be
    /// `sha256` when absent, which is right for anything but a zip around a WAD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Its idgames file id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idgames: Option<i64>,
    /// Its Doomworld forum topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doomworld: Option<i64>,
    /// Its Doom Wiki page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doomwiki: Option<String>,
    /// The keys of the Cacowards it won. A list, because a WAD can win several.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cacowards: Vec<String>,
    /// When it was uploaded, as RFC 3339 in UTC, so that the newest of several
    /// releases of one WAD is the one fetched for a row that names none of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<String>,
}

impl MirrorEntry {
    /// The file's name on the mirror.
    pub fn stored_name(&self) -> &str {
        self.stored_as.as_deref().unwrap_or(&self.filename)
    }

    /// The file's path under the mirror's root.
    pub fn stored_path(&self) -> String {
        stored_path(&self.sha256, self.stored_name())
    }

    /// This entry in its normal form, or `None` if it cannot be used.
    ///
    /// An index comes from somewhere else, and `sha256` and `filename` go on to
    /// name a path on this machine, so `"filename": "../../.bashrc"` has to stop
    /// here rather than be written wherever it points.
    fn validated(mut self) -> Option<Self> {
        self.sha256 = normal_hash(&self.sha256)?;
        if !is_bare_file_name(&self.filename) || i64::try_from(self.size).is_err() {
            return None;
        }
        if self
            .stored_as
            .as_deref()
            .is_some_and(|name| !is_bare_file_name(name))
        {
            return None;
        }
        self.content_sha256 = match self.content_sha256.as_deref() {
            Some(hash) => Some(normal_hash(hash)?),
            None => None,
        };
        self.title = self.title.filter(|text| !text.trim().is_empty());
        self.author = self.author.filter(|text| !text.trim().is_empty());
        self.doomwiki = self.doomwiki.filter(|page| !page.trim().is_empty());
        Some(self)
    }

    /// This entry, for a file the index already has under `existing`.
    ///
    /// The file's own facts stay as they were, since they describe what is on the
    /// mirror, unless it had to go back under a new name. What is known about it adds
    /// up, so a machine that only knows a WAD by its forum thread does not take away
    /// the idgames id another one recorded.
    fn merged_over(self, existing: MirrorEntry) -> Self {
        let mut cacowards = existing.cacowards;
        for key in self.cacowards {
            if !cacowards.contains(&key) {
                cacowards.push(key);
            }
        }
        Self {
            sha256: existing.sha256,
            size: existing.size,
            filename: existing.filename,
            stored_as: self.stored_as.or(existing.stored_as),
            added: existing.added.or(self.added),
            content_sha256: self.content_sha256.or(existing.content_sha256),
            title: self.title.or(existing.title),
            author: self.author.or(existing.author),
            idgames: self.idgames.or(existing.idgames),
            doomworld: self.doomworld.or(existing.doomworld),
            doomwiki: self.doomwiki.or(existing.doomwiki),
            cacowards,
        }
    }
}

/// A file's path under a mirror's root.
pub fn stored_path(sha256: &str, stored_name: &str) -> String {
    format!("{sha256}/{stored_name}")
}

/// What a mirror's index says, read as what it is: a document someone else may
/// also be writing.
///
/// Held as JSON rather than as a list of [`MirrorEntry`], because writing it back
/// has to keep what this build does not understand. A newer caco's fields, and an
/// entry this one finds malformed, may matter to whoever wrote them.
#[derive(Debug, Clone, PartialEq)]
pub struct MirrorIndex {
    doc: Map<String, Value>,
}

impl Default for MirrorIndex {
    fn default() -> Self {
        let mut doc = Map::new();
        doc.insert("version".to_string(), INDEX_VERSION.into());
        doc.insert("files".to_string(), Value::Array(Vec::new()));
        Self { doc }
    }
}

impl MirrorIndex {
    /// Parse an index. A format newer than this build is refused rather than
    /// guessed at, since its entries may not mean what they look like they mean.
    pub fn parse(text: &str) -> crate::Result<Self> {
        let Value::Object(mut doc) = serde_json::from_str(text)? else {
            return Err(crate::Error::Config(
                "the index is not a JSON object".into(),
            ));
        };
        let version = doc.get("version").and_then(Value::as_u64).unwrap_or(1);
        if version > INDEX_VERSION {
            return Err(crate::Error::Config(format!(
                "the index is format {version}, and this version of caco reads up to \
                 {INDEX_VERSION}"
            )));
        }
        match doc
            .entry("files")
            .or_insert_with(|| Value::Array(Vec::new()))
        {
            Value::Array(_) => Ok(Self { doc }),
            _ => Err(crate::Error::Config(
                "the index's files are not a list".into(),
            )),
        }
    }

    fn files(&self) -> &[Value] {
        self.doc
            .get("files")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// The entries that can be used, and how many could not be.
    pub fn entries(&self) -> (Vec<MirrorEntry>, usize) {
        let files = self.files();
        let entries: Vec<MirrorEntry> = files.iter().filter_map(parse_entry).collect();
        let skipped = files.len() - entries.len();
        (entries, skipped)
    }

    /// The entry for the file with this hash.
    pub fn entry(&self, sha256: &str) -> Option<MirrorEntry> {
        self.files()
            .iter()
            .filter_map(parse_entry)
            .find(|entry| entry.sha256.eq_ignore_ascii_case(sha256))
    }

    /// Add `entry`, or if its file is here already, add what it knows about it.
    pub fn upsert(&mut self, entry: MirrorEntry) {
        let Some(Value::Array(files)) = self.doc.get_mut("files") else {
            return;
        };
        let same_file = |value: &Value| {
            value
                .get("sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&entry.sha256))
        };
        let Some(position) = files.iter().position(same_file) else {
            files.push(serde_json::to_value(entry).expect("an entry serializes"));
            return;
        };
        let merged = match parse_entry(&files[position]) {
            Some(existing) => entry.merged_over(existing),
            None => entry,
        };
        let Value::Object(fields) = serde_json::to_value(merged).expect("an entry serializes")
        else {
            unreachable!("an entry serializes to an object");
        };
        match &mut files[position] {
            // Field by field, so fields this build does not know stay on the entry.
            Value::Object(existing) => existing.extend(fields),
            other => *other = Value::Object(fields),
        }
    }

    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(&self.doc).expect("a JSON document serializes");
        text.push('\n');
        text
    }
}

fn parse_entry(value: &Value) -> Option<MirrorEntry> {
    MirrorEntry::deserialize(value).ok()?.validated()
}

/// A SHA-256 in lowercase hex, or `None` if this is not one.
fn normal_hash(text: &str) -> Option<String> {
    (text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

/// Whether `name` is a file name and nothing more: no directories, nothing that
/// climbs out of one, nothing a filesystem will not store.
pub fn is_bare_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
}

/// A name for `filename` that a store fussy about names will take: the characters
/// Gitea's documentation promises it accepts, with `_` for anything else.
///
/// Only used once a mirror has refused the real one, so a mirror that can hold a
/// file under its own name does.
pub fn safe_stored_name(filename: &str) -> String {
    filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The index entry for a WAD's file, saying everything the library knows about it.
///
/// Identifiers come from the WAD first and then from the Cacoward rows linked to
/// it, because each is more for another machine's row to be recognised by: that
/// row may have come from the forum thread when this one came from a local file.
pub fn entry_for_wad(
    conn: &Connection,
    wad: &WadRecord,
    identity: &FileIdentity,
    filename: &str,
    added: &str,
) -> crate::Result<MirrorEntry> {
    let source_url = wad.source_url.as_deref().unwrap_or("");
    let mut entry = MirrorEntry {
        sha256: identity.file.sha256.clone(),
        size: identity.file.size,
        filename: filename.to_string(),
        stored_as: None,
        content_sha256: Some(identity.content_sha256.clone()),
        title: Some(wad.title.clone()),
        author: wad
            .author
            .clone()
            .filter(|author| !author.trim().is_empty()),
        idgames: wad
            .idgames_id
            .as_deref()
            .and_then(|id| id.trim().parse().ok()),
        doomworld: extract_topic_id_from_url(source_url),
        doomwiki: doomwiki_page_from_url(source_url),
        cacowards: Vec::new(),
        added: Some(added.to_string()),
    };
    for row in db::cacowards::cacowards_for_wad(conn, wad.id)? {
        entry.idgames = entry.idgames.or(row.idgames_id);
        entry.doomworld = entry.doomworld.or(row.doomworld_id);
        entry.doomwiki = entry
            .doomwiki
            .or_else(|| row.doomwiki_page.as_deref().map(canonical_page));
        entry.cacowards.push(row.key);
    }
    Ok(entry)
}

/// The mirrors to offer `wad`'s file to, now that caco has it.
///
/// Empty unless the WAD is manual-only and there is a file to upload, since that
/// is the only case where a copy on a mirror changes anything: an idgames WAD can
/// be fetched again as it is, and a WAD a mirror already holds reads as automatic.
pub fn offer_targets(wad: &WadRecord, mirrors: &[MirrorConfig]) -> Vec<String> {
    let has_file = wad.cached_path.as_deref().is_some_and(|p| !p.is_empty());
    if !has_file || wad.retrievability().is_automatic() {
        return Vec::new();
    }
    mirrors
        .iter()
        .filter(|mirror| mirror.write)
        .map(|mirror| mirror.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewCacoward, NewWad, SourceType, WadUpdate};
    use crate::identity::Hashes;

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn entry(sha256: &str, filename: &str) -> MirrorEntry {
        MirrorEntry {
            sha256: sha256.to_string(),
            size: 4,
            filename: filename.to_string(),
            ..Default::default()
        }
    }

    fn index_with(files: Value) -> MirrorIndex {
        MirrorIndex::parse(&serde_json::json!({ "version": 1, "files": files }).to_string())
            .unwrap()
    }

    #[test]
    fn test_a_new_index_round_trips() {
        let mut index = MirrorIndex::default();
        index.upsert(MirrorEntry {
            title: Some("Lonesome Road".to_string()),
            doomworld: Some(151858),
            cacowards: vec!["2025-winner-lonesome-road".to_string()],
            ..entry(HASH_A, "lonesome.zip")
        });

        let again = MirrorIndex::parse(&index.to_json()).unwrap();

        assert_eq!(again, index);
        let (entries, skipped) = again.entries();
        assert_eq!(skipped, 0);
        assert_eq!(entries[0].filename, "lonesome.zip");
        assert_eq!(entries[0].doomworld, Some(151858));
    }

    /// The issue's example: an index that only says what it knows is enough.
    #[test]
    fn test_an_entry_only_needs_its_hash_size_and_name() {
        let index = index_with(serde_json::json!([
            { "sha256": HASH_A, "size": 4, "filename": "a.wad" }
        ]));
        assert_eq!(index.entries().0, [entry(HASH_A, "a.wad")]);
    }

    #[test]
    fn test_a_missing_version_or_file_list_is_an_empty_first_version() {
        assert_eq!(MirrorIndex::parse("{}").unwrap().entries().0, []);
    }

    #[test]
    fn test_a_newer_format_is_refused_rather_than_guessed_at() {
        let failure = MirrorIndex::parse(r#"{"version": 2, "files": []}"#).unwrap_err();
        assert!(failure.to_string().contains("format 2"), "{failure}");
    }

    #[test]
    fn test_something_else_entirely_is_refused() {
        // What a misconfigured server is likeliest to answer with.
        assert!(MirrorIndex::parse("<html>Just a moment...</html>").is_err());
        assert!(MirrorIndex::parse("[]").is_err());
        assert!(MirrorIndex::parse(r#"{"files": 3}"#).is_err());
    }

    /// `filename` becomes a path on this machine.
    #[test]
    fn test_entries_that_could_escape_a_directory_are_skipped() {
        for bad in [
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": "../../.bashrc" }),
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": "/etc/passwd" }),
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": "..\\evil.wad" }),
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": ".." }),
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": "" }),
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": "a.wad",
                                "stored_as": "../a.wad" }),
            serde_json::json!({ "sha256": "../../etc", "size": 4, "filename": "a.wad" }),
            serde_json::json!({ "sha256": "abc", "size": 4, "filename": "a.wad" }),
            serde_json::json!({ "sha256": HASH_A, "size": 4, "filename": "a.wad",
                                "content_sha256": "nope" }),
            serde_json::json!({ "sha256": HASH_A, "size": -1, "filename": "a.wad" }),
            serde_json::json!({ "sha256": HASH_A, "filename": "a.wad" }),
            serde_json::json!("not an object"),
        ] {
            let index = index_with(serde_json::json!([bad.clone()]));
            assert_eq!(index.entries(), (Vec::new(), 1), "{bad}");
        }
    }

    #[test]
    fn test_a_bad_entry_does_not_sink_the_rest_of_the_index() {
        let index = index_with(serde_json::json!([
            { "sha256": "abc", "size": 4, "filename": "bad.wad" },
            { "sha256": HASH_B, "size": 4, "filename": "good.wad" },
        ]));
        assert_eq!(index.entries(), (vec![entry(HASH_B, "good.wad")], 1));
    }

    #[test]
    fn test_hashes_are_read_in_lowercase() {
        let index = index_with(serde_json::json!([
            { "sha256": HASH_A.to_ascii_uppercase(), "size": 4, "filename": "a.wad" },
        ]));
        assert_eq!(index.entries().0[0].sha256, HASH_A);
        assert!(index.entry(HASH_A).is_some());
    }

    #[test]
    fn test_file_names_keep_what_real_wads_are_called() {
        for good in [
            "DATW Ver 1.8.wad",
            "Purgatory V1.1.PK3",
            "Santa's.wad",
            "Piña.zip",
        ] {
            assert!(is_bare_file_name(good), "{good}");
        }
    }

    /// Writing back must not take away what someone else wrote.
    #[test]
    fn test_rewriting_an_index_keeps_what_this_build_does_not_understand() {
        let mut index = MirrorIndex::parse(
            &serde_json::json!({
                "version": 1,
                "generator": "someone's script",
                "files": [
                    { "sha256": "abc", "filename": "from-the-future.wad", "shiny": true },
                    { "sha256": HASH_A, "size": 4, "filename": "a.wad", "shiny": true },
                ],
            })
            .to_string(),
        )
        .unwrap();

        index.upsert(MirrorEntry {
            title: Some("A".to_string()),
            ..entry(HASH_A, "a.wad")
        });
        index.upsert(entry(HASH_B, "b.wad"));

        let written: Value = serde_json::from_str(&index.to_json()).unwrap();
        assert_eq!(written["generator"], "someone's script");
        assert_eq!(written["files"][0]["filename"], "from-the-future.wad");
        assert_eq!(written["files"][1]["shiny"], true);
        assert_eq!(written["files"][1]["title"], "A");
        assert_eq!(written["files"][2]["filename"], "b.wad");
    }

    #[test]
    fn test_uploading_a_file_again_adds_to_what_is_known_about_it() {
        let mut index = MirrorIndex::default();
        index.upsert(MirrorEntry {
            idgames: Some(1),
            cacowards: vec!["2024-winner-a".to_string()],
            added: Some("2026-01-01T00:00:00Z".to_string()),
            ..entry(HASH_A, "first-name.wad")
        });

        index.upsert(MirrorEntry {
            doomworld: Some(2),
            cacowards: vec!["2024-winner-a".to_string(), "2024-mordeth-a".to_string()],
            added: Some("2026-06-01T00:00:00Z".to_string()),
            ..entry(HASH_A, "second-name.wad")
        });

        let (entries, _) = index.entries();
        assert_eq!(entries.len(), 1);
        let merged = &entries[0];
        assert_eq!(merged.idgames, Some(1));
        assert_eq!(merged.doomworld, Some(2));
        assert_eq!(merged.cacowards, ["2024-winner-a", "2024-mordeth-a"]);
        // The name and the date describe the file already on the mirror.
        assert_eq!(merged.filename, "first-name.wad");
        assert_eq!(merged.added.as_deref(), Some("2026-01-01T00:00:00Z"));
    }

    /// The one way a file's own facts change: it had to go back under a safe name.
    #[test]
    fn test_a_new_stored_name_replaces_the_old_one() {
        let mut index = MirrorIndex::default();
        index.upsert(entry(HASH_A, "Santa's.wad"));
        index.upsert(MirrorEntry {
            stored_as: Some("Santa_s.wad".to_string()),
            ..entry(HASH_A, "Santa's.wad")
        });
        let merged = &index.entries().0[0];
        assert_eq!(merged.stored_name(), "Santa_s.wad");
        assert_eq!(merged.filename, "Santa's.wad");

        // And uploading it again knows nothing that changes that.
        index.upsert(entry(HASH_A, "Santa's.wad"));
        assert_eq!(index.entries().0[0].stored_name(), "Santa_s.wad");
    }

    #[test]
    fn test_a_file_is_found_under_the_name_it_was_stored_as() {
        let stored = MirrorEntry {
            stored_as: Some("Santa_s.wad".to_string()),
            ..entry(HASH_A, "Santa's.wad")
        };
        assert_eq!(stored.stored_path(), format!("{HASH_A}/Santa_s.wad"));
        assert_eq!(
            entry(HASH_A, "plain.wad").stored_path(),
            format!("{HASH_A}/plain.wad")
        );
    }

    /// Everything Gitea 28.3 was seen to refuse, checked against the container
    /// these rules came from, plus what its documentation says it refuses.
    #[test]
    fn test_a_safe_name_avoids_what_a_registry_refuses() {
        assert_eq!(
            safe_stored_name("Santa's Bender, v2.wad"),
            "Santa_s_Bender__v2.wad"
        );
        assert_eq!(safe_stored_name("Piña Colada.zip"), "Pi_a_Colada.zip");
        assert_eq!(
            safe_stored_name("plain-1.2+fix_a.wad"),
            "plain-1.2+fix_a.wad"
        );
    }

    fn wad_with_file(conn: &Connection, new: NewWad) -> WadRecord {
        let id = db::add_wad(conn, &new.cached_path("/keep/file.wad")).unwrap();
        db::get_wad(conn, id, false).unwrap().unwrap()
    }

    fn setup() -> Connection {
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();
        conn
    }

    fn identity() -> FileIdentity {
        FileIdentity {
            file: Hashes {
                size: 4,
                sha256: HASH_A.to_string(),
                sha1: "c".repeat(40),
            },
            content_sha256: HASH_B.to_string(),
        }
    }

    #[test]
    fn test_an_entry_says_what_the_wad_knows() {
        let conn = setup();
        let wad = wad_with_file(
            &conn,
            NewWad::new("Lonesome Road", SourceType::Doomworld)
                .author("tourniquet")
                .source_url("https://www.doomworld.com/vb/thread/151858"),
        );

        let entry =
            entry_for_wad(&conn, &wad, &identity(), "road.zip", "2026-11-03T00:00:00Z").unwrap();

        assert_eq!(entry.sha256, HASH_A);
        assert_eq!(entry.size, 4);
        assert_eq!(entry.filename, "road.zip");
        assert_eq!(entry.content_sha256.as_deref(), Some(HASH_B));
        assert_eq!(entry.title.as_deref(), Some("Lonesome Road"));
        assert_eq!(entry.author.as_deref(), Some("tourniquet"));
        assert_eq!(entry.doomworld, Some(151858));
        assert_eq!(entry.idgames, None);
        assert_eq!(entry.added.as_deref(), Some("2026-11-03T00:00:00Z"));
    }

    /// A local file linked to its award is known by the award's identifiers, and
    /// those are what another machine's row imported from the thread will have.
    #[test]
    fn test_an_entry_borrows_identifiers_from_the_awards_it_won() {
        let conn = setup();
        let wad = wad_with_file(&conn, NewWad::new("Road", SourceType::Local));
        for (key, doomworld, doomwiki) in [
            ("2025-winner-road", Some(151858), Some("Lonesome_Road")),
            ("2025-mordeth-road", Some(999), None),
        ] {
            let row = db::upsert_cacoward(
                &conn,
                &NewCacoward {
                    key: key.to_string(),
                    year: 2025,
                    category: "winner".to_string(),
                    page_order: 1,
                    wad_title: "Road".to_string(),
                    doomworld_id: doomworld,
                    doomwiki_page: doomwiki.map(str::to_string),
                    ..Default::default()
                },
            )
            .unwrap();
            db::link_wad(&conn, row, wad.id, true).unwrap();
        }

        let entry = entry_for_wad(&conn, &wad, &identity(), "road.zip", "now").unwrap();

        assert_eq!(entry.cacowards.len(), 2);
        assert!(entry.cacowards.contains(&"2025-winner-road".to_string()));
        assert!(entry.doomworld.is_some());
        assert_eq!(entry.doomwiki.as_deref(), Some("Lonesome Road"));
    }

    fn mirror(name: &str, write: bool) -> MirrorConfig {
        MirrorConfig {
            name: name.to_string(),
            path: Some("/mnt/nas/doom".to_string()),
            write,
            ..Default::default()
        }
    }

    #[test]
    fn test_a_manual_wad_with_a_file_is_offered_to_every_writable_mirror() {
        let conn = setup();
        let wad = wad_with_file(&conn, NewWad::new("Manual", SourceType::Doomworld));
        let mirrors = [
            mirror("home", true),
            mirror("read-only", false),
            mirror("nas", true),
        ];
        assert_eq!(offer_targets(&wad, &mirrors), ["home", "nas"]);
    }

    #[test]
    fn test_nothing_to_offer_without_a_writable_mirror() {
        let conn = setup();
        let wad = wad_with_file(&conn, NewWad::new("Manual", SourceType::Doomworld));
        assert!(offer_targets(&wad, &[]).is_empty());
        assert!(offer_targets(&wad, &[mirror("read-only", false)]).is_empty());
    }

    /// idgames can already give this one back.
    #[test]
    fn test_an_automatic_wad_is_not_offered() {
        let conn = setup();
        let wad = wad_with_file(
            &conn,
            NewWad::new("On idgames", SourceType::Idgames).idgames_id("7"),
        );
        assert!(offer_targets(&wad, &[mirror("home", true)]).is_empty());
    }

    #[test]
    fn test_a_wad_with_no_file_here_is_not_offered() {
        let conn = setup();
        for path in [None, Some(String::new())] {
            let id = db::add_wad(&conn, &NewWad::new("Gone", SourceType::Doomworld)).unwrap();
            let update = WadUpdate::new().set_text("cached_path", path);
            db::update_wad(&conn, id, &update).unwrap();
            let wad = db::get_wad(&conn, id, false).unwrap().unwrap();
            assert!(offer_targets(&wad, &[mirror("home", true)]).is_empty());
        }
    }
}

//! Notice when an installed sourceport has fallen behind its recipe's ref.
//!
//! A recipe usually tracks a branch, so "is there a new version" is really
//! "does the remote ref still point at the commit we built". `git ls-remote`
//! answers that in one round trip without fetching an object, which is what
//! makes this cheap enough to run at startup.
//!
//! The answer is cached and throttled, for two reasons: a sourceport that updates
//! weekly does not need to be asked about on every launch, and a machine
//! with no network must not pay a DNS timeout per sourceport before the window
//! appears. The cache lives in the sourceports cache directory — it is a fact about
//! a remote, so losing it costs one more round trip.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use super::build::SourceportPaths;
use super::manifest::list_installed;

/// Filename of the check cache, inside the prefix root.
///
/// A plain file there is invisible to [`list_installed`], which only
/// descends into directories that carry a manifest.
const CACHE_NAME: &str = "update-check.toml";

/// Where one installed sourceport stands against its remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    pub name: String,
    pub git_ref: String,
    /// Commit the installed build was made from.
    pub installed_commit: String,
    /// Commit the ref points at now, or `None` if the remote could not be
    /// reached and nothing was cached.
    pub remote_commit: Option<String>,
}

/// Branches and release tags advertised by a sourceport's remote.
///
/// This is deliberately not populated by [`status`](super::status): listing
/// every ref can be noticeably slower than rendering the Sourceports dialog. The
/// frontend asks for it only when the user clicks "Check versions".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteRefs {
    pub branches: Vec<String>,
    /// Tags in version-like descending order, so the first item is the best
    /// generic-git approximation of "latest release".
    pub releases: Vec<String>,
}

impl RemoteRefs {
    pub fn latest_release(&self) -> Option<&str> {
        self.releases.first().map(String::as_str)
    }
}

impl UpdateStatus {
    /// Whether a rebuild would get something new.
    ///
    /// A build whose commit was never resolved (`"unknown"`, from a checkout
    /// git would not answer for) is not reported as behind: it would light up
    /// on every launch with a rebuild that could not clear it.
    pub fn is_behind(&self) -> bool {
        match &self.remote_commit {
            Some(remote) => self.installed_commit != "unknown" && remote != &self.installed_commit,
            None => false,
        }
    }

    /// Short form of the remote commit, for display.
    pub fn short_remote(&self) -> String {
        self.remote_commit
            .as_deref()
            .unwrap_or("unknown")
            .chars()
            .take(9)
            .collect()
    }
}

/// What a previous check found, per sourceport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedCheck {
    /// RFC 3339 timestamp of the check.
    pub checked_at: String,
    pub remote_commit: String,
}

/// Remote state as of the last check.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateCache {
    #[serde(default)]
    pub sourceports: BTreeMap<String, CachedCheck>,
}

pub fn cache_path(prefix_root: &Path) -> PathBuf {
    prefix_root.join(CACHE_NAME)
}

/// Read the check cache. A missing or unreadable file is an empty cache —
/// the worst that costs is one more round trip.
pub fn load_cache(prefix_root: &Path) -> UpdateCache {
    std::fs::read_to_string(cache_path(prefix_root))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_cache(prefix_root: &Path, cache: &UpdateCache) -> crate::Result<()> {
    std::fs::create_dir_all(prefix_root)?;
    std::fs::write(cache_path(prefix_root), toml::to_string_pretty(cache)?)?;
    Ok(())
}

/// Whether `name` should be asked about again.
///
/// `interval_days <= 0` disables checking entirely, which is the setting for
/// someone who does not want caco touching the network at startup.
pub fn is_due(cache: &UpdateCache, name: &str, now: DateTime<Local>, interval_days: i64) -> bool {
    if interval_days <= 0 {
        return false;
    }
    let Some(entry) = cache.sourceports.get(name) else {
        return true;
    };
    // An unparseable timestamp means the cache was hand-edited or written by
    // a different version: re-check rather than trust it forever.
    let Ok(checked) = DateTime::parse_from_rfc3339(&entry.checked_at) else {
        return true;
    };
    now.signed_duration_since(checked).num_days() >= interval_days
}

/// Ask a remote what `git_ref` points at, without fetching any objects.
pub fn remote_commit(repo: &str, git_ref: &str) -> crate::Result<String> {
    let output = Command::new("git")
        .args(["ls-remote", repo, git_ref])
        .output()
        .map_err(|e| crate::error::Error::Config(format!("git ls-remote failed: {e}")))?;
    if !output.status.success() {
        return Err(crate::error::Error::Config(format!(
            "git ls-remote {repo} {git_ref}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    parse_ls_remote(&String::from_utf8_lossy(&output.stdout), git_ref)
        .ok_or_else(|| crate::error::Error::Config(format!("remote has no ref '{git_ref}'")))
}

/// Lazily list the branches and tags a sourceport can be built from.
///
/// This uses git rather than a hosting-provider API, so custom recipes on
/// GitLab, Codeberg or a private server get the same version picker as GitHub
/// recipes. In that provider-neutral model, a release is a git tag.
pub fn remote_refs(repo: &str) -> crate::Result<RemoteRefs> {
    let output = Command::new("git")
        .args(["ls-remote", "--heads", "--tags", repo])
        .output()
        .map_err(|e| crate::error::Error::Config(format!("git ls-remote failed: {e}")))?;
    if !output.status.success() {
        return Err(crate::error::Error::Config(format!(
            "git ls-remote {repo}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(parse_remote_refs(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_remote_refs(stdout: &str) -> RemoteRefs {
    let mut branches = BTreeSet::new();
    let mut releases = BTreeSet::new();
    for line in stdout.lines() {
        let Some((_, full_ref)) = line.split_once('\t') else {
            continue;
        };
        if let Some(name) = full_ref.strip_prefix("refs/heads/") {
            branches.insert(name.to_string());
        } else if let Some(name) = full_ref.strip_prefix("refs/tags/") {
            // Annotated tags appear twice; their peeled commit ends in ^{}.
            releases.insert(name.trim_end_matches("^{}").to_string());
        }
    }
    let mut releases: Vec<_> = releases.into_iter().collect();
    releases.sort_by(|a, b| versionish_cmp(b, a));
    RemoteRefs {
        branches: branches.into_iter().collect(),
        releases,
    }
}

/// Compare names in the way people expect version tags to sort: numeric runs
/// compare numerically, while everything else remains deterministic. This
/// handles `v4.14.3` without imposing semver rules on projects that tag as
/// `release-2026-08` or use some other convention.
fn versionish_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.as_bytes().iter().copied().peekable();
    let mut right = b.as_bytes().iter().copied().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut ln = Vec::new();
                let mut rn = Vec::new();
                while left.peek().is_some_and(u8::is_ascii_digit) {
                    ln.push(left.next().unwrap());
                }
                while right.peek().is_some_and(u8::is_ascii_digit) {
                    rn.push(right.next().unwrap());
                }
                let ltrim = ln.iter().position(|c| *c != b'0').unwrap_or(ln.len());
                let rtrim = rn.iter().position(|c| *c != b'0').unwrap_or(rn.len());
                let lnum = &ln[ltrim..];
                let rnum = &rn[rtrim..];
                match lnum.len().cmp(&rnum.len()).then_with(|| lnum.cmp(rnum)) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                left.next();
                right.next();
                match x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

/// Pick the commit for `git_ref` out of `git ls-remote` output.
///
/// An annotated tag yields two lines: the tag object and, suffixed `^{}`, the
/// commit it points at. The second is what a checkout resolves `HEAD` to, so
/// it is the one to compare a manifest against. `ls-remote` also matches a
/// pattern against the tail of a refname, so `master` can bring back
/// `refs/heads/topic/master` too — exact refnames are tried first.
fn parse_ls_remote(stdout: &str, git_ref: &str) -> Option<String> {
    let rows: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(sha, name)| (sha.trim(), name.trim()))
        .filter(|(sha, _)| !sha.is_empty())
        .collect();

    for wanted in [
        format!("refs/tags/{git_ref}^{{}}"),
        format!("refs/heads/{git_ref}"),
        format!("refs/tags/{git_ref}"),
        git_ref.to_string(),
    ] {
        if let Some((sha, _)) = rows.iter().find(|(_, name)| *name == wanted) {
            return Some((*sha).to_string());
        }
    }
    // Unambiguous single match: take it.
    if rows.len() == 1 {
        return Some(rows[0].0.to_string());
    }
    None
}

/// Check every installed sourceport against its recipe's remote.
///
/// Returns a status per installed sourceport, in name order — the caller decides
/// what to do with the ones that are behind. Sourceports whose check is not yet due
/// are answered from the cache, so this is free on most launches.
///
/// A remote that cannot be reached is not an error: the sourceport keeps whatever
/// the cache last knew, or reports `None`, and the next run tries again.
pub fn check_updates(
    paths: &SourceportPaths,
    interval_days: i64,
    now: DateTime<Local>,
) -> Vec<UpdateStatus> {
    let mut cache = load_cache(&paths.prefix_root);
    let mut changed = false;
    let mut statuses = Vec::new();
    let mut seen = BTreeSet::new();

    for installed in list_installed(&paths.prefix_root) {
        let name = installed.manifest.name.clone();
        // Several refs may be installed side by side. list_installed is
        // newest-first, and that is also the build managed_binary launches,
        // so only check that active install. Track the ref it was actually
        // built from: a release chosen in the UI must not be compared to the
        // recipe's development branch and reported as perpetually behind.
        if !installed.is_usable() || !seen.insert(name.clone()) {
            continue;
        }
        let repo = installed.manifest.repo.clone();
        let git_ref = installed.manifest.git_ref.clone();

        let remote = if is_due(&cache, &name, now, interval_days) {
            match remote_commit(&repo, &git_ref) {
                Ok(commit) => {
                    cache.sourceports.insert(
                        name.clone(),
                        CachedCheck {
                            checked_at: now.to_rfc3339(),
                            remote_commit: commit.clone(),
                        },
                    );
                    changed = true;
                    Some(commit)
                }
                // Offline, or the ref is gone. Fall back to what we knew.
                Err(_) => cache
                    .sourceports
                    .get(&name)
                    .map(|c| c.remote_commit.clone()),
            }
        } else {
            cache
                .sourceports
                .get(&name)
                .map(|c| c.remote_commit.clone())
        };

        statuses.push(UpdateStatus {
            name,
            git_ref,
            installed_commit: installed.manifest.commit.clone(),
            remote_commit: remote,
        });
    }

    if changed {
        let _ = save_cache(&paths.prefix_root, &cache);
    }
    statuses.sort_by(|a, b| a.name.cmp(&b.name));
    statuses
}

/// Forget the cached check for a sourceport, so the next run asks the remote again.
///
/// Called after a build: the freshly recorded commit is the answer, and
/// leaving a stale entry would keep the update badge lit until the interval
/// expired.
pub fn invalidate(prefix_root: &Path, name: &str) {
    let mut cache = load_cache(prefix_root);
    if cache.sourceports.remove(name).is_some() {
        let _ = save_cache(prefix_root, &cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sourceports::manifest::{SourceportManifest, write_manifest};

    fn now() -> DateTime<Local> {
        DateTime::parse_from_rfc3339("2026-08-10T12:00:00+00:00")
            .unwrap()
            .into()
    }

    fn cache_with(name: &str, checked_at: &str, commit: &str) -> UpdateCache {
        let mut cache = UpdateCache::default();
        cache.sourceports.insert(
            name.to_string(),
            CachedCheck {
                checked_at: checked_at.to_string(),
                remote_commit: commit.to_string(),
            },
        );
        cache
    }

    fn paths(root: &Path) -> SourceportPaths {
        SourceportPaths {
            recipe_dir: root.join("recipes"),
            src_root: root.join("src"),
            prefix_root: root.join("prefix"),
        }
    }

    fn install(root: &Path, name: &str, commit: &str) {
        install_ref(root, name, "master", commit, "2026-01-01T00:00:00+00:00");
    }

    fn install_ref(root: &Path, name: &str, git_ref: &str, commit: &str, built_at: &str) {
        let prefix = root
            .join("prefix")
            .join(name)
            .join(crate::sourceports::recipe::slugify_ref(git_ref));
        write_manifest(
            &prefix,
            &SourceportManifest {
                name: name.to_string(),
                repo: "https://example.invalid/x".to_string(),
                git_ref: git_ref.to_string(),
                commit: commit.to_string(),
                binary: name.to_string(),
                built_at: built_at.to_string(),
            },
        )
        .unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin").join(name), b"x").unwrap();
    }

    // -- ls-remote parsing -------------------------------------------------

    #[test]
    fn a_branch_resolves_to_its_head() {
        let out = "abc123\trefs/heads/master\n";
        assert_eq!(parse_ls_remote(out, "master").as_deref(), Some("abc123"));
    }

    #[test]
    fn an_annotated_tag_resolves_to_the_commit_not_the_tag_object() {
        // The tag object's own sha would never equal a manifest commit, so
        // taking the first line would report every tagged build as behind.
        let out = "tagobj\trefs/tags/v1.0.0\ncommitsha\trefs/tags/v1.0.0^{}\n";
        assert_eq!(parse_ls_remote(out, "v1.0.0").as_deref(), Some("commitsha"));
    }

    #[test]
    fn an_exact_refname_beats_a_tail_match() {
        // `ls-remote master` also matches refs/heads/topic/master.
        let out = "wrong\trefs/heads/topic/master\nright\trefs/heads/master\n";
        assert_eq!(parse_ls_remote(out, "master").as_deref(), Some("right"));
    }

    #[test]
    fn an_ambiguous_match_resolves_to_nothing() {
        let out = "a\trefs/heads/topic/master\nb\trefs/heads/other/master\n";
        assert_eq!(parse_ls_remote(out, "master"), None);
    }

    #[test]
    fn a_lone_match_is_taken_even_without_an_exact_refname() {
        let out = "onlysha\trefs/heads/topic/master\n";
        assert_eq!(parse_ls_remote(out, "master").as_deref(), Some("onlysha"));
    }

    #[test]
    fn empty_output_resolves_to_nothing() {
        assert_eq!(parse_ls_remote("", "master"), None);
    }

    #[test]
    fn remote_refs_separate_branches_and_deduplicate_annotated_tags() {
        let refs = parse_remote_refs(
            "a\trefs/heads/main\n\
             b\trefs/heads/release/4.0\n\
             c\trefs/tags/v4.9.0\n\
             d\trefs/tags/v4.10.0\n\
             e\trefs/tags/v4.10.0^{}\n",
        );
        assert_eq!(refs.branches, vec!["main", "release/4.0"]);
        assert_eq!(refs.releases, vec!["v4.10.0", "v4.9.0"]);
        assert_eq!(refs.latest_release(), Some("v4.10.0"));
    }

    #[test]
    fn release_sorting_accepts_non_semver_tags() {
        let refs = parse_remote_refs(
            "a\trefs/tags/release-2025-12\n\
             b\trefs/tags/release-2026-2\n\
             c\trefs/tags/release-2026-10\n",
        );
        assert_eq!(
            refs.releases,
            vec!["release-2026-10", "release-2026-2", "release-2025-12"]
        );
    }

    // -- throttling --------------------------------------------------------

    #[test]
    fn a_port_never_checked_is_due() {
        assert!(is_due(&UpdateCache::default(), "uzdoom", now(), 1));
    }

    #[test]
    fn a_check_inside_the_interval_is_not_due() {
        let cache = cache_with("uzdoom", "2026-08-10T06:00:00+00:00", "abc");
        assert!(!is_due(&cache, "uzdoom", now(), 1));
    }

    #[test]
    fn a_check_older_than_the_interval_is_due() {
        let cache = cache_with("uzdoom", "2026-08-01T12:00:00+00:00", "abc");
        assert!(is_due(&cache, "uzdoom", now(), 1));
    }

    #[test]
    fn a_zero_interval_disables_checking() {
        // The setting for "do not touch the network at startup".
        assert!(!is_due(&UpdateCache::default(), "uzdoom", now(), 0));
        assert!(!is_due(&UpdateCache::default(), "uzdoom", now(), -1));
    }

    #[test]
    fn an_unparseable_timestamp_forces_a_recheck() {
        let cache = cache_with("uzdoom", "yesterday", "abc");
        assert!(is_due(&cache, "uzdoom", now(), 1));
    }

    // -- status ------------------------------------------------------------

    #[test]
    fn a_differing_remote_commit_is_behind() {
        let status = UpdateStatus {
            name: "uzdoom".into(),
            git_ref: "master".into(),
            installed_commit: "aaa".into(),
            remote_commit: Some("bbb".into()),
        };
        assert!(status.is_behind());
    }

    #[test]
    fn a_matching_remote_commit_is_not_behind() {
        let status = UpdateStatus {
            name: "uzdoom".into(),
            git_ref: "master".into(),
            installed_commit: "aaa".into(),
            remote_commit: Some("aaa".into()),
        };
        assert!(!status.is_behind());
    }

    #[test]
    fn an_unresolvable_build_is_never_reported_as_behind() {
        // Otherwise it lights up every launch and no rebuild clears it.
        let status = UpdateStatus {
            name: "uzdoom".into(),
            git_ref: "master".into(),
            installed_commit: "unknown".into(),
            remote_commit: Some("bbb".into()),
        };
        assert!(!status.is_behind());
    }

    #[test]
    fn an_unreachable_remote_is_not_behind() {
        let status = UpdateStatus {
            name: "uzdoom".into(),
            git_ref: "master".into(),
            installed_commit: "aaa".into(),
            remote_commit: None,
        };
        assert!(!status.is_behind());
    }

    // -- cache round trip --------------------------------------------------

    #[test]
    fn the_cache_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_with("uzdoom", "2026-08-10T06:00:00+00:00", "abc");
        save_cache(dir.path(), &cache).unwrap();
        assert_eq!(
            load_cache(dir.path()).sourceports.get("uzdoom"),
            cache.sourceports.get("uzdoom")
        );
    }

    #[test]
    fn a_corrupt_cache_reads_as_empty_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(cache_path(dir.path()), "not toml =").unwrap();
        assert!(load_cache(dir.path()).sourceports.is_empty());
    }

    #[test]
    fn the_cache_file_is_not_mistaken_for_an_install() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        install(dir.path(), "uzdoom", "aaa");
        save_cache(&p.prefix_root, &cache_with("uzdoom", "x", "y")).unwrap();
        assert_eq!(list_installed(&p.prefix_root).len(), 1);
    }

    #[test]
    fn invalidate_drops_only_the_named_port() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = cache_with("uzdoom", "2026-08-10T06:00:00+00:00", "abc");
        cache.sourceports.insert(
            "nyan-doom".to_string(),
            CachedCheck {
                checked_at: "2026-08-10T06:00:00+00:00".to_string(),
                remote_commit: "def".to_string(),
            },
        );
        save_cache(dir.path(), &cache).unwrap();

        invalidate(dir.path(), "uzdoom");
        let after = load_cache(dir.path());
        assert!(!after.sourceports.contains_key("uzdoom"));
        assert!(after.sourceports.contains_key("nyan-doom"));
    }

    // -- driver ------------------------------------------------------------

    #[test]
    fn nothing_installed_means_no_network_and_no_statuses() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_updates(&paths(dir.path()), 1, now()).is_empty());
    }

    #[test]
    fn a_throttled_run_answers_from_cache_without_touching_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        install(dir.path(), "uzdoom", "aaa");
        // Checked an hour ago, remote had moved on. The repo URL is
        // unreachable, so a status at all proves nothing was fetched.
        save_cache(
            &p.prefix_root,
            &cache_with("uzdoom", "2026-08-10T11:00:00+00:00", "bbb"),
        )
        .unwrap();

        let statuses = check_updates(&p, 1, now());
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].remote_commit.as_deref(), Some("bbb"));
        assert!(statuses[0].is_behind());
    }

    #[test]
    fn a_disabled_interval_never_reaches_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        install(dir.path(), "uzdoom", "aaa");

        // No cache entry and checking off: the unreachable repo would make
        // this hang or error if a lookup were attempted.
        let statuses = check_updates(&p, 0, now());
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].remote_commit, None);
        assert!(!statuses[0].is_behind());
    }

    #[test]
    fn only_the_newest_usable_ref_is_checked_for_each_port() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        install_ref(
            dir.path(),
            "uzdoom",
            "trunk",
            "old",
            "2026-01-01T00:00:00+00:00",
        );
        install_ref(
            dir.path(),
            "uzdoom",
            "4.14.3",
            "new",
            "2026-02-01T00:00:00+00:00",
        );

        let statuses = check_updates(&p, 0, now());
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].git_ref, "4.14.3");
        assert_eq!(statuses[0].installed_commit, "new");
    }
}

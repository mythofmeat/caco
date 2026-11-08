//! Which versions of a sourceport there are to build: its releases, and the branch it is
//! developed on.
//!
//! A repository has far more refs than versions. uzdoom's has eighty-odd branches, from
//! feature work to a branch per old version, and two hundred tags, most of them GZDoom's and
//! ZDoom's from before the fork, along with `x-testing` and `x-preview`, which its CI moves to
//! whatever it built last. Offering every one of those was unreadable, and sorting them by name
//! made `x-testing` the latest release, which is how uzdoom came to be built from `trunk` by
//! default. What a person chooses between is the newest release, an older one, or the branch the
//! project is developed on, so that is all there is:
//!
//! - A **release** is a tag that is a version number, after a name if it has one: `5.0.3`,
//!   `v1.6.2` and `woof_15.2.0`, and not `5.0.0-rc.1`, `5.1.0-pre` or `x-testing`. Releases are
//!   ordered by number, so `5.0.10` is newer than `5.0.9`. A recipe's `first_release` leaves out
//!   the numbers that came with an older project's history: uzdoom's start at 4.14.3, and
//!   ZDoom's `2.8.1` and GZDoom's `g4.14.2` build no uzdoom.
//! - The **development branch** is the one the repository's `HEAD` names, `trunk` or `master`
//!   or `main`, so no recipe has to know which.
//!
//! It all comes from one `git ls-remote` rather than a hosting service's API, so a recipe on
//! GitLab, Codeberg or somebody's own server has the same choice as one on GitHub.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::manifest::SourceportManifest;
use super::recipe::SourceportRecipe;
use crate::error::Error;

/// A release's version number, and the tag it was read from.
///
/// Ordered by number, part by part, so `5.0.10` is newer than `5.0.9`. The tag breaks a tie,
/// so that `v1.2` and `1.2` still have an order and sorting never depends on where a tag was
/// in the listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReleaseNumber {
    tag: String,
    parts: Vec<u64>,
}

impl ReleaseNumber {
    /// The number `tag` ends in, if that is all that comes after its name: two or more
    /// numbers with dots between them, after anything without a digit in it.
    ///
    /// A single number is not enough, since `111` and `merged_in_1` are on uzdoom and are
    /// not releases. Anything after the number is not allowed either, which is what leaves
    /// out a release candidate.
    pub fn parse(tag: &str) -> Option<Self> {
        let start = tag.find(|c: char| c.is_ascii_digit())?;
        let parts = tag[start..]
            .split('.')
            .map(|part| {
                // `u64::from_str` would take a `+`, which no version is spelled with.
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                part.parse().ok()
            })
            .collect::<Option<Vec<u64>>>()?;
        (parts.len() >= 2).then(|| Self {
            tag: tag.to_string(),
            parts,
        })
    }

    /// Whether this is `first` or comes after it, by number alone: a floor of `4.14.3`
    /// lets `v4.14.3` in.
    fn is_at_least(&self, first: &ReleaseNumber) -> bool {
        self.parts >= first.parts
    }
}

impl Ord for ReleaseNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts
            .cmp(&other.parts)
            .then_with(|| self.tag.cmp(&other.tag))
    }
}

impl PartialOrd for ReleaseNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<String> for ReleaseNumber {
    type Error = String;

    fn try_from(tag: String) -> Result<Self, String> {
        Self::parse(&tag).ok_or_else(|| format!("{tag:?} is not a version number such as 4.14.3"))
    }
}

impl From<ReleaseNumber> for String {
    fn from(number: ReleaseNumber) -> String {
        number.tag
    }
}

/// Whether `tag` is one of a project's releases, given the first of them.
pub fn is_release(tag: &str, first_release: Option<&ReleaseNumber>) -> bool {
    ReleaseNumber::parse(tag)
        .is_some_and(|number| first_release.is_none_or(|first| number.is_at_least(first)))
}

/// Which version of a sourceport to build, as a person picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// The newest release, whichever that is when it is built, and every release after it is
    /// an update to the build. A project that has never made one is built from its development
    /// branch.
    LatestRelease,
    /// A branch or a tag, by name. New commits on a branch are an update to its build, and a
    /// tag stays where it is: an older release is one somebody chose to stay on.
    Named(String),
}

impl Version {
    /// What `manifest`'s build was made as, so that picking up where it left off rebuilds the
    /// same thing and an update check knows what would be new to it.
    ///
    /// A build from before caco wrote that down follows releases if it was made from one, since
    /// that is how it was treated then: every release build was told about newer ones.
    pub fn of(manifest: &SourceportManifest, first_release: Option<&ReleaseNumber>) -> Self {
        let follows_releases = manifest
            .follows_releases
            .unwrap_or_else(|| is_release(&manifest.git_ref, first_release));
        if follows_releases {
            Version::LatestRelease
        } else {
            Version::Named(manifest.git_ref.clone())
        }
    }
}

/// One version, as it is in the repository now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// What a person calls it, and what the build records: `5.0.3`, `trunk`.
    pub name: String,
    /// The ref to fetch, spelled out in full. uzdoom has a `4.14.3` tag and a `4.14.3` branch,
    /// and `git clone --branch 4.14.3` takes the branch where `git fetch origin 4.14.3` takes
    /// the tag, so a build of a short name could be one or the other depending on whether
    /// there was a checkout already.
    pub refname: String,
    /// The commit it is at. An annotated tag's is the commit it points at, not the tag object,
    /// since that is what a checkout of it ends up on.
    pub commit: String,
    /// Whether a newer release is an update to this build.
    pub follows_releases: bool,
}

/// What a sourceport's repository has to build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteVersions {
    development: Option<String>,
    /// Tags, newest first.
    releases: Vec<String>,
    /// Every branch and tag, by full refname, with the commit it is at.
    commits: BTreeMap<String, String>,
}

impl RemoteVersions {
    /// Read the output of [`ls_remote_args`].
    pub fn parse(stdout: &str, first_release: Option<&ReleaseNumber>) -> Self {
        let mut development = None;
        let mut commits = BTreeMap::new();
        for line in stdout.lines() {
            let Some((left, refname)) = line.split_once('\t') else {
                continue;
            };
            let refname = refname.trim();
            if let Some(points_at) = left.strip_prefix("ref: ") {
                if refname == "HEAD" {
                    development = points_at.strip_prefix("refs/heads/").map(str::to_string);
                }
                continue;
            }
            let sha = left.trim();
            if sha.is_empty() {
                continue;
            }
            if let Some(tag) = refname.strip_suffix("^{}") {
                // The commit an annotated tag points at. It comes after the line for the tag
                // object, and overrides it whichever order they arrive in.
                commits.insert(tag.to_string(), sha.to_string());
            } else if refname.starts_with("refs/heads/") || refname.starts_with("refs/tags/") {
                commits
                    .entry(refname.to_string())
                    .or_insert_with(|| sha.to_string());
            }
        }

        let mut releases: Vec<ReleaseNumber> = commits
            .keys()
            .filter_map(|refname| refname.strip_prefix("refs/tags/"))
            .filter_map(ReleaseNumber::parse)
            .filter(|number| first_release.is_none_or(|first| number.is_at_least(first)))
            .collect();
        releases.sort_by(|a, b| b.cmp(a));

        Self {
            development,
            releases: releases.into_iter().map(String::from).collect(),
            commits,
        }
    }

    /// The branch the project is developed on, if the repository says.
    pub fn development(&self) -> Option<&str> {
        self.development.as_deref()
    }

    pub fn latest_release(&self) -> Option<&str> {
        self.releases.first().map(String::as_str)
    }

    /// Every release but the latest, newest first.
    pub fn older_releases(&self) -> &[String] {
        self.releases.get(1..).unwrap_or_default()
    }

    /// What building `version` would check out now, or `None` if there is nothing by that
    /// name, or a project with no releases does not say where it is developed.
    pub fn resolve(&self, version: &Version) -> Option<Target> {
        match version {
            Version::LatestRelease => match self.latest_release() {
                Some(tag) => self.target(&format!("refs/tags/{tag}"), tag, true),
                None => self.resolve(&Version::Named(self.development.clone()?)),
            },
            // A tag before a branch, as `git fetch` takes a short name.
            Version::Named(name) => ["tags", "heads"]
                .iter()
                .find_map(|kind| self.target(&format!("refs/{kind}/{name}"), name, false)),
        }
    }

    fn target(&self, refname: &str, name: &str, follows_releases: bool) -> Option<Target> {
        Some(Target {
            name: name.to_string(),
            refname: refname.to_string(),
            commit: self.commits.get(refname)?.clone(),
            follows_releases,
        })
    }
}

/// `git ls-remote` for everything [`RemoteVersions`] reads: what `HEAD` points at, and every
/// branch and tag. Asking for no more than that matters on GitHub, which has a ref for every
/// pull request, and uzdoom has more than 800 of them.
pub fn ls_remote_args(repo: &str) -> Vec<String> {
    [
        "ls-remote",
        "--symref",
        repo,
        "HEAD",
        "refs/heads/*",
        "refs/tags/*",
    ]
    .map(String::from)
    .to_vec()
}

/// Ask `recipe`'s repository what there is to build. One round trip, which fetches nothing.
pub fn remote_versions(recipe: &SourceportRecipe) -> crate::Result<RemoteVersions> {
    let output = Command::new("git")
        .args(ls_remote_args(&recipe.repo))
        // A repository that is not there makes GitHub ask for a login, and git would ask for
        // it on whatever terminal caco was started from, and wait.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| Error::SourceportRemote(format!("git could not be run: {e}")))?;
    if !output.status.success() {
        return Err(Error::SourceportRemote(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(RemoteVersions::parse(
        &String::from_utf8_lossy(&output.stdout),
        recipe.first_release.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// uzdoom's repository as `ls-remote` showed it on 2026-11-08, cut down to one of each
    /// kind of thing in it.
    const UZDOOM: &str = "\
ref: refs/heads/trunk\tHEAD
2ca34a2909bdfa633df241ae08f2c3cd85d72c2a\tHEAD
b7b9c724369f162a1e4aa166676e07378fe7bc90\trefs/heads/2.6
1cb7598afadbe7ffe753b2eaf1b887e58e988036\trefs/heads/4.14.3
7910df7102033fd7754a317f3254f24101135926\trefs/heads/5.0
2ca34a2909bdfa633df241ae08f2c3cd85d72c2a\trefs/heads/trunk
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/vulkan
1111111111111111111111111111111111111111\trefs/tags/111
2222222222222222222222222222222222222222\trefs/tags/2.8.1
3333333333333333333333333333333333333333\trefs/tags/2.8.1^{}
4444444444444444444444444444444444444444\trefs/tags/4.10.0
1cb7598afadbe7ffe753b2eaf1b887e58e988036\trefs/tags/4.14.3
5555555555555555555555555555555555555555\trefs/tags/4.14.3-rc1
6666666666666666666666666666666666666666\trefs/tags/5.0.0
7777777777777777777777777777777777777777\trefs/tags/5.0.0-rc.3
8888888888888888888888888888888888888888\trefs/tags/5.0.0-rc.3^{}
9999999999999999999999999999999999999999\trefs/tags/5.0.1
abababababababababababababababababababab\trefs/tags/5.0.2
cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd\trefs/tags/5.0.3
7910df7102033fd7754a317f3254f24101135926\trefs/tags/5.0.3^{}
efefefefefefefefefefefefefefefefefefefef\trefs/tags/5.1.0-pre
bcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbc\trefs/tags/UZDoom-Begin
dededededededededededededededededededede\trefs/tags/g4.14.2
fafafafafafafafafafafafafafafafafafafafa\trefs/tags/x-testing
";

    fn uzdoom() -> RemoteVersions {
        RemoteVersions::parse(UZDOOM, ReleaseNumber::parse("4.14.3").as_ref())
    }

    fn manifest(git_ref: &str, follows_releases: Option<bool>) -> SourceportManifest {
        SourceportManifest {
            name: "uzdoom".into(),
            repo: "https://example.invalid/uzdoom".into(),
            git_ref: git_ref.into(),
            commit: "deadbeef".into(),
            follows_releases,
            binary: "uzdoom".into(),
            built_at: "2026-01-01T00:00:00+00:00".into(),
        }
    }

    // -- what a release is -------------------------------------------------

    #[test]
    fn a_version_number_is_a_release_after_a_name_or_without_one() {
        for tag in ["5.0.3", "v1.6.2", "V2.0", "4.14", "woof_15.2.0", "v.1.1.1"] {
            assert!(
                ReleaseNumber::parse(tag).is_some(),
                "{tag} should be a release"
            );
        }
    }

    #[test]
    fn anything_after_the_number_is_not_a_release() {
        for tag in [
            "5.0.0-rc.1",
            "4.14.3-rc1",
            "5.1.0-pre",
            "g4.15pre",
            "x-testing",
            "UZDoom-Begin",
            "111",
            "merged_in_1",
            "5.0.",
            "5..0",
            "5.+1",
            "",
        ] {
            assert!(
                ReleaseNumber::parse(tag).is_none(),
                "{tag} is not a release"
            );
        }
    }

    #[test]
    fn releases_are_ordered_by_number_not_by_name() {
        let n = |tag| ReleaseNumber::parse(tag).unwrap();
        assert!(n("5.0.10") > n("5.0.9"));
        assert!(n("5.0.0") > n("4.14.3"));
        assert!(n("v1.2.1.1") > n("v1.2.1"));
        // A tie on the number still has an order, so a sort is the same every time.
        assert_ne!(n("v1.2").cmp(&n("1.2")), Ordering::Equal);
    }

    #[test]
    fn the_first_release_is_compared_by_number_alone() {
        let first = ReleaseNumber::parse("4.14.3");
        assert!(is_release("v4.14.3", first.as_ref()));
        assert!(is_release("4.14.3", first.as_ref()));
        assert!(!is_release("4.14.2", first.as_ref()));
        assert!(is_release("2.8.1", None));
    }

    #[test]
    fn a_first_release_that_is_no_version_number_is_refused() {
        assert!(ReleaseNumber::try_from("trunk".to_string()).is_err());
        assert_eq!(
            String::from(ReleaseNumber::try_from("4.14.3".to_string()).unwrap()),
            "4.14.3"
        );
    }

    // -- reading a repository ----------------------------------------------

    #[test]
    fn uzdoom_offers_its_own_releases_newest_first_and_nothing_else() {
        let versions = uzdoom();
        // What its releases page has, and not ZDoom's 2.8.1, GZDoom's g4.14.2, a stray
        // 4.10.0 from before the fork, a candidate or what its CI moves around.
        assert_eq!(versions.latest_release(), Some("5.0.3"));
        assert_eq!(
            versions.older_releases(),
            ["5.0.2", "5.0.1", "5.0.0", "4.14.3"]
        );
        assert_eq!(versions.development(), Some("trunk"));
    }

    #[test]
    fn without_a_first_release_every_version_number_counts() {
        let versions = RemoteVersions::parse(UZDOOM, None);
        assert_eq!(versions.latest_release(), Some("5.0.3"));
        assert!(versions.older_releases().iter().any(|t| t == "2.8.1"));
        assert!(versions.older_releases().iter().any(|t| t == "g4.14.2"));
    }

    #[test]
    fn a_repository_that_does_not_say_where_it_is_developed_has_no_development_branch() {
        let without_head: String = UZDOOM
            .lines()
            .filter(|line| !line.ends_with("\tHEAD"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(
            RemoteVersions::parse(&without_head, None).development(),
            None
        );
    }

    // -- what a version resolves to ----------------------------------------

    #[test]
    fn the_latest_release_is_its_tag_and_follows_releases() {
        let target = uzdoom().resolve(&Version::LatestRelease).unwrap();
        assert_eq!(target.name, "5.0.3");
        assert_eq!(target.refname, "refs/tags/5.0.3");
        assert!(target.follows_releases);
    }

    #[test]
    fn an_annotated_tag_is_at_the_commit_it_points_at() {
        // The tag object's own sha would never equal a build's commit, so every build of an
        // annotated release would read as out of date.
        let target = uzdoom().resolve(&Version::LatestRelease).unwrap();
        assert_eq!(target.commit, "7910df7102033fd7754a317f3254f24101135926");
    }

    #[test]
    fn a_name_that_is_a_tag_and_a_branch_is_the_tag() {
        let target = uzdoom().resolve(&Version::Named("4.14.3".into())).unwrap();
        assert_eq!(target.refname, "refs/tags/4.14.3");
        assert!(
            !target.follows_releases,
            "an older release stays where it is"
        );
    }

    #[test]
    fn a_branch_is_its_head() {
        let target = uzdoom().resolve(&Version::Named("trunk".into())).unwrap();
        assert_eq!(target.refname, "refs/heads/trunk");
        assert_eq!(target.commit, "2ca34a2909bdfa633df241ae08f2c3cd85d72c2a");
        assert!(!target.follows_releases);
    }

    #[test]
    fn a_project_with_no_releases_builds_its_development_branch() {
        let versions = RemoteVersions::parse(
            "ref: refs/heads/main\tHEAD\nabc\tHEAD\nabc\trefs/heads/main\ndef\trefs/tags/nightly\n",
            None,
        );
        let target = versions.resolve(&Version::LatestRelease).unwrap();
        assert_eq!(target.name, "main");
        assert!(!target.follows_releases);
    }

    #[test]
    fn a_name_the_repository_does_not_have_resolves_to_nothing() {
        assert_eq!(uzdoom().resolve(&Version::Named("nope".into())), None);
        assert_eq!(
            RemoteVersions::default().resolve(&Version::LatestRelease),
            None
        );
    }

    // -- what a build was made as ------------------------------------------

    #[test]
    fn a_build_says_whether_it_follows_releases() {
        assert_eq!(
            Version::of(&manifest("5.0.3", Some(true)), None),
            Version::LatestRelease
        );
        assert_eq!(
            Version::of(&manifest("5.0.2", Some(false)), None),
            Version::Named("5.0.2".into())
        );
    }

    #[test]
    fn a_build_from_before_that_follows_releases_if_it_was_made_from_one() {
        let first = ReleaseNumber::parse("4.14.3");
        assert_eq!(
            Version::of(&manifest("5.0.3", None), first.as_ref()),
            Version::LatestRelease
        );
        assert_eq!(
            Version::of(&manifest("trunk", None), first.as_ref()),
            Version::Named("trunk".into())
        );
        assert_eq!(
            Version::of(&manifest("2.8.1", None), first.as_ref()),
            Version::Named("2.8.1".into())
        );
    }

    #[test]
    fn ls_remote_asks_for_head_branches_and_tags_and_no_more() {
        let args = ls_remote_args("https://example.invalid/x");
        assert_eq!(
            &args[..3],
            ["ls-remote", "--symref", "https://example.invalid/x"]
        );
        assert_eq!(&args[3..], ["HEAD", "refs/heads/*", "refs/tags/*"]);
    }
}

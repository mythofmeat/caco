//! Tokens pasted into caco for its mirrors, by mirror name.
//!
//! Setting up a mirror has to be doable from inside caco, and an environment variable
//! cannot be set from in there, so a token can be saved here instead. A file, the way
//! `cargo login` keeps its token, readable only by the user, and not the OS keychain:
//! that would mean a D-Bus service on a Linux desktop that may not be running one, and
//! a dependency in every package build, for something a user can always paste again.
//!
//! It is on neither side of the data split. The data dir travels between machines and
//! may be kept in a git repository, so a token there gets committed. The cache side is
//! for what caco can regenerate, which a token is not. So it has a place of its own,
//! [`crate::config::mirror_token_file`], and a second machine gets a token of its own.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const HEADER: &str = "# Tokens for your mirrors, by mirror name. Written by caco.\n\
                      # Not in the data directory, because that travels between machines and this should not.\n\n";

/// The file tokens are saved in.
///
/// The path is explicit for the reason [`crate::gc::GcPaths`] is: a test must not be
/// able to write into the real one.
#[derive(Debug, Clone)]
pub struct TokenFile {
    path: PathBuf,
}

impl TokenFile {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The real one. Frontends use this; tests must not.
    pub fn from_config() -> Self {
        Self::at(crate::config::mirror_token_file())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every saved token. A missing or unreadable file is no tokens: the mirror that
    /// needed one then says it was refused, which is a better place to find out.
    pub fn read(&self) -> HashMap<String, String> {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return HashMap::new();
        };
        let Ok(mut table) = text.parse::<toml::Table>() else {
            return HashMap::new();
        };
        let Some(toml::Value::Table(tokens)) = table.remove("tokens") else {
            return HashMap::new();
        };
        tokens
            .into_iter()
            .filter_map(|(name, token)| Some((name, token.as_str()?.to_string())))
            .collect()
    }

    /// Save a mirror's token, or forget it with `None`.
    pub fn set(&self, mirror: &str, token: Option<&str>) -> crate::Result<()> {
        let mut tokens = self.read();
        match token {
            Some(token) => tokens.insert(mirror.to_string(), token.to_string()),
            None => tokens.remove(mirror),
        };
        self.write(tokens)
    }

    /// Keep a token with its mirror when the mirror is renamed.
    pub fn rename(&self, from: &str, to: &str) -> crate::Result<()> {
        let mut tokens = self.read();
        let Some(token) = tokens.remove(from) else {
            return Ok(());
        };
        tokens.insert(to.to_string(), token);
        self.write(tokens)
    }

    /// Replace the file whole, which is what leaves a token readable by nobody else
    /// even if the file it replaces was: permissions are set on the new one before
    /// anything is written to it.
    fn write(&self, tokens: HashMap<String, String>) -> crate::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut table = toml::Table::new();
        table.insert(
            "tokens".to_string(),
            toml::Value::Table(
                tokens
                    .into_iter()
                    .map(|(name, token)| (name, toml::Value::String(token)))
                    .collect(),
            ),
        );
        let text = format!("{HEADER}{}", toml::to_string_pretty(&table)?);

        let tmp = self.path.with_extension("toml.partial");
        let _ = fs::remove_file(&tmp);
        let mut file = private_file(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// A new file only its owner can read.
#[cfg(unix)]
fn private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_in(dir: &tempfile::TempDir) -> TokenFile {
        TokenFile::at(dir.path().join("nested/mirror-tokens.toml"))
    }

    #[test]
    fn test_no_file_is_no_tokens() {
        let dir = tempfile::tempdir().unwrap();
        assert!(file_in(&dir).read().is_empty());
    }

    #[test]
    fn test_a_token_is_saved_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);

        file.set("home", Some("sekrit")).unwrap();
        file.set("nas box", Some("other")).unwrap();
        let tokens = file.read();
        assert_eq!(tokens.get("home").map(String::as_str), Some("sekrit"));
        assert_eq!(tokens.get("nas box").map(String::as_str), Some("other"));

        file.set("home", None).unwrap();
        let tokens = file.read();
        assert_eq!(tokens.get("home"), None);
        assert_eq!(tokens.get("nas box").map(String::as_str), Some("other"));
    }

    #[test]
    fn test_a_token_follows_its_mirror_when_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        file.set("old", Some("sekrit")).unwrap();

        file.rename("old", "new").unwrap();
        // Nothing saved under this name, and nothing to do.
        file.rename("never-had-one", "whatever").unwrap();

        let tokens = file.read();
        assert_eq!(tokens.get("new").map(String::as_str), Some("sekrit"));
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn test_a_file_that_does_not_parse_is_no_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        fs::write(file.path(), "not = [toml").unwrap();
        assert!(file.read().is_empty());
    }

    /// The whole point of having a file of its own.
    #[cfg(unix)]
    #[test]
    fn test_only_the_owner_can_read_it_whatever_was_there_before() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        fs::write(file.path(), "[tokens]\nold = \"token\"\n").unwrap();
        fs::set_permissions(file.path(), fs::Permissions::from_mode(0o644)).unwrap();

        file.set("home", Some("sekrit")).unwrap();

        let mode = fs::metadata(file.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(file.read().len(), 2);
    }
}

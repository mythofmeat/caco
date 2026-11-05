//! Which machine this is.
//!
//! Each machine appends to a folder named after its id, which is why the id cannot live
//! anywhere that travels: two machines with the same id would write to the same files,
//! and files that conflict are the problem this is all here to end. So it sits beside
//! the mirror tokens, outside both the data dir and the cache.

use std::fs;
use std::io;
use std::path::Path;

/// Longest a machine's id can be. It is a folder name, and goes in every line of its log.
const MAX_ID_LEN: usize = 64;

/// Whether `id` can be a machine's id, which ends up as a folder name in the log. Dots
/// are out because a [`super::clock::Clock`] uses them to separate its parts.
pub fn is_machine_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// This machine's id, making one up the first time it is asked for.
pub fn this_machine() -> io::Result<String> {
    machine_id_in(&crate::config::machine_id_file(), &hostname())
}

fn machine_id_in(file: &Path, hostname: &str) -> io::Result<String> {
    if let Ok(text) = fs::read_to_string(file) {
        let id = text.trim();
        if is_machine_id(id) {
            return Ok(id.to_string());
        }
        tracing::warn!(
            "{} does not hold a machine id: making a new one",
            file.display()
        );
    }

    let id = new_id(hostname);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    // Through a temporary name, because half of an id read back later is a different
    // machine, and this is the file that says which machine this is.
    let partial = file.with_extension("partial");
    fs::write(&partial, format!("{id}\n"))?;
    fs::rename(&partial, file)?;
    Ok(id)
}

/// The host's name, for people to read, and some randomness, because two machines called
/// `localhost` are not the same machine.
fn new_id(hostname: &str) -> String {
    let name = slug(hostname);
    let suffix = random_hex();
    if name.is_empty() {
        format!("machine-{suffix}")
    } else {
        format!("{name}-{suffix}")
    }
}

/// The first label of a host name, lower case, with nothing a folder name would mind.
fn slug(hostname: &str) -> String {
    let first = hostname.trim().split('.').next().unwrap_or_default();
    let cleaned: String = first
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(32)
        .collect();
    cleaned.trim_matches('-').to_string()
}

/// Eight hex digits the operating system seeded, which is what a `RandomState` is, for the
/// price of no dependency. Eight is plenty, since a clash needs two machines with the same
/// name too.
fn random_hex() -> String {
    use std::hash::{BuildHasher, Hasher};

    let random = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    format!("{:08x}", random as u32)
}

/// The host's name, wherever this system keeps it. Blank if nothing says.
fn hostname() -> String {
    if let Ok(name) = std::env::var("HOSTNAME")
        && !name.trim().is_empty()
    {
        return name;
    }
    if let Ok(name) = fs::read_to_string("/etc/hostname")
        && !name.trim().is_empty()
    {
        return name;
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// What to call a machine in a sentence: its id without the random part.
pub fn display_name(id: &str) -> &str {
    match id.rsplit_once('-') {
        Some((name, suffix))
            if !name.is_empty()
                && suffix.len() == 8
                && suffix.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            name
        }
        _ => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_an_id_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config/caco/machine-id");

        let id = machine_id_in(&file, "ThinkPad.local").unwrap();
        assert!(id.starts_with("thinkpad-"), "{id}");
        assert!(is_machine_id(&id));
        assert_eq!(machine_id_in(&file, "renamed").unwrap(), id);
    }

    /// A file somebody has been at could name a folder anywhere.
    #[test]
    fn test_a_file_that_does_not_hold_an_id_gets_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("machine-id");
        fs::write(&file, "../../etc\n").unwrap();

        let id = machine_id_in(&file, "box").unwrap();
        assert!(id.starts_with("box-"));
        assert_eq!(fs::read_to_string(&file).unwrap().trim(), id);
    }

    #[test]
    fn test_two_machines_with_the_same_name_are_still_two_machines() {
        assert_ne!(new_id("localhost"), new_id("localhost"));
    }

    #[test]
    fn test_a_host_with_no_usable_name_is_still_a_machine() {
        for name in ["", "   ", "...", "---"] {
            let id = new_id(name);
            assert!(id.starts_with("machine-"), "{name:?} made {id}");
            assert!(is_machine_id(&id));
        }
    }

    #[test]
    fn test_any_host_name_makes_a_valid_id() {
        for name in ["My MacBook Pro.local", "häuschen", "UPPER_case", "a/b\\c"] {
            let id = new_id(name);
            assert!(is_machine_id(&id), "{name:?} made {id}");
        }
    }

    #[test]
    fn test_display_name_drops_only_the_random_part() {
        assert_eq!(display_name("thinkpad-3fa9c1d2"), "thinkpad");
        assert_eq!(display_name("my-mac-book-3fa9c1d2"), "my-mac-book");
        assert_eq!(display_name("no-suffix"), "no-suffix");
        assert_eq!(display_name("machine"), "machine");
    }
}

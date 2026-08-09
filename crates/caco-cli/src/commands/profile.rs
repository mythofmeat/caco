//! `caco profile` — manage sourceport config profiles.

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::profiles;

use crate::resolve;

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// List config profiles
    Ls {
        /// Filter by sourceport
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
    },
    /// Create a new profile
    Create {
        /// Profile name
        name: String,
        /// Sourceport
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
        /// Copy from existing profile
        #[arg(long)]
        from: Option<String>,
    },
    /// Open profile in editor
    Edit {
        /// Profile name
        name: String,
        /// Sourceport
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
    },
    /// Copy a profile
    Cp {
        /// Source profile
        source: String,
        /// Destination profile
        dest: String,
        /// Sourceport
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
    },
    /// Delete a profile
    Rm {
        /// Profile name
        name: String,
        /// Sourceport
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Print absolute path to profile file
    Path {
        /// Profile name
        name: String,
        /// Sourceport
        #[arg(short = 'p', long)]
        sourceport: Option<String>,
    },
}

pub fn run(conn: &Connection, cmd: &ProfileCommand) -> Result<(), String> {
    match cmd {
        ProfileCommand::Ls { sourceport } => list_profiles(sourceport.as_deref()),
        ProfileCommand::Create {
            name,
            sourceport,
            from,
        } => create_profile(name, sourceport.as_deref(), from.as_deref()),
        ProfileCommand::Edit { name, sourceport } => edit_profile(name, sourceport.as_deref()),
        ProfileCommand::Cp {
            source,
            dest,
            sourceport,
        } => copy_profile(source, dest, sourceport.as_deref()),
        ProfileCommand::Rm {
            name,
            sourceport,
            yes,
        } => remove_profile(conn, name, sourceport.as_deref(), *yes),
        ProfileCommand::Path { name, sourceport } => show_path(name, sourceport.as_deref()),
    }
}

fn resolve_port(port: Option<&str>) -> Result<String, String> {
    profiles::resolve_sourceport(port).map_err(|e| e.to_string())
}

fn list_profiles(sourceport: Option<&str>) -> Result<(), String> {
    let found = profiles::list(sourceport);

    if found.is_empty() {
        if let Some(port) = sourceport {
            println!("No profiles for '{port}'.");
        } else {
            println!("No profiles found.");
        }
        return Ok(());
    }

    for profile in &found {
        println!("{}/{}", profile.sourceport, profile.name);
    }
    Ok(())
}

fn create_profile(name: &str, sourceport: Option<&str>, from: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport)?;
    profiles::create(&port, name, from).map_err(|e| e.to_string())?;

    match from {
        Some(source) => println!("Created profile '{name}' (copied from '{source}') for '{port}'."),
        None => println!("Created profile '{name}' for '{port}'."),
    }
    Ok(())
}

fn edit_profile(name: &str, sourceport: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport)?;

    let path = profiles::path(&port, name);
    if !path.exists() {
        return Err(format!(
            "Profile '{name}' not found for '{port}'. Create it with: caco profile create {name}"
        ));
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| format!("Failed to open editor '{editor}': {e}"))?;

    if !status.success() {
        return Err(format!(
            "Editor exited with code {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn copy_profile(source: &str, dest: &str, sourceport: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport)?;
    profiles::copy(&port, source, dest).map_err(|e| e.to_string())?;
    println!("Copied profile '{source}' to '{dest}' for '{port}'.");
    Ok(())
}

fn remove_profile(
    conn: &Connection,
    name: &str,
    sourceport: Option<&str>,
    yes: bool,
) -> Result<(), String> {
    let port = resolve_port(sourceport)?;
    // Check existence before prompting, but let core own the wording so the
    // message matches every other profile error.
    if !profiles::exists(&port, name) {
        return Err(caco_core::Error::ProfileNotFound {
            sourceport: port,
            name: name.to_string(),
        }
        .to_string());
    }

    // Check for WADs referencing this profile
    let referencing = profiles::referencing_wads(conn, name).map_err(|e| e.to_string())?;
    if !referencing.is_empty() {
        eprintln!(
            "Warning: {} WAD(s) reference profile '{name}':",
            referencing.len()
        );
        for wad in referencing.iter().take(5) {
            eprintln!("  {}: {}", wad.id, wad.title);
        }
    }

    if !yes && !resolve::confirm(&format!("Delete profile '{name}' for '{port}'?")) {
        return Err("Aborted.".to_string());
    }

    profiles::remove(&port, name).map_err(|e| e.to_string())?;
    println!("Deleted profile '{name}' for '{port}'.");
    Ok(())
}

fn show_path(name: &str, sourceport: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport)?;
    println!("{}", profiles::path(&port, name).display());
    Ok(())
}

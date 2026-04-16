//! `caco profile` — manage sourceport config profiles.

use clap::Subcommand;
use rusqlite::Connection;

use caco_core::config;

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

fn resolve_port(port: Option<&str>) -> String {
    port.map(|p| p.to_string())
        .unwrap_or_else(config::get_default_sourceport)
}

fn list_profiles(sourceport: Option<&str>) -> Result<(), String> {
    let profiles = config::list_profiles(sourceport);

    if profiles.is_empty() {
        if let Some(port) = sourceport {
            println!("No profiles for '{port}'.");
        } else {
            println!("No profiles found.");
        }
        return Ok(());
    }

    for (port, names) in &profiles {
        for name in names {
            println!("{port}/{name}");
        }
    }
    Ok(())
}

fn create_profile(name: &str, sourceport: Option<&str>, from: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport);
    if port.is_empty() {
        return Err("No sourceport specified and no default configured.".to_string());
    }

    let path = config::get_profile_path(&port, name);
    if path.exists() {
        return Err(format!("Profile '{name}' already exists for '{port}'."));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    if let Some(source_name) = from {
        let source_path = config::get_profile_path(&port, source_name);
        if !source_path.exists() {
            return Err(format!(
                "Source profile '{source_name}' not found for '{port}'."
            ));
        }
        std::fs::copy(&source_path, &path).map_err(|e| format!("Failed to copy profile: {e}"))?;
        println!("Created profile '{name}' (copied from '{source_name}') for '{port}'.");
    } else {
        std::fs::File::create(&path).map_err(|e| format!("Failed to create profile: {e}"))?;
        println!("Created profile '{name}' for '{port}'.");
    }
    Ok(())
}

fn edit_profile(name: &str, sourceport: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport);
    if port.is_empty() {
        return Err("No sourceport specified and no default configured.".to_string());
    }

    let path = config::get_profile_path(&port, name);
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
    let port = resolve_port(sourceport);
    if port.is_empty() {
        return Err("No sourceport specified and no default configured.".to_string());
    }

    let source_path = config::get_profile_path(&port, source);
    let dest_path = config::get_profile_path(&port, dest);

    if !source_path.exists() {
        return Err(format!("Source profile '{source}' not found for '{port}'."));
    }
    if dest_path.exists() {
        return Err(format!(
            "Destination profile '{dest}' already exists for '{port}'."
        ));
    }

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    std::fs::copy(&source_path, &dest_path).map_err(|e| format!("Failed to copy profile: {e}"))?;
    println!("Copied profile '{source}' to '{dest}' for '{port}'.");
    Ok(())
}

fn remove_profile(
    conn: &Connection,
    name: &str,
    sourceport: Option<&str>,
    yes: bool,
) -> Result<(), String> {
    let port = resolve_port(sourceport);
    if port.is_empty() {
        return Err("No sourceport specified and no default configured.".to_string());
    }

    let path = config::get_profile_path(&port, name);
    if !path.exists() {
        return Err(format!("Profile '{name}' not found for '{port}'."));
    }

    // Check for WADs referencing this profile
    let referencing =
        caco_core::db::search_wads(conn, Some(&format!("config:{name}")), None, true, false, 0)
            .map_err(|e| e.to_string())?;
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

    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete profile: {e}"))?;
    println!("Deleted profile '{name}' for '{port}'.");
    Ok(())
}

fn show_path(name: &str, sourceport: Option<&str>) -> Result<(), String> {
    let port = resolve_port(sourceport);
    if port.is_empty() {
        return Err("No sourceport specified and no default configured.".to_string());
    }

    let path = config::get_profile_path(&port, name);
    println!("{}", path.display());
    Ok(())
}

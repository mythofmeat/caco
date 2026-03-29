//! `caco completions` — shell completion scripts; `caco _complete` — dynamic data.

use std::fs;
use std::path::PathBuf;

use clap::Args;
use rusqlite::Connection;

use caco_core::db;
use caco_core::sourceports;
use crate::parsing;

const FISH_SCRIPT: &str = include_str!("../../../../completions/caco.fish");
const BASH_SCRIPT: &str = include_str!("../../../../completions/caco.bash");
const ZSH_SCRIPT: &str = include_str!("../../../../completions/_caco");

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell type (bash, fish, zsh)
    shell: Option<String>,

    /// Install completions to standard location
    #[arg(long)]
    install: bool,
}

#[derive(Args)]
pub struct CompleteArgs {
    /// Completion context
    context: String,
}

fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(|n| n.to_string()))
        .unwrap_or_else(|| "bash".to_string())
}

fn install_path(shell: &str) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    match shell {
        "fish" => Ok(home.join(".config/fish/completions/caco.fish")),
        "bash" => Ok(home.join(".local/share/bash-completion/completions/caco")),
        "zsh" => Ok(home.join(".zfunc/_caco")),
        _ => Err(format!("Unknown shell: {shell}")),
    }
}

pub fn run_completions(args: &CompletionsArgs) -> Result<(), String> {
    let shell = args
        .shell
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(detect_shell);

    let script = match shell.as_str() {
        "fish" => FISH_SCRIPT,
        "bash" => BASH_SCRIPT,
        "zsh" => ZSH_SCRIPT,
        other => return Err(format!("Unknown shell: {other} (expected bash, fish, or zsh)")),
    };

    if args.install {
        let path = install_path(&shell)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
        }
        fs::write(&path, script)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        eprintln!("Installed {shell} completions to {}", path.display());
        match shell.as_str() {
            "zsh" => {
                eprintln!();
                eprintln!("Add to your ~/.zshrc if not already present:");
                eprintln!("  fpath=(~/.zfunc $fpath)");
                eprintln!("  autoload -Uz compinit && compinit");
            }
            "bash" => {
                eprintln!();
                eprintln!("Completions will load automatically in new shells.");
                eprintln!("To use now: source {}", path.display());
            }
            _ => {}
        }
    } else {
        print!("{script}");
    }

    Ok(())
}

pub fn run_complete(conn: &Connection, args: &CompleteArgs) -> Result<(), String> {
    match args.context.as_str() {
        "wads" => {
            let wads = db::search_wads(conn, None, None, true, false, 0)
                .map_err(|e| e.to_string())?;
            for wad in &wads {
                println!("{}\t{}", wad.id, wad.title);
            }
        }
        "tags" => {
            let tags = db::get_all_tags(conn).map_err(|e| e.to_string())?;
            for tag in &tags {
                println!("{tag}");
            }
        }
        "iwads" => {
            let iwads = db::get_all_iwads(conn).map_err(|e| e.to_string())?;
            let mut families = std::collections::HashSet::new();
            for iwad in &iwads {
                if families.insert(iwad.family.clone()) {
                    println!("{}", iwad.family);
                }
                println!("{}/{}", iwad.family, iwad.variant);
            }
        }
        "statuses" => {
            for status in db::Status::ALL {
                println!("{}", status.as_str());
            }
        }
        "play-states" => {
            for ps in db::PlayState::ALL {
                println!("{}", ps.as_str());
            }
        }
        "intents" => {
            for intent in db::Intent::ALL {
                println!("{}", intent.as_str());
            }
        }
        "sort-fields" => {
            for field in parsing::SORT_FIELDS {
                println!("{field}+");
                println!("{field}-");
            }
        }
        "sourceports" => {
            let detected = sourceports::detect_sourceports();
            for (exe, _path, _family) in &detected {
                println!("{exe}");
            }
        }
        "modify-fields" => {
            for field in parsing::MODIFY_FIELDS {
                println!("{field}=");
                println!("!{field}");
            }
            println!("tag=");
            println!("!tag");
            println!("beaten+");
            println!("beaten-");
            println!("beaten=");
        }
        "query-fields" => {
            let fields = ["id:", "title:", "author:", "year:", "filename:", "tag:",
                          "status:", "source:", "iwad:", "complevel:", "config:",
                          "play:", "intent:", "avail:"];
            for field in &fields {
                println!("{field}");
            }
        }
        other => {
            return Err(format!("Unknown completion context: {other}"));
        }
    }
    Ok(())
}

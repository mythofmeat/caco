//! `caco config` — view or edit configuration.

use clap::Args;

use caco_core::config;

#[derive(Args)]
pub struct ConfigArgs {
    /// Open config in editor
    #[arg(short = 'e', long)]
    edit: bool,
}

pub fn run(args: &ConfigArgs) -> Result<(), String> {
    let config_path = config::config_file();

    if args.edit {
        // Create config with defaults if it doesn't exist
        if !config_path.exists() {
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create config directory: {e}"))?;
            }
            let cfg = config::Config::default();
            let content = toml::to_string_pretty(&cfg).map_err(|e| format!("Failed to serialize config: {e}"))?;
            std::fs::write(&config_path, content)
                .map_err(|e| format!("Failed to write config: {e}"))?;
        }

        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());

        let status = std::process::Command::new(&editor)
            .arg(&config_path)
            .status()
            .map_err(|e| format!("Failed to open editor '{editor}': {e}"))?;

        if !status.success() {
            return Err(format!("Editor exited with code {}", status.code().unwrap_or(-1)));
        }
        Ok(())
    } else {
        // Print config file
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read config: {e}"))?;
            print!("{content}");
        } else {
            println!("# No config file found at {}", config_path.display());
            println!("# Create one with: caco config --edit");
        }
        Ok(())
    }
}

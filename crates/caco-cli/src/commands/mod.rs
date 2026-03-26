pub mod cache;
pub mod companion;
pub mod completions;
pub mod config;
pub mod demos;
pub mod enrich;
pub mod gc;
pub mod import;
pub mod info;
pub mod ls;
pub mod modify;
pub mod play;
pub mod profile;
pub mod random;
pub mod saves;
pub mod stats;
pub mod trash;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    /// List WADs in library
    Ls(ls::LsArgs),
    /// Show WAD details
    Info(info::InfoArgs),
    /// Show WAD details (alias)
    #[command(name = "i", hide = true)]
    InfoAlias(info::InfoArgs),
    /// Modify WAD metadata
    Modify(modify::ModifyArgs),
    /// Soft-delete WADs
    Trash(trash::TrashArgs),
    /// Pick a random WAD
    Random(random::RandomArgs),
    /// Import WADs from sources
    Import(import::ImportArgs),
    /// Play a WAD
    Play(play::PlayArgs),
    /// Manage WAD cache
    Cache {
        #[command(subcommand)]
        command: cache::CacheCommand,
    },
    /// Library statistics
    Stats(stats::StatsArgs),
    /// Play session history
    Sessions(stats::SessionsArgs),
    /// Manage save files
    Saves {
        #[command(subcommand)]
        command: saves::SavesCommand,
    },
    /// Manage demo recordings
    Demos {
        #[command(subcommand)]
        command: demos::DemosCommand,
    },
    /// Manage companion files
    Companion {
        #[command(subcommand)]
        command: companion::CompanionCommand,
    },
    /// Manage sourceport config profiles
    Profile {
        #[command(subcommand)]
        command: profile::ProfileCommand,
    },
    /// Re-run enrichment for existing WADs
    Enrich(enrich::EnrichArgs),
    /// Garbage collect finished/abandoned WAD data
    Gc(gc::GcArgs),
    /// View or edit config
    Config(config::ConfigArgs),
    /// Output shell completions
    Completions(completions::CompletionsArgs),
    /// Dynamic completion data (hidden)
    #[command(name = "_complete", hide = true)]
    Complete(completions::CompleteArgs),
}

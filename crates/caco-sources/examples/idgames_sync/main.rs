//! Build the local index of idgames against the real thing, away from the app.
//!
//! For finding out what a change does to the real archive: what the first run asks for,
//! how big the file is, and how long a search takes. It runs at the same polite speed the
//! app does, so a first run with the API up is about half an hour, and it asks volunteers
//! for 21,000 files if the API is blocked. The index goes where `CACO_CACHE_HOME` says.
//!
//! ```bash
//! CACO_CACHE_HOME=/tmp/index cargo run -p caco-sources --example idgames_sync
//! CACO_CACHE_HOME=/tmp/index cargo run -p caco-sources --example idgames_sync -- search sunlust
//! cargo run -p caco-sources --example idgames_sync -- read some-upload.txt
//! ```

use std::time::Instant;

use caco_core::config;
use caco_sources::idgames::Index;
use caco_sources::idgames::sync::{SyncOptions, SyncProgress, sync_index};
use caco_sources::idgames::textfile;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // What the text files on the archive read as, one JSON object per file.
    if let [command, files @ ..] = args.as_slice()
        && command == "read"
    {
        for file in files {
            let bytes = std::fs::read(file).expect("read the file");
            let found = textfile::parse(&textfile::decode(&bytes));
            let line = serde_json::json!({
                "file": file,
                "title": found.title,
                "author": found.author,
                "email": found.email,
                "description": found.description,
                "credits": found.credits,
                "base": found.base,
                "buildtime": found.buildtime,
                "editors": found.editors,
                "bugs": found.bugs,
            });
            println!("{line}");
        }
        return;
    }

    let path = config::idgames_index_path();
    let index = Index::open(&path).expect("open the index");

    if let [command, query @ ..] = args.as_slice()
        && command == "search"
    {
        let query = query.join(" ");
        let started = Instant::now();
        let found = index.search(&query, 15).expect("search");
        println!("{} results in {:?}", found.len(), started.elapsed());
        for entry in found {
            println!(
                "{:>6} {} {:<40} {}{}",
                entry.id, entry.date, entry.title, entry.dir, entry.filename
            );
        }
        return;
    }

    let options = SyncOptions::for_config(&config::load_config(), true, chrono::Utc::now());
    let started = Instant::now();
    let report = sync_index(
        &index,
        &options,
        &mut |progress| match progress {
            SyncProgress::Listing => println!("[{:?}] reading the listing", started.elapsed()),
            SyncProgress::AskingApi { done, total } if done % 25 == 0 => {
                println!(
                    "[{:?}] asking the API: {done} of {total}",
                    started.elapsed()
                );
            }
            SyncProgress::ReadingText { done, total } if done % 100 == 0 => {
                println!(
                    "[{:?}] reading text files: {done} of {total}",
                    started.elapsed()
                );
            }
            _ => {}
        },
        &|| false,
    )
    .expect("sync");
    println!("[{:?}] done: {report:#?}", started.elapsed());
    println!("{:#?}", index.status().expect("status"));
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    println!("{}: {:.1} MB", path.display(), size as f64 / 1e6);
}

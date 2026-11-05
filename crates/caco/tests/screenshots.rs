//! Off-screen screenshots of every GUI surface.
//!
//! egui is immediate-mode and this app has no unit-testable layout: whether a
//! row fits inside its panel is only decided at paint time, against a real
//! font atlas and a real viewport size. So the only way to catch "this widget
//! is clipped" or "this window is taller than the window it lives in" is to
//! render a frame and look at it.
//!
//! `egui_kittest` renders through wgpu with no display server, so this runs
//! over ssh, in a tty, or in CI with a software adapter, which is where
//! `verify.yml` runs it. It is `#[ignore]`d for the same reason
//! `sourceports_build.rs` is: `cargo test --workspace` must not depend on a
//! GPU. Run it explicitly:
//!
//! ```bash
//! cargo test -p caco --test screenshots -- --ignored --nocapture
//! ```
//!
//! PNGs land in `target/screenshots/<WxH>/`. `CACO_SHOT_SIZE=800x400` overrides
//! the viewport; small sizes are the interesting ones, since that is where
//! fixed-size dialogs overflow.
//!
//! A dialog escaping the window is not the only way to fail. kittest's own
//! checks stay on, because each catches something a PNG hides: a character no
//! font can draw panics instead of painting tofu, and so does an input widget
//! with no accessible name.
//!
//! The library is a *copy* of the real database into a temp `CACO_HOME`, so
//! the shots show real WADs while nothing can write to the real library.

use std::ops::Not;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use caco::dialogs::storage::StorageTab;
use caco::get::GetTarget;
use caco::import::state::{SearchResultEntry, SearchResults, SearchSource, SearchSourceData};
use caco::message::{AppMessage, EnrichReport, IdgamesIndexRun};
use caco::state::{ActionRequest, ViewMode};
use caco_core::sourceports::RemoteRefs;
use caco_sources::SourceError;
use caco_sources::enrich_service::WikiBlocked;
use caco_sources::http::WafKind;
use caco_sources::idgames::sync::{
    ListingOutcome, SyncOptions, SyncProgress, SyncReport, sync_index,
};
use caco_sources::idgames::{IdgamesClient, Index};
use caco_sources::import_service::ImportResult;
use egui_kittest::kittest::{NodeT, Queryable};

/// How long to leave worker threads to answer between rounds of frames.
const WORKER_PAUSE: Duration = Duration::from_millis(80);

/// What the wizard calls a folder mirror, unless told otherwise.
const MIRROR_NAME: &str = "folder";

/// A port nothing listens on, for everything that would otherwise ask the real idgames: it
/// fails at once, every time, the way a blocked API does as far as anything here can tell.
const NO_API: &str = "http://127.0.0.1:9/api.php";

/// Viewport used for every shot unless `CACO_SHOT_SIZE` says otherwise.
/// 1200x800 is `main.rs`'s `with_inner_size`, so this is the window a user
/// gets on first launch.
fn shot_size() -> egui::Vec2 {
    let spec = std::env::var("CACO_SHOT_SIZE").unwrap_or_else(|_| "1200x800".to_string());
    let (w, h) = spec.split_once('x').expect("CACO_SHOT_SIZE must be WxH");
    egui::vec2(
        w.trim().parse().expect("width"),
        h.trim().parse().expect("height"),
    )
}

/// One directory per viewport, so a run at one size cannot overwrite the shots
/// from another: CI keeps both, and a layout fix wants them side by side.
fn out_dir() -> PathBuf {
    let size = shot_size();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/screenshots")
        .join(format!("{}x{}", size.x, size.y));
    std::fs::create_dir_all(&dir).expect("create screenshot dir");
    dir.canonicalize().expect("resolve screenshot dir")
}

/// Copy the real library into a scratch `CACO_HOME`.
///
/// Falls back to an empty database when there is no real library (CI), so the
/// harness still proves every surface *renders* even if the shots are boring.
fn scratch_home() -> PathBuf {
    let home = std::env::temp_dir().join("caco-screenshots-home");
    std::fs::create_dir_all(&home).expect("create scratch home");

    // Resolve the real DB before pointing CACO_HOME elsewhere.
    let real_db = caco_core::config::get_db_path();
    let scratch_db = home.join("library.db");
    if real_db != scratch_db {
        // Not last run's. With no real library to copy over it, which is CI, that is what the
        // next run would start from, and CI runs this twice in a row. A `-wal` left beside a
        // library it did not come from is no better.
        for suffix in ["", "-wal", "-shm"] {
            let mut stale = scratch_db.clone().into_os_string();
            stale.push(suffix);
            let _ = std::fs::remove_file(stale);
        }
        if real_db.exists() {
            std::fs::copy(&real_db, &scratch_db).expect("copy library");
        }
    }

    // Last run's logs, or this run's library would merge them straight back in, and its
    // own settings for this "machine", which the first save below writes again.
    let _ = std::fs::remove_dir_all(home.join("sync"));
    let _ = std::fs::remove_file(home.with_file_name("caco-screenshots-home.machine.toml"));

    let index = home.join("idgames-index.db");
    for suffix in ["", "-wal", "-shm"] {
        let mut name = index.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(name);
    }

    unsafe {
        std::env::set_var("CACO_HOME", &home);
        // Not just the library: linking a file for a WAD caco can fetch again writes to
        // the WAD cache, which is not under `CACO_HOME`.
        std::env::set_var("CACO_CACHE_DIR", home.join("cache-wads"));
        // And nor is the folder of each WAD's saves, which a merge can rename.
        std::env::set_var("CACO_DATA_DIR", home.join("data"));
        std::env::set_var("CACO_IDGAMES_INDEX", &index);
        std::env::set_var("CACO_IDGAMES_API", NO_API);
        std::env::set_var("CACO_CONFIG", home.join("config.toml"));
    }
    // The real settings, but for the one that would have the app ask the real mirrors for
    // a listing as soon as it is created. Nothing here is asked of anyone until a button
    // says so, and those have the API above and a copy of idgames on disk to go to.
    let mut config = (*caco_core::config::load_config()).clone();
    config.idgames_index_check_days = 0;
    caco_core::config::save_config(&config).expect("save scratch config");
    caco_core::config::reload_config();

    put_sourceport_on_path(&home);
    home
}

/// The other machine the sync shots hear from.
const LAPTOP: &str = "laptop-3fa9c1d2";

/// What is on the scratch copy of idgames. The names are real ones, and the same in two
/// places on purpose, as `castle.zip` is. One text file is Latin-1, one is from before there
/// was a template, and one upload has no text file at all, all as they are on the archive.
const COPY_OF_IDGAMES: &[(&str, &[u8])] = &[
    (
        "levels/doom2/Ports/megawads/sunlust",
        b"Title                : Sunlust\r\n\
          Author               : Ribbiks & Dannebubinga\r\n\
          Description          : Sunlust is a set of 32 boom-compatible maps for Doom II,\r\n\
          \x20                      designed to be played from pistol start.\r\n",
    ),
    (
        "levels/doom2/Ports/s-u/thepact_1_2",
        b"Title     : The Pact\nAuthor    : Laoco\xf6n\nDescription : A statue, and his sons.\n",
    ),
    (
        "levels/doom/a-c/asdoom",
        b"DOOM - the entire ASDOOM series,\nreplacing levels E1M1 to E1M9.\n",
    ),
    (
        "levels/doom/a-c/castle",
        b"Title : The Castle\nAuthor : Someone\n",
    ),
    (
        "levels/doom2/a-c/castle",
        b"Title : Castle of a Quite Different Kind, and a Long Enough Title to Need Clipping\n\
          Author : Someone Else With a Rather Long Name Indeed\n",
    ),
];

/// A copy of idgames as `rsync` leaves one, and the name of a folder holding it.
fn write_copy_of_idgames(home: &Path) -> PathBuf {
    use std::io::Write;

    let root = home.join("idgames-copy");
    let _ = std::fs::remove_dir_all(&root);
    let mut listing = String::new();
    let mut in_dir: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
    for (path, text) in COPY_OF_IDGAMES {
        let (dir, name) = path.rsplit_once('/').unwrap();
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file.with_extension("zip"), b"PK").unwrap();
        std::fs::write(file.with_extension("txt"), text).unwrap();
        let lines = in_dir.entry(dir).or_default();
        lines.push(format!("-rw-rw-r-- 1 a b 2 Mar  9  2019 {name}.zip"));
        lines.push(format!(
            "-rw-rw-r-- 1 a b {} Mar  9  2019 {name}.txt",
            text.len()
        ));
    }
    in_dir
        .entry("combos")
        .or_default()
        .push("-rw-rw-r-- 1 a b 2 Mar  9  2021 extrme1.zip".to_string());
    std::fs::create_dir_all(root.join("combos")).unwrap();
    std::fs::write(root.join("combos/extrme1.zip"), b"PK").unwrap();
    for (dir, lines) in in_dir {
        listing.push_str(&format!("./{dir}:\ntotal 1\n{}\n\n", lines.join("\n")));
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(listing.as_bytes()).unwrap();
    std::fs::write(root.join("ls-laR.gz"), gz.finish().unwrap()).unwrap();
    root
}

/// Settings only draws its "detected" controls with a sourceport on `PATH`, so
/// without this, what a run checks would depend on what the machine has
/// installed, and CI has nothing. Detection only asks whether the file exists,
/// so an empty one does: nothing ever runs it.
fn put_sourceport_on_path(home: &Path) {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).expect("create scratch bin");
    std::fs::write(bin.join("nyan-doom"), "").expect("create scratch sourceport");
    let path = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", format!("{}:{path}", bin.display())) };
}

struct Shooter {
    harness: egui_kittest::Harness<'static, caco::app::CacoApp>,
    home: PathBuf,
}

impl Shooter {
    fn new() -> Self {
        let home = scratch_home();
        let db_path = home.join("library.db");
        let conn = caco_core::db::open_connection(&db_path).expect("open db");
        caco_core::db::init_db(&conn).expect("init db");

        // The same creation path as `main.rs`, and the real `eframe::App`: the
        // theme's fonts are on the context before the harness runs its first
        // pass, and workers wake the context that is actually painting.
        let harness = egui_kittest::Harness::builder()
            .with_size(shot_size())
            .build_eframe(|cc| {
                caco::theme::apply_doom_theme(&cc.egui_ctx);
                caco::app::CacoApp::new(conn, db_path, &cc.egui_ctx)
            });

        let mut me = Self { harness, home };
        // First frame loads the library, later frames pick up thumbnails as
        // the worker threads answer.
        me.settle(8, WORKER_PAUSE);
        me
    }

    /// Run `rounds` rounds of frames, `pause` apart, so layout can converge
    /// and workers can answer.
    ///
    /// `run_ok`, not `run`. A worker posting a message asks for a repaint, as
    /// it should, and `run` panics on a UI still being asked after `max_steps`.
    /// How many requests land inside one `run` is down to thread timing, so
    /// `run` passes and fails the same build at random. `run_ok` stops at the
    /// budget instead, skipping its accessibility check when it does, which is
    /// why `shot` makes that check itself.
    fn settle(&mut self, rounds: u32, pause: Duration) {
        for _ in 0..rounds {
            self.harness.run_ok();
            std::thread::sleep(pause);
        }
    }

    fn shot(&mut self, name: &str) {
        self.settle(3, Duration::ZERO);
        // `settle` can end up skipping this: see there.
        self.harness.check_accessibility();
        self.no_dead_way_out(name);
        let img = self
            .harness
            .render()
            .unwrap_or_else(|e| panic!("render {name}: {e:?}"));
        let path = out_dir().join(format!("{name}.png"));
        img.save(&path)
            .unwrap_or_else(|e| panic!("save {name}: {e}"));
        println!("wrote {}", path.display());
    }

    /// Don't draw a control that can't act: a way out that does nothing is the worst of them.
    /// See `docs/ui.md`. A button with a reason to wait, such as Finish on a form that is not
    /// filled in, is not one of these.
    fn no_dead_way_out(&self, name: &str) {
        for label in ["Cancel", "Close", "Stop", "Stop Build", "Back", "Done"] {
            for node in self.harness.query_all_by_label(label) {
                assert!(
                    !node.accesskit_node().is_disabled(),
                    "{name} draws a disabled {label}"
                );
            }
        }
    }

    /// Switch to a full-panel view and shoot it.
    fn shot_view(&mut self, name: &str, mode: ViewMode) {
        self.shot_view_after(name, mode, |_| {});
    }

    /// Same, but drive the view into a state only a worker can produce before
    /// shooting it.
    fn shot_view_after(&mut self, name: &str, mode: ViewMode, after_open: impl FnOnce(&mut Self)) {
        self.harness.state_mut().set_view(mode);
        self.settle(6, WORKER_PAUSE);
        after_open(self);
        self.shot(name);
        self.harness.state_mut().set_view(ViewMode::Library);
        self.settle(1, Duration::ZERO);
    }

    /// Open a dialog through the same path a click takes, shoot it, close it.
    ///
    /// Returns how far the dialog spills outside the app window, per edge, in
    /// points. Anything positive means a user at this window size cannot reach
    /// part of the dialog — a title bar above the top edge, a Close button
    /// below the bottom one.
    fn shot_dialog(&mut self, name: &str, action: ActionRequest) -> f32 {
        self.shot_dialog_after(name, action, |_| {})
    }

    /// Same, but drive the open dialog into a state only a worker can produce
    /// before shooting it — a build with a step and a log, for one.
    fn shot_dialog_after(
        &mut self,
        name: &str,
        action: ActionRequest,
        after_open: impl FnOnce(&mut Self),
    ) -> f32 {
        self.shot_opened_by(name, |s| {
            s.harness.state_mut().dispatch_action(action);
            s.settle(4, WORKER_PAUSE);
            after_open(s);
        })
    }

    /// Same, for a dialog nothing dispatches: `open` gets it on screen however the
    /// app does.
    fn shot_opened_by(&mut self, name: &str, open: impl FnOnce(&mut Self)) -> f32 {
        open(self);
        self.settle(4, Duration::ZERO);
        self.shot(name);
        let overflow = self.worst_overflow();
        self.harness.state_mut().close_dialog();
        self.settle(1, Duration::ZERO);
        overflow
    }

    /// Post the report of a library run the wiki refused, the way the worker
    /// posts it, and check it reached the open Enrich dialog.
    fn post_wiki_blocked_report(&mut self, findings: Vec<String>) {
        let blocked = WikiBlocked {
            kind: WafKind::Cloudflare,
            unchecked: 71,
        };
        let tx = self.harness.state_mut().background_sender();
        tx.send(AppMessage::EnrichComplete(Ok(EnrichReport {
            examined: 116,
            findings,
            wiki_lookups: 1,
            wiki_blocked: Some(blocked.to_string()),
            cancelled: false,
            dry_run: false,
        })));
        self.settle(2, WORKER_PAUSE);
        self.harness.get_by_label_contains("could not be checked");
    }

    /// A manual-only WAD whose file is gone, in the state the library is in
    /// once a data dir arrives from a machine that had it: a recorded path
    /// that resolves to nothing. Play is what finds that out, so the shot goes
    /// through it.
    ///
    /// The path is long on purpose, since a path that cannot wrap is what has
    /// pushed dialogs off the screen before, and the file name is one no real
    /// cache holds, because Play looks for it by name in the cache dir too.
    fn add_lost_wad(&self) -> i64 {
        use caco_core::db::{self, NewWad, SourceType, WadUpdate};

        let gone = self
            .home
            .join("mnt/nas-that-is-not-mounted/doom/cacowards/2025/runners-up")
            .join("caco-screenshots-purgatory-v1.1-final-with-the-music-fix.pk3");
        let conn = db::open_connection(&self.home.join("library.db")).expect("open db");
        let id = db::add_wad(
            &conn,
            &NewWad::new("Purgatory", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/forum/topic/150000-purgatory/"),
        )
        .expect("add lost WAD");
        let recorded = WadUpdate::new().set_text("cached_path", Some(gone.display().to_string()));
        db::update_wad(&conn, id, &recorded).expect("record its file");
        id
    }

    /// A WAD whose file has changed since it was last played: a file on disk,
    /// hashed, and a session that recorded an older one. Play is what notices,
    /// so the shot goes through it, and it stops before resolving a
    /// sourceport, which the scratch home does not configure.
    fn add_changed_wad(&self) -> i64 {
        use caco_core::db::{self, NewWad, SourceType};
        use caco_core::identity;

        let dir = self.home.join("changed");
        std::fs::create_dir_all(&dir).expect("create scratch WAD dir");
        // A well-formed PWAD with an empty directory: the re-analysis worker
        // reads it as soon as the library reloads.
        let path = dir.join("caco-screenshots-sunder-fixed.wad");
        let mut wad = b"PWAD".to_vec();
        wad.extend_from_slice(&0_i32.to_le_bytes());
        wad.extend_from_slice(&12_i32.to_le_bytes());
        std::fs::write(&path, wad).expect("write WAD");

        let conn = db::open_connection(&self.home.join("library.db")).expect("open db");
        let id = db::add_wad(
            &conn,
            &NewWad::new("Sunder", SourceType::Idgames).cached_path(path.display().to_string()),
        )
        .expect("add changed WAD");
        let found = identity::identify(&path).expect("hash WAD");
        identity::record(&conn, id, &path, &found).expect("record its identity");
        conn.execute(
            "INSERT INTO sessions (wad_id, started_at, content_sha256) \
             VALUES (?1, '2026-10-12T19:30:00+00:00', ?2)",
            rusqlite::params![id, "0123456789abcdef".repeat(4)],
        )
        .expect("record a session on the old file");
        id
    }

    /// Put an event in another machine's log, as if a sync tool had just brought it. Clocks
    /// from long ago, so that nothing here depends on what else has happened.
    fn other_machine_says(
        &self,
        wall_ms: u64,
        op: caco_core::sync::event::Op,
        kind: &str,
        key: &str,
        fields: serde_json::Value,
    ) {
        use caco_core::sync::clock::Clock;
        use caco_core::sync::event::{Event, VERSION};

        let dir = self.home.join("sync").join(LAPTOP);
        std::fs::create_dir_all(&dir).expect("create the other machine's log");
        let event = Event {
            v: VERSION,
            id: format!("{wall_ms}-{kind}-{key}"),
            machine: LAPTOP.to_string(),
            clock: Clock::at(wall_ms, LAPTOP),
            at: "2026-11-03T20:15:00.000Z".to_string(),
            op,
            kind: kind.to_string(),
            key: key.to_string(),
            fields: fields.as_object().cloned().unwrap_or_default(),
        };
        let path = dir.join("2026-11.jsonl");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str(&event.to_line().expect("write event"));
        text.push('\n');
        std::fs::write(path, text).expect("write the other machine's log");
    }

    /// Press Sync Now in the open Sync tool, and wait for the other machine to show up.
    fn sync_now_until_the_other_machine_shows(&mut self) {
        self.harness.get_by_label("Sync Now").click_accesskit();
        self.settle(1, Duration::ZERO);
        self.wait_for("the other machine's row", |s| {
            s.harness.query_all_by_label("laptop").next().is_some()
        });
    }

    /// Start the mirror shots from the same place whatever machine this is: no mirrors,
    /// which decides the page the mirrors tool opens on, in the scratch home's own config
    /// file, which the wizard goes on writing to. And no WAD files, in the scratch copy
    /// of the library only, so that what gets offered and filled is what these shots add,
    /// and a run against a real library does not copy its files into a temp dir.
    fn start_mirror_shots(&self) {
        let conn = caco_core::db::open_connection(&self.home.join("library.db")).expect("open db");
        conn.execute("UPDATE wads SET cached_path = NULL", [])
            .expect("forget the real library's files");

        unsafe { std::env::set_var("CACO_CONFIG", self.home.join("config.toml")) };
        let mut config = (*caco_core::config::load_config()).clone();
        config.mirrors.clear();
        caco_core::config::save_config(&config).expect("save scratch config");
        caco_core::config::reload_config();
    }

    /// Build the index the way the real one is built, by the real sync, from the copy of
    /// idgames this writes, with the API refusing and so with no ids. Then tell the app, as
    /// the worker does.
    fn build_idgames_index(&mut self) -> PathBuf {
        let root = write_copy_of_idgames(&self.home);
        let copy = caco_core::config::MirrorConfig {
            name: "copy".to_string(),
            path: Some(root.display().to_string()),
            layout: Some("idgames".to_string()),
            ..Default::default()
        };
        let index = Index::open_default().expect("open the index");
        let report = sync_index(
            &index,
            &SyncOptions {
                sources: vec![copy],
                api: IdgamesClient::with_api_url(NO_API),
                check_days: 1,
                force: true,
                now: chrono::Utc::now(),
                api_pause: Duration::ZERO,
                text_pause: Duration::ZERO,
            },
            &mut |_| {},
            &|| false,
        )
        .expect("sync the index");
        assert!(
            matches!(report.listing, ListingOutcome::Updated(_)),
            "{report:?}"
        );
        assert_eq!(report.text_read, COPY_OF_IDGAMES.len(), "{report:?}");
        let tx = self.harness.state_mut().background_sender();
        tx.send(AppMessage::IdgamesIndexStatus(index.status().unwrap()));
        self.settle(2, WORKER_PAUSE);
        root
    }

    /// Start the Get shots from a Downloads folder of their own, and with pages going
    /// to a list rather than a browser: this presses Get for real, on a machine where
    /// nobody asked for a browser to open. Returns the folder and the list.
    ///
    /// Doom Wiki enrichment goes off too, since an import has nothing to learn from a
    /// wiki that is not part of what these shots are about.
    fn start_get_shots(&mut self) -> (PathBuf, Arc<Mutex<Vec<String>>>) {
        let downloads = self.home.join("Downloads");
        let _ = std::fs::remove_dir_all(&downloads);
        std::fs::create_dir_all(&downloads).expect("create scratch Downloads");

        let mut config = (*caco_core::config::load_config()).clone();
        config.downloads_dir = downloads.display().to_string();
        config.auto_doomwiki_enrich = false;
        caco_core::config::save_config(&config).expect("save scratch config");
        caco_core::config::reload_config();

        let opened: Arc<Mutex<Vec<String>>> = Arc::default();
        let recording = Arc::clone(&opened);
        self.harness
            .state_mut()
            .open_pages_with(Arc::new(move |url| {
                recording.lock().unwrap().push(url.to_string());
                Ok(())
            }));
        (downloads, opened)
    }

    /// Wait for a page to go to the browser, and return it.
    fn wait_for_page(&mut self, opened: &Arc<Mutex<Vec<String>>>) -> String {
        self.wait_for("a page to open", |_| !opened.lock().unwrap().is_empty());
        opened.lock().unwrap().remove(0)
    }

    /// A browser finishing a download: Firefox's way, which is the hardest to read, since
    /// the final name is there and empty for as long as the temporary one grows.
    fn download(&mut self, downloads: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let done = downloads.join(name);
        let part = downloads.join(format!("{name}.part"));
        std::fs::write(&done, b"").expect("write placeholder");
        std::fs::write(&part, bytes).expect("write temporary");
        // Long enough for the worker to have looked at both, which must prompt nothing.
        self.settle(5, WORKER_PAUSE * 2);
        assert!(
            self.harness.query_by_label("Not This One").is_none(),
            "asked about a download that had not finished"
        );
        std::fs::rename(&part, &done).expect("finish download");
        done
    }

    /// The row a Cacoward entry has, unlinked in the scratch library if the real one has
    /// it linked, since the shots are about an entry with no WAD.
    fn absent_cacoward(&self, key: &str) -> i64 {
        let conn = caco_core::db::open_connection(&self.home.join("library.db")).expect("open db");
        conn.execute(
            "UPDATE cacowards SET wad_id = NULL, manual_override = 0 WHERE key = ?1",
            [key],
        )
        .expect("unlink entry");
        conn.query_row("SELECT id FROM cacowards WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .expect("the dataset has this entry")
    }

    /// Keep rendering until `done` says so, for what a real worker is off doing.
    fn wait_for(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        for _ in 0..100 {
            self.settle(1, WORKER_PAUSE);
            if done(self) {
                return;
            }
        }
        // What was on screen when it gave up is most of what there is to know.
        self.shot("timed-out");
        panic!("timed out waiting for {what}: see timed-out.png");
    }

    /// Type into the field with this label, the way a person has to: focus it first.
    fn type_into(&mut self, label: &str, text: &str) {
        self.harness.get_by_label(label).focus();
        self.settle(1, Duration::ZERO);
        self.harness.get_by_label(label).type_text(text);
        self.settle(2, Duration::ZERO);
    }

    /// Open the mirrors tool on a kind's page, through the buttons a person would use.
    fn open_wizard_on(&mut self, kind: &str) {
        self.harness
            .state_mut()
            .dispatch_action(ActionRequest::Mirrors);
        self.settle(2, Duration::ZERO);
        self.harness.get_by_label(kind).click_accesskit();
        self.settle(3, Duration::ZERO);
    }

    /// A manual-only WAD with its file here: what a mirror is offered. Its own bytes and
    /// its own forum thread, or the mirror would rightly recognise it as one it holds.
    /// The file's name is long on purpose, since it goes in a sentence in the offer.
    fn add_manual_wad_with_file(&self, title: &str, topic: i64) -> i64 {
        use caco_core::db::{self, NewWad, SourceType};

        let conn = db::open_connection(&self.home.join("library.db")).expect("open db");
        let dir = self.home.join("offered");
        std::fs::create_dir_all(&dir).expect("create scratch WAD dir");
        let path = dir.join(format!(
            "caco-screenshots-{topic}-v1.2-final-with-the-music-fix.pk3"
        ));
        let mut wad = b"PWAD\0\0\0\0\x0c\0\0\0".to_vec();
        wad.extend_from_slice(topic.to_string().as_bytes());
        std::fs::write(&path, wad).expect("write WAD");

        db::add_wad(
            &conn,
            &NewWad::new(title, SourceType::Doomworld)
                .source_url(format!(
                    "https://www.doomworld.com/forum/topic/{topic}-a-wad/"
                ))
                .cached_path(path.display().to_string()),
        )
        .expect("add manual WAD")
    }

    /// A WAD a mirror lists and cannot produce, because the mirror's copy has gone.
    /// Play is what finds that out, so the shot goes through it, and it stops at the
    /// link dialog with nothing launched.
    fn add_wad_on_a_broken_mirror(&self) -> i64 {
        use caco_core::db::{self, NewWad, SourceType, mirrors};
        use caco_core::mirrors::MirrorEntry;

        let conn = db::open_connection(&self.home.join("library.db")).expect("open db");
        let id = db::add_wad(
            &conn,
            &NewWad::new("Purgatory", SourceType::Doomworld)
                .source_url("https://www.doomworld.com/forum/topic/150000-purgatory/"),
        )
        .expect("add WAD");
        let entry = MirrorEntry {
            sha256: "ab".repeat(32),
            size: 4,
            filename: "caco-screenshots-purgatory-v1.1-final.pk3".to_string(),
            doomworld: Some(150000),
            ..Default::default()
        };
        mirrors::sync_files(&conn, MIRROR_NAME, &[entry]).expect("list it on the mirror");
        id
    }

    /// Open the mirror offer for `wad_id` the way an import does, by posting what the
    /// import worker posts.
    fn post_import_of(&mut self, wad_id: i64) {
        let tx = self.harness.state_mut().background_sender();
        tx.send(AppMessage::ImportComplete(Ok(ImportResult {
            wad_id: Some(wad_id),
            is_duplicate: false,
            duplicate_id: None,
            duplicate_title: None,
            error: None,
        })));
        self.settle(2, WORKER_PAUSE);
    }

    fn purge_wad(&self, id: i64) {
        let conn = caco_core::db::open_connection(&self.home.join("library.db")).expect("open db");
        caco_core::db::delete_wad(&conn, id, true).expect("purge WAD");
    }

    /// Largest distance by which any floating window escapes the viewport.
    ///
    /// Read off egui's own area bookkeeping rather than the pixels, because a
    /// window clipped at the screen edge and a window that merely happens to
    /// end there look identical in a PNG. Modals are in the middle layer, and what is
    /// asked over them in the foreground one.
    fn worst_overflow(&self) -> f32 {
        let ctx = &self.harness.ctx;
        let screen = ctx.content_rect();
        let layers: Vec<egui::LayerId> = ctx.memory(|m| {
            m.areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|l| matches!(l.order, egui::Order::Middle | egui::Order::Foreground))
                .collect()
        });
        layers
            .into_iter()
            .filter_map(|l| ctx.memory(|m| m.area_rect(l.id)))
            .map(|r| {
                let over = (screen.min.x - r.min.x)
                    .max(screen.min.y - r.min.y)
                    .max(r.max.x - screen.max.x)
                    .max(r.max.y - screen.max.y);
                if over > 1.0 {
                    println!("    window {r:?} vs screen {screen:?}");
                }
                over
            })
            .fold(f32::NEG_INFINITY, f32::max)
    }
}

#[test]
#[ignore = "needs a GPU adapter; run explicitly"]
fn shoot_every_surface() {
    let mut s = Shooter::new();

    s.shot("01-library-grid");

    // Views, not dialogs — no overflow check applies, but a blank page is
    // still a failure worth seeing.
    for (name, mode) in [
        ("02-stats", ViewMode::Stats),
        ("03-cacowards", ViewMode::Cacowards),
    ] {
        s.shot_view(name, mode);
    }
    // Posters, for looking at the pictures, and with them the thumbnails come in.
    s.shot_view_after("03-cacowards-grid", ViewMode::Cacowards, |s| {
        s.harness.get_by_label("Grid").click_accesskit();
        s.settle(4, WORKER_PAUSE);
    });
    s.harness.state_mut().set_view(ViewMode::Cacowards);
    s.settle(2, Duration::ZERO);
    s.harness.get_by_label("List").click_accesskit();
    s.settle(2, Duration::ZERO);
    s.harness.state_mut().set_view(ViewMode::Library);
    s.settle(1, Duration::ZERO);

    // The far end of a strip that has to scroll to reach it, since the newest year
    // is what opens. An old year also has different sections to lay out: no
    // honorable mentions, and awards the recent ones don't give.
    s.shot_view_after("03-cacowards-oldest", ViewMode::Cacowards, |s| {
        // Through accesskit, because a pointer click lands where the chip is, and
        // that is off the end of the strip until something scrolls it there.
        s.harness.get_by_label("2004").click_accesskit();
        s.settle(3, Duration::ZERO);
        s.harness.get_by_label("WORST WAD");
        assert!(s.harness.query_by_label("DOOM MOMA").is_none());
    });

    // The forms, each with inputs to name. The `3`, `4` and `5` shortcuts reach them.
    for (name, key) in [
        ("04-import-doomworld", egui::Key::Num3),
        ("04-import-url", egui::Key::Num4),
        ("04-import-local", egui::Key::Num5),
    ] {
        s.shot_view_after(name, ViewMode::Import, |s| {
            s.harness.key_press(key);
            s.settle(1, Duration::ZERO);
        });
    }

    // A blocked source is the state this view most has to explain, and only a
    // worker can report it. Posted the way the search worker posts it, on the tab
    // the `2` shortcut reaches. Finding the text proves it reaches the screen; the
    // PNG shows whether it fits there.
    s.shot_view_after("04-import-search-blocked", ViewMode::Import, |s| {
        s.harness.key_press(egui::Key::Num2);
        s.settle(1, Duration::ZERO);
        let blocked = SourceError::WafBlocked {
            site: "Doom Wiki",
            kind: WafKind::Cloudflare,
        };
        let tx = s.harness.state_mut().background_sender();
        tx.send(AppMessage::SearchComplete(
            SearchSource::Doomwiki,
            Err(blocked.to_string()),
        ));
        s.settle(2, WORKER_PAUSE);
        s.harness.get_by_label_contains("blocked the request");
    });

    // The local index of idgames, from nothing, through the real sync and the real workers.
    // The view comes first on the Import view, so this is what opening it looks like.
    s.shot_view_after("04-import-idgames-no-index", ViewMode::Import, |s| {
        s.harness.key_press(egui::Key::Num1);
        s.settle(1, Duration::ZERO);
        s.harness.get_by_label_contains("No local index yet");
    });
    let copy_root = s.build_idgames_index();
    s.shot_view_after("04-import-idgames-newest", ViewMode::Import, |s| {
        s.wait_for("the newest uploads", |s| {
            s.harness.query_by_label("Newest on idgames").is_some()
        });
        s.harness.get_by_label_contains("Searching 6 uploads here");
        // What the text files said, in whatever they were saved as, and a file name for
        // what has none.
        s.harness.get_all_by_label("The Pact").next().unwrap();
        s.harness.get_all_by_label("extrme1").next().unwrap();
        s.harness.get_all_by_label("Laocoön").next().unwrap();
    });
    s.shot_view_after("04-import-idgames-search", ViewMode::Import, |s| {
        s.type_into("Search:", "Laocoon");
        s.harness.key_press(egui::Key::Enter);
        s.wait_for("the search", |s| {
            s.harness.query_by_label("1 result").is_some()
        });
        s.harness.get_all_by_label("The Pact").next().unwrap();
        assert!(s.harness.query_by_label("Newest on idgames").is_none());
    });
    s.shot_view_after("04-import-idgames-same-name", ViewMode::Import, |s| {
        s.harness.get_by_label("Search:").focus();
        s.settle(1, Duration::ZERO);
        // Replacing what the last shot typed, as a person selecting it would.
        s.harness
            .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        s.harness.get_by_label("Search:").type_text("castle");
        s.settle(1, Duration::ZERO);
        s.harness.key_press(egui::Key::Enter);
        s.wait_for("the search", |s| {
            s.harness.query_by_label("2 results").is_some()
        });
        // Where each one is, since the name will not tell them apart.
        s.harness.get_by_label_contains("levels/doom/a-c/");
    });
    // The notes only a worker reaches: a run in flight, then what a failure leaves behind,
    // then a search that only got an answer from half an index.
    s.shot_view_after("04-import-idgames-reading", ViewMode::Import, |s| {
        let tx = s.harness.state_mut().background_sender();
        tx.send(AppMessage::IdgamesIndexProgress(
            SyncProgress::ReadingText {
                done: 4_200,
                total: 21_384,
            },
        ));
        s.settle(2, WORKER_PAUSE);
        s.harness.get_by_label_contains("4200 of 21384");
        s.harness.get_by_label("Stop");
        assert!(s.harness.query_by_label("Update").is_none());
    });
    s.shot_view_after("04-import-idgames-no-listing", ViewMode::Import, |s| {
        let tx = s.harness.state_mut().background_sender();
        tx.send(AppMessage::IdgamesIndexFinished(Ok(IdgamesIndexRun {
            report: SyncReport {
                listing: ListingOutcome::Failed(
                    "https://youfailit.net/pub/idgames/: could not connect: Connection refused"
                        .to_string(),
                ),
                ..Default::default()
            },
            status: Index::open_default().unwrap().status().unwrap(),
            ids_filled_in: 0,
            asked_for: false,
        })));
        s.settle(2, WORKER_PAUSE);
        s.harness
            .get_by_label_contains("Could not read a new listing");
        s.harness.get_by_label("Update");
    });
    s.shot_view_after("04-import-idgames-half-built", ViewMode::Import, |s| {
        let note = SourceError::WafBlocked {
            site: "idgames",
            kind: WafKind::Cloudflare,
        };
        let tx = s.harness.state_mut().background_sender();
        tx.send(AppMessage::SearchComplete(
            SearchSource::Idgames,
            Ok(SearchResults {
                entries: vec![SearchResultEntry {
                    title: "sunlust".to_string(),
                    author: None,
                    description: None,
                    source_data: SearchSourceData::Idgames {
                        target: caco_sources::idgames::IdgamesRef::Path(
                            "levels/doom2/Ports/megawads/sunlust.zip".to_string(),
                        ),
                        rating: None,
                        date: Some("2015-08-09".to_string()),
                        dir: Some("levels/doom2/Ports/megawads/".to_string()),
                        filename: Some("sunlust.zip".to_string()),
                    },
                }],
                note: Some(format!(
                    "From the local index, which is still being built, because idgames' own \
                     search could not be asked. {note}"
                )),
            }),
        ));
        s.settle(2, WORKER_PAUSE);
        s.harness.get_by_label_contains("still being built");
        // The sidebar's, and the one that takes this row as it is, with no id.
        assert!(s.harness.get_all_by_label("Import").count() >= 2);
    });

    let mut escaped = Vec::new();
    let mut record = |name: &str, over: f32| {
        println!("  {name}: overflow {over:.0}pt");
        // 1pt of slack absorbs rounding; anything more is a real spill.
        if over > 1.0 {
            escaped.push(format!("{name} (+{over:.0}pt)"));
        }
    };

    for (name, action) in [
        // One shot per Storage tab: they share a window but not a body, and
        // the tab that overflows is the one that has to be named.
        ("storage-cache", ActionRequest::Storage(StorageTab::Cache)),
        ("storage-clean", ActionRequest::Storage(StorageTab::Clean)),
        ("storage-trash", ActionRequest::Storage(StorageTab::Trash)),
        ("storage-files", ActionRequest::Storage(StorageTab::Files)),
        ("profiles", ActionRequest::Profiles),
        ("enrich", ActionRequest::Enrich),
        ("resources", ActionRequest::Resources),
        ("sourceports", ActionRequest::Sourceports),
        ("collections", ActionRequest::Collections),
        ("settings", ActionRequest::Settings),
    ] {
        record(name, s.shot_dialog(&format!("dlg-{name}"), action));
    }

    // Help, which nothing could open until there was a way to, and the dialogs about one WAD,
    // which take one to open. A WAD of their own, since the library can be empty.
    record("help", s.shot_dialog("dlg-help", ActionRequest::Help));
    let one_wad = s.add_manual_wad_with_file("A Wad to Open Dialogs On", 999_100);
    for (name, action) in [
        ("edit-details", ActionRequest::Edit(one_wad)),
        ("delete", ActionRequest::Delete(one_wad)),
        ("sessions", ActionRequest::Sessions(one_wad)),
        ("map-stats", ActionRequest::MapStats(one_wad)),
        ("saves-demos", ActionRequest::WadData(one_wad)),
    ] {
        record(name, s.shot_dialog(&format!("dlg-{name}"), action));
    }
    s.purge_wad(one_wad);

    // Linking an award to a WAD by hand, with a filter that has an input to name.
    let an_award = s.absent_cacoward("2025-winner-below-the-gaudy-goblin");
    record(
        "cacoward-link",
        s.shot_dialog("dlg-cacoward-link", ActionRequest::LinkCacoward(an_award)),
    );

    // The form Collections opens, whose fields only exist once it has.
    record(
        "collections-form",
        s.shot_dialog_after("dlg-collections-form", ActionRequest::Collections, |s| {
            s.harness.get_by_label("Add").click_accesskit();
            s.settle(3, Duration::ZERO);
            s.harness.get_by_label("Query");
            s.harness.get_by_label("Save");
        }),
    );

    // Being asked before something goes for good: in the footer, with the way out and the
    // action where they always are. Through the button, on a WAD of its own in the Trash.
    let trashed = s.add_manual_wad_with_file("A Wad in the Trash", 999_101);
    {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        caco_core::db::delete_wad(&conn, trashed, false).expect("trash WAD");
    }
    record(
        "storage-trash-asking",
        s.shot_dialog_after(
            "dlg-storage-trash-asking",
            ActionRequest::Storage(StorageTab::Trash),
            |s| {
                s.harness.get_by_label("Empty Trash").click_accesskit();
                s.settle(3, Duration::ZERO);
                s.harness.get_by_label_contains("Permanently delete");
                s.harness.get_by_label("Delete Permanently");
            },
        ),
    );
    s.purge_wad(trashed);

    // A dialog is modal: a real click on the sidebar around it goes nowhere. Stats would have
    // something to show if it went anywhere.
    s.harness.state_mut().dispatch_action(ActionRequest::Enrich);
    s.settle(2, Duration::ZERO);
    s.harness.get_by_label("Stats").click();
    s.settle(2, Duration::ZERO);
    assert!(
        s.harness.query_by_label("Playtime by month").is_none(),
        "a click went through a dialog to the app behind it"
    );

    // Nor can the keyboard get there. Tab goes through everything that wants focus, which past
    // the dialog's own controls would be the sidebar, where Enter presses what it lands on.
    for press in 1..=12 {
        s.harness.key_press(egui::Key::Tab);
        s.settle(1, Duration::ZERO);
        let ctx = &s.harness.ctx;
        let focused = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id));
        assert!(
            focused
                .as_ref()
                .is_none_or(|widget| widget.layer_id.order != egui::Order::Background),
            "Tab {press} went through a dialog to {focused:?}"
        );
    }
    s.harness.state_mut().close_dialog();
    s.settle(1, Duration::ZERO);

    // And a field that had the keyboard when a dialog opened lets go of it, or what is typed
    // goes to the app behind. Help has no fields of its own to take it.
    s.harness.state_mut().set_view(ViewMode::Library);
    s.settle(2, Duration::ZERO);
    s.harness
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    s.settle(2, Duration::ZERO);
    s.harness.state_mut().dispatch_action(ActionRequest::Help);
    s.settle(2, Duration::ZERO);
    s.harness.event(egui::Event::Text("typed".to_string()));
    s.settle(2, Duration::ZERO);
    s.harness.state_mut().close_dialog();
    s.settle(2, Duration::ZERO);
    assert_eq!(
        s.harness.get_by_label("Filter WADs").value().as_deref(),
        Some(""),
        "what was typed in a dialog reached the filter behind it"
    );

    // The version picker only exists once a remote has answered, and that is
    // asked on demand, so opening the dialog alone never reaches it.
    record(
        "sourceports-version-picker",
        s.shot_dialog_after(
            "dlg-sourceports-version-picker",
            ActionRequest::Sourceports,
            |s| {
                let tx = s.harness.state_mut().background_sender();
                tx.send(AppMessage::SourceportVersionsLoaded {
                    sourceport: "nyan-doom".to_string(),
                    outcome: Ok(RemoteRefs {
                        branches: vec!["master".to_string()],
                        releases: vec!["v1.2.3".to_string(), "v1.2.2".to_string()],
                    }),
                });
            },
        ),
    );

    // A finished build with output is the state the Sourceports dialog exists
    // for, and its tallest: status row plus log pane on top of the sourceport list.
    // Posted the way the build worker posts it, so the shot proves the app can
    // actually reach this layout and not just that it renders.
    record(
        "sourceports-build-log",
        s.shot_dialog_after(
            "dlg-sourceports-build-log",
            ActionRequest::Sourceports,
            |s| {
                let tx = s.harness.state_mut().background_sender();
                tx.send(AppMessage::SourceportBuildStep("Compiling".to_string()));
                for i in 1..=200 {
                    tx.send(AppMessage::SourceportBuildLine(format!(
                        "[{i}/1636] Building CXX object src/CMakeFiles/nyan-doom.dir/p_map.c.o"
                    )));
                }
                tx.send(AppMessage::SourceportBuildComplete(Err(
                    "compile failed: see log".to_string(),
                )));
            },
        ),
    );

    // Only a run reaches the report, and this is its tallest state: findings from
    // the files it could read, under a note about the wiki it could not.
    record(
        "enrich-wiki-blocked",
        s.shot_dialog_after("dlg-enrich-wiki-blocked", ActionRequest::Enrich, |s| {
            let findings = (1..=30)
                .map(|i| format!("WAD {i} \u{2192} complevel 9 (Boom)"))
                .collect();
            s.post_wiki_blocked_report(findings);
        }),
    );

    // With no findings, the report must not claim the wiki found nothing.
    record(
        "enrich-wiki-blocked-empty",
        s.shot_dialog_after(
            "dlg-enrich-wiki-blocked-empty",
            ActionRequest::Enrich,
            |s| {
                s.post_wiki_blocked_report(Vec::new());
                assert!(s.harness.query_by_label("Nothing new detected.").is_none());
            },
        ),
    );

    // Only Play reaches the File Changed dialog, and only for a WAD whose file
    // plays differently from the one its last session recorded.
    let changed = s.add_changed_wad();
    record(
        "file-changed",
        s.shot_dialog_after("dlg-file-changed", ActionRequest::Play(changed), |s| {
            s.settle(4, WORKER_PAUSE);
            s.harness
                .get_by_label_contains("has changed since you last played it");
        }),
    );
    s.purge_wad(changed);

    // The Sync tool, on a machine that has only heard from itself, then with another
    // machine's log arrived the way a sync tool leaves it: a WAD, sessions, a session of a
    // WAD that has not arrived, and a line from a newer version. Sync Now is pressed for
    // real, and what it finds is checked in the library.
    record(
        "sync",
        s.shot_dialog_after("dlg-sync", ActionRequest::Sync, |s| {
            s.harness.get_by_label_contains("Leave library.db out");
            s.harness.get_by_label("Sync Now");
            assert!(s.harness.query_by_label("laptop").is_none());
        }),
    );
    {
        use caco_core::sync::event::Op;
        use serde_json::json;
        s.other_machine_says(
            1_000,
            Op::Add,
            "wad",
            "doomworld-999001",
            json!({
                "title": "Came From The Laptop",
                "source_type": "doomworld",
                "source_id": "999001",
            }),
        );
        s.other_machine_says(
            2_000,
            Op::Add,
            "session",
            "00000000000000000000000000000001",
            json!({ "wad": "doomworld-999001", "started_at": "2026-11-03T20:00:00+00:00" }),
        );
        s.other_machine_says(
            3_000,
            Op::Add,
            "session",
            "00000000000000000000000000000002",
            json!({ "wad": "doomworld-999002", "started_at": "2026-11-03T20:00:00+00:00" }),
        );
    }
    let newer = s.home.join("sync").join(LAPTOP).join("2026-11.jsonl");
    let mut text = std::fs::read_to_string(&newer).expect("read the log");
    text.push_str(&format!(
        "{{\"v\":99,\"id\":\"from-the-future\",\"clock\":\"0000000004000.000000.{LAPTOP}\"}}\n"
    ));
    std::fs::write(&newer, text).expect("add a line from a newer version");
    record(
        "sync-machines",
        s.shot_dialog_after("dlg-sync-machines", ActionRequest::Sync, |s| {
            s.sync_now_until_the_other_machine_shows();
            s.harness.get_by_label_contains("1 change is waiting");
            s.harness
                .get_by_label_contains("from a newer version of caco");
        }),
    );
    {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        let (title, sessions): (String, i64) = conn
            .query_row(
                "SELECT title, (SELECT COUNT(*) FROM sessions WHERE wad_id = wads.id) \
                 FROM wads WHERE uid = 'doomworld-999001'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the other machine's WAD arrived");
        assert_eq!(title, "Came From The Laptop");
        assert_eq!(sessions, 1);
    }

    // Only Play reaches the Newer Saves dialog, for a WAD with a file here and a save
    // the other machine says it has. A second save of a name too long for one line.
    let with_newer_saves = s.add_manual_wad_with_file("Saved On The Laptop", 999_003);
    {
        use caco_core::sync::event::Op;
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        let uid = caco_core::db::get_wad(&conn, with_newer_saves, false)
            .expect("read WAD")
            .expect("the WAD is there")
            .uid;
        s.other_machine_says(
            5_000,
            Op::Set,
            "saves",
            &format!("{uid}/{LAPTOP}"),
            serde_json::json!({
                "wad": uid,
                "machine": LAPTOP,
                "files": serde_json::to_string(&serde_json::json!([
                    { "path": "nyan_doom_data/doom2/road/nyan-doom-savegame0.dsg",
                      "size": 4, "sha256": "aa" },
                    { "path": "nyan_doom_data/doom2/road/a-save-with-rather-a-long-name-indeed-for-a-file.dsg",
                      "size": 4, "sha256": "bb" },
                ])).unwrap(),
                "recorded_at": "2026-11-03T20:15:00+00:00",
                "present": 1,
            }),
        );
    }
    s.harness.state_mut().dispatch_action(ActionRequest::Sync);
    s.settle(2, Duration::ZERO);
    s.sync_now_until_the_other_machine_shows();
    s.wait_for("the other machine's saves", |s| {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        conn.query_row("SELECT COUNT(*) FROM wad_saves", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("count saves")
            > 0
    });
    s.harness.state_mut().close_dialog();
    s.settle(1, Duration::ZERO);
    record(
        "newer-saves",
        s.shot_dialog_after(
            "dlg-newer-saves",
            ActionRequest::Play(with_newer_saves),
            |s| {
                s.wait_for("the question about newer saves", |s| {
                    s.harness.query_by_label("Play Anyway").is_some()
                });
                s.harness.get_by_label_contains("laptop has 2 saves");
                s.harness.get_by_label("nyan-doom-savegame0.dsg");
            },
        ),
    );
    s.purge_wad(with_newer_saves);

    // Setting up a mirror, the way a person does. It opens on the choice of kind only
    // when there are none, and each kind has instructions of its own to fit.
    s.start_mirror_shots();
    record(
        "mirrors-kinds",
        s.shot_dialog_after("dlg-mirrors-kinds", ActionRequest::Mirrors, |s| {
            s.harness.get_by_label("A Folder");
            s.harness.get_by_label("A Web Server");
        }),
    );
    for (name, kind, instruction) in [
        (
            "mirrors-folder",
            "A Folder",
            "If it isn't there, caco will create it",
        ),
        ("mirrors-gitea", "Gitea", "set package to Read and Write"),
        ("mirrors-webdav", "WebDAV", "rclone serve webdav"),
        (
            "mirrors-web-server",
            "A Web Server",
            "python3 -m http.server",
        ),
        ("mirrors-idgames-copy", "Copy of idgames", "rsync -rt"),
    ] {
        record(
            name,
            s.shot_opened_by(&format!("dlg-{name}"), |s| {
                s.open_wizard_on(kind);
                s.harness.get_by_label_contains(instruction);
            }),
        );
    }

    // A test that fails has to say so and say what to try, through the real worker.
    record(
        "mirrors-test-failed",
        s.shot_opened_by("dlg-mirrors-test-failed", |s| {
            s.open_wizard_on("WebDAV");
            // A port nothing listens on, so the failure is quick and always the same.
            s.type_into("Address", "http://127.0.0.1:9/doom");
            s.harness.get_by_label("Test It").click_accesskit();
            s.wait_for("the test to fail", |s| {
                s.harness
                    .query_by_label_contains("Could not use it")
                    .is_some()
            });
            s.harness
                .get_by_label_contains("reachable from this machine");
            s.harness.get_by_label("Add Anyway");
        }),
    );

    // A password goes through the real wizard, and its handling is the point of this
    // step: in a file only this user can read, and nowhere near the config, which is
    // meant to travel between machines. A mirror that cannot be reached can be added
    // anyway, which is how this gets there with no server. Removing it takes the
    // password with it.
    s.open_wizard_on("WebDAV");
    s.type_into("Address", "http://127.0.0.1:9/doom");
    s.type_into("Username", "doom");
    s.type_into("Password", "a-password-for-the-test");
    s.harness.get_by_label("Test It").click_accesskit();
    s.wait_for("the test to fail", |s| {
        s.harness.query_by_label("Add Anyway").is_some()
    });
    s.harness.get_by_label("Add Anyway").click_accesskit();
    s.settle(3, WORKER_PAUSE);
    {
        let config = caco_core::config::load_config();
        let saved = &config.mirrors[0];
        assert_eq!(saved.login(), Some("doom"));
        assert_eq!(saved.token().as_deref(), Some("a-password-for-the-test"));
        let on_disk = std::fs::read_to_string(caco_core::config::config_file()).unwrap();
        assert!(on_disk.contains("doom"), "{on_disk}");
        assert!(!on_disk.contains("a-password-for-the-test"), "{on_disk}");

        let tokens = caco_core::mirror_tokens::TokenFile::from_config();
        assert!(!tokens.path().starts_with(&s.home), "{:?}", tokens.path());
        assert_eq!(
            tokens.read().get("webdav").map(String::as_str),
            Some("a-password-for-the-test")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tokens.path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
    s.harness.get_by_label("Done").click_accesskit();
    s.settle(3, WORKER_PAUSE);
    s.harness.get_by_label("Remove").click_accesskit();
    s.settle(3, Duration::ZERO);
    s.harness.get_by_label("Stop Using It").click_accesskit();
    s.settle(3, WORKER_PAUSE);
    assert!(caco_core::config::load_config().mirrors.is_empty());
    assert!(
        caco_core::mirror_tokens::TokenFile::from_config()
            .read()
            .is_empty()
    );
    s.harness.get_by_label_contains("No mirrors yet");
    s.harness.state_mut().close_dialog();
    s.settle(1, Duration::ZERO);

    // And the whole of it, for real: a folder that isn't there yet, tested, created and
    // filled, from nothing but what's on screen.
    let filled = s.add_manual_wad_with_file("Lonesome Road", 151858);
    let mirror_root = s.home.join("mirrors/one-that-is/not-there-yet");
    let _ = std::fs::remove_dir_all(s.home.join("mirrors"));
    let mirror_path = mirror_root.display().to_string();
    record(
        "mirrors-test-passed",
        s.shot_opened_by("dlg-mirrors-test-passed", |s| {
            s.open_wizard_on("A Folder");
            s.type_into("Folder", &mirror_path);
            s.harness.get_by_label("Test It").click_accesskit();
            s.wait_for("the test to pass", |s| {
                s.harness
                    .query_by_label_contains("isn't there yet")
                    .is_some()
            });
            assert!(!mirror_root.exists(), "testing it must not create it");
            s.harness.get_by_label("Finish");
        }),
    );

    s.open_wizard_on("A Folder");
    s.type_into("Folder", &mirror_path);
    s.harness.get_by_label("Test It").click_accesskit();
    s.wait_for("the test to pass", |s| {
        s.harness.query_by_label("Finish").is_some()
    });
    s.harness.get_by_label("Finish").click_accesskit();
    let upload_label = format!("Upload 1 to {MIRROR_NAME}");
    s.wait_for("the mirror to be set up", |s| {
        s.harness.query_by_label(&upload_label).is_some()
    });
    assert!(
        mirror_root.join(caco_core::mirrors::INDEX_PATH).is_file(),
        "finishing should create the folder and an empty index"
    );
    assert_eq!(caco_core::config::load_config().mirrors.len(), 1);
    record(
        "mirrors-fill",
        s.shot_opened_by("dlg-mirrors-fill", |s| {
            s.harness.get_by_label_contains("Lonesome Road");
        }),
    );

    s.harness
        .state_mut()
        .dispatch_action(ActionRequest::Mirrors);
    s.settle(2, Duration::ZERO);
    s.harness.get_by_label("Upload 1 WAD").click_accesskit();
    s.settle(2, Duration::ZERO);
    s.harness.get_by_label(&upload_label).click_accesskit();
    s.wait_for("the upload to finish", |s| {
        s.harness.state_mut().bulk_upload_running().not()
            && s.harness.query_by_label("Uploaded 1 WAD.").is_some()
    });
    record(
        "mirrors-fill-done",
        s.shot_opened_by("dlg-mirrors-fill-done", |s| {
            s.harness.get_by_label_contains("Nothing to upload");
        }),
    );
    {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        let wad = caco_core::db::get_wad(&conn, filled, false)
            .expect("read WAD")
            .expect("WAD still there");
        assert!(wad.mirrored, "the upload did not reach the library");
        assert!(wad.retrievability().is_automatic());
    }

    // The list, with a mirror in it that has been read.
    record(
        "mirrors-list",
        s.shot_dialog_after("dlg-mirrors-list", ActionRequest::Mirrors, |s| {
            s.harness.get_by_label("Read Them Again").click_accesskit();
            s.wait_for("the mirror to be read", |s| {
                s.harness.query_by_label_contains("1 file").is_some()
            });
            s.harness.get_by_label("Add a Mirror");
            s.harness.get_by_label("Edit");
        }),
    );

    // Editing reads it back, and removing asks first and says what it costs.
    record(
        "mirrors-edit",
        s.shot_dialog_after("dlg-mirrors-edit", ActionRequest::Mirrors, |s| {
            s.harness.get_by_label("Edit").click_accesskit();
            s.settle(3, Duration::ZERO);
            assert_eq!(
                s.harness.get_by_label("Folder").value().as_deref(),
                Some(mirror_path.as_str())
            );
        }),
    );
    record(
        "mirrors-remove",
        s.shot_dialog_after("dlg-mirrors-remove", ActionRequest::Mirrors, |s| {
            s.harness.get_by_label("Remove").click_accesskit();
            s.settle(3, Duration::ZERO);
            s.harness
                .get_by_label_contains("Nothing on the mirror is deleted");
            s.harness.get_by_label("Keep It");
        }),
    );
    s.purge_wad(filled);

    // A copy of idgames goes through the same wizard, with less to it: the real test, the
    // folder above it first, which is the likeliest mistake, and then the right one.
    record(
        "mirrors-idgames-copy-wrong-folder",
        s.shot_dialog_after(
            "dlg-mirrors-idgames-copy-wrong-folder",
            ActionRequest::Mirrors,
            |s| {
                s.harness.get_by_label("Add a Mirror").click_accesskit();
                s.settle(2, Duration::ZERO);
                s.harness.get_by_label("Copy of idgames").click_accesskit();
                s.settle(3, Duration::ZERO);
                s.type_into("Folder or address", &s.home.display().to_string());
                s.harness.get_by_label("Test It").click_accesskit();
                s.wait_for("the test to fail", |s| {
                    s.harness
                        .query_by_label_contains("Could not use it")
                        .is_some()
                });
                s.harness.get_by_label_contains("not the one above");
            },
        ),
    );
    record(
        "mirrors-idgames-copy-passed",
        s.shot_dialog_after(
            "dlg-mirrors-idgames-copy-passed",
            ActionRequest::Mirrors,
            |s| {
                s.harness.get_by_label("Add a Mirror").click_accesskit();
                s.settle(2, Duration::ZERO);
                s.harness.get_by_label("Copy of idgames").click_accesskit();
                s.settle(3, Duration::ZERO);
                s.type_into("Folder or address", &copy_root.display().to_string());
                s.harness.get_by_label("Test It").click_accesskit();
                s.wait_for("the test to pass", |s| {
                    s.harness
                        .query_by_label_contains("its listing has 6 uploads")
                        .is_some()
                });
                s.harness.get_by_label("Finish");
            },
        ),
    );
    // Finishing saves it as what it is, touches nothing in it, and reads the index from it.
    record(
        "mirrors-idgames-copy-list",
        s.shot_dialog_after(
            "dlg-mirrors-idgames-copy-list",
            ActionRequest::Mirrors,
            |s| {
                s.harness.get_by_label("Add a Mirror").click_accesskit();
                s.settle(2, Duration::ZERO);
                s.harness.get_by_label("Copy of idgames").click_accesskit();
                s.settle(3, Duration::ZERO);
                s.type_into("Folder or address", &copy_root.display().to_string());
                s.harness.get_by_label("Test It").click_accesskit();
                s.wait_for("the test to pass", |s| {
                    s.harness.query_by_label("Finish").is_some()
                });
                s.harness.get_by_label("Finish").click_accesskit();
                s.wait_for("it to be saved", |s| {
                    s.harness
                        .query_by_label_contains("A copy of idgames")
                        .is_some()
                });
                s.wait_for("the index run it starts", |s| {
                    !s.harness.state_mut().idgames_index_running()
                });
                let saved = caco_core::config::load_config();
                let copy = saved
                    .mirrors
                    .iter()
                    .find(|mirror| mirror.is_idgames_copy())
                    .expect("saved as a copy of idgames");
                assert!(!copy.write);
                assert_eq!(copy.path.as_deref(), copy_root.to_str());
                let on_disk = std::fs::read_to_string(caco_core::config::config_file()).unwrap();
                assert!(on_disk.contains("layout = \"idgames\""), "{on_disk}");
                assert!(!copy_root.join("index").exists());
            },
        ),
    );
    // With a copy to read from and no API to ask, Update can be pressed for real.
    s.shot_view_after("04-import-idgames-updated", ViewMode::Import, |s| {
        s.harness.key_press(egui::Key::Num1);
        s.settle(1, Duration::ZERO);
        s.harness.get_by_label("Update").click_accesskit();
        s.settle(1, WORKER_PAUSE);
        s.wait_for("the index run", |s| {
            !s.harness.state_mut().idgames_index_running()
        });
        s.harness.get_by_label_contains("idgames has nothing new");
    });
    let mut config = (*caco_core::config::load_config()).clone();
    config.mirrors.retain(|mirror| !mirror.is_idgames_copy());
    caco_core::config::save_config(&config).expect("save scratch config");
    caco_core::config::reload_config();

    // The tools are in the sidebar, whatever the size of the window, and scrolled to if they
    // have to be. Settings and Help are in reach without that.
    s.shot_view_after("05-tools", ViewMode::Library, |s| {
        for tool in [
            "Storage",
            "Mirrors",
            "Sync",
            "Enrich",
            "IWADs",
            "Sourceports",
            "Profiles",
        ] {
            s.harness.get_by_label(tool);
        }
        for pinned in ["Settings", "Help"] {
            let row = s.harness.get_by_label(pinned).rect();
            assert!(
                row.max.y <= shot_size().y,
                "{pinned} is off the window: {row:?}"
            );
        }
    });

    // Only a WAD arriving with no way back brings up the offer.
    let manual = s.add_manual_wad_with_file("The Evil Reborn", 152000);
    record(
        "mirror-offer",
        s.shot_opened_by("dlg-mirror-offer", |s| {
            s.post_import_of(manual);
            s.harness
                .get_by_label_contains("any machine with this library");
        }),
    );

    // Edit's File section offers it too, for a file that was here before the mirror.
    record(
        "edit-sources-upload",
        s.shot_dialog_after(
            "dlg-edit-sources-upload",
            ActionRequest::Edit(manual),
            |s| {
                s.harness.get_by_label("Sources").click();
                s.settle(3, Duration::ZERO);
                s.harness.get_by_label(&format!("Upload to {MIRROR_NAME}"));
            },
        ),
    );

    // Pressing the button has to do what it says. Through the real offer, the real
    // worker and a real (if local) mirror, and the WAD comes out the other side
    // reading as something caco can fetch.
    s.post_import_of(manual);
    s.harness
        .get_by_label(&format!("Upload to {MIRROR_NAME}"))
        .click();
    for _ in 0..50 {
        s.settle(1, WORKER_PAUSE);
        if s.harness.state_mut().uploads_running() == 0 {
            break;
        }
    }
    {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        let wad = caco_core::db::get_wad(&conn, manual, false)
            .expect("read WAD")
            .expect("WAD still there");
        assert!(wad.mirrored, "the upload did not reach the library");
        assert!(wad.retrievability().is_automatic());
    }

    // Once it is up, the same section says so and stops offering.
    record(
        "edit-sources-on-mirror",
        s.shot_dialog_after(
            "dlg-edit-sources-on-mirror",
            ActionRequest::Edit(manual),
            |s| {
                s.harness.get_by_label("Sources").click();
                s.settle(3, Duration::ZERO);
                s.harness
                    .get_by_label_contains(&format!("On mirror: {MIRROR_NAME}"));
                assert!(
                    s.harness
                        .query_by_label(&format!("Upload to {MIRROR_NAME}"))
                        .is_none()
                );
            },
        ),
    );
    s.purge_wad(manual);

    // Getting a file through the browser, the whole way and the way a person does: press
    // the button, the page opens, a download lands, and the file ends up in the library.
    // Real folder, real worker, real link. Only the browser is played by a list.
    let (downloads, opened) = s.start_get_shots();
    let gotten = s.add_lost_wad();
    let page = "https://www.doomworld.com/forum/topic/150000-purgatory/";
    record(
        "link-get",
        s.shot_dialog_after("dlg-link-get", ActionRequest::Play(gotten), |s| {
            s.settle(4, WORKER_PAUSE);
            s.harness.get_by_label_contains("Get in Browser opens");
            s.harness.get_by_label("Link Local File");
        }),
    );
    s.harness
        .state_mut()
        .dispatch_action(ActionRequest::Play(gotten));
    s.wait_for("the link dialog", |s| {
        s.harness.query_by_label("Get in Browser").is_some()
    });
    s.harness.get_by_label("Get in Browser").click_accesskit();
    assert_eq!(s.wait_for_page(&opened), page);
    s.settle(2, Duration::ZERO);
    let waiting = "Waiting for download: Purgatory...";
    s.harness.get_by_label(waiting);
    s.shot("06-get-waiting");

    // The menu behind that line, for when the page that opened is a dead share link.
    s.harness.get_by_label(waiting).click();
    s.settle(3, Duration::ZERO);
    s.harness.get_by_label("Open doomworld.com");
    s.harness.get_by_label("Stop Waiting");
    s.shot("06-get-waiting-menu");
    s.harness.get_by_label("Open doomworld.com").click();
    assert_eq!(s.wait_for_page(&opened), page);
    s.settle(3, Duration::ZERO);

    // Something else lands first. A person gets to say so, and then the file stays put.
    let unrelated = s.download(
        &downloads,
        "invoice-for-something-else.zip",
        b"PK-not-a-wad",
    );
    s.wait_for("the question about a download", |s| {
        s.harness.query_by_label("Not This One").is_some()
    });
    s.harness.get_by_label_contains("to Purgatory?");
    s.shot("06-get-question");
    s.harness.get_by_label("Not This One").click();
    s.settle(3, Duration::ZERO);
    assert!(s.harness.query_by_label("Not This One").is_none());
    assert!(unrelated.exists(), "declining must leave the file alone");
    s.harness.get_by_label(waiting);

    // The real one, whose name is as long as they come, asked over a modal dialog, and
    // the app is still usable underneath. The click is a pointer click on purpose, since
    // whether it reaches the button past a dialog is what's being checked.
    let long_name = "caco-screenshots-purgatory-v1.1-final-with-the-music-fix-and-more.pk3";
    let wad_bytes = b"PWAD\0\0\0\0\x0c\0\0\0 a file for purgatory";
    record(
        "get-question-over-dialog",
        s.shot_opened_by("dlg-get-question-over-dialog", |s| {
            s.harness
                .state_mut()
                .dispatch_action(ActionRequest::Settings);
            s.settle(2, Duration::ZERO);
            s.download(&downloads, long_name, wad_bytes);
            s.wait_for("the question about a download", |s| {
                s.harness.query_by_label("Not This One").is_some()
            });
        }),
    );
    s.harness.get_by_label("Link").click();
    s.wait_for("the file to be linked", |s| {
        s.harness.state_mut().get_linking().not()
            && s.harness
                .query_by_label(&format!("Upload to {MIRROR_NAME}"))
                .is_some()
    });
    {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        let wad = caco_core::db::get_wad(&conn, gotten, false)
            .expect("read WAD")
            .expect("WAD still there");
        let kept = caco_core::config::keep_dir().join(long_name);
        assert_eq!(wad.cached_path.as_deref(), kept.to_str());
        assert_eq!(std::fs::read(&kept).unwrap(), wad_bytes);
        assert!(
            !downloads.join(long_name).exists(),
            "the default link mode moves it"
        );
        assert!(wad.identity.is_some(), "linking hashes the file");
        assert_eq!(wad.last_known_path, None);
    }
    assert!(s.harness.query_by_label(waiting).is_none());
    // The issue's last step, and how a mirror fills itself.
    s.harness.get_by_label("Not Now").click();
    s.settle(3, Duration::ZERO);
    let _ = std::fs::remove_file(caco_core::config::keep_dir().join(long_name));
    s.purge_wad(gotten);

    // An absent Cacoward entry has no row to link to, and its Get knows which thread to
    // open: a forum-only release, which is what this exists for. One the mirror these
    // shots filled does not hold, or it would rightly be something caco could fetch.
    let entry = s.absent_cacoward("2025-winner-below-the-gaudy-goblin");
    s.shot_view_after("06-get-cacoward", ViewMode::Cacowards, |s| {
        s.harness.get_by_label("2025").click_accesskit();
        s.settle(3, Duration::ZERO);
        s.harness
            .state_mut()
            .dispatch_action(ActionRequest::Get(GetTarget::Cacoward(entry)));
        assert_eq!(
            s.wait_for_page(&opened),
            "https://www.doomworld.com/forum/topic/149566/"
        );
        s.settle(2, Duration::ZERO);
        s.harness.get_by_label("Waiting\u{2026}");
    });
    let goblin_name = "GaudyGoblin.pk3";
    let goblin_bytes = b"PWAD\0\0\0\0\x0c\0\0\0 a file for the goblin";
    s.download(&downloads, goblin_name, goblin_bytes);
    s.wait_for("the question about a download", |s| {
        s.harness.query_by_label("Not This One").is_some()
    });
    s.harness
        .get_by_label_contains("to Below the Gaudy Goblin?");
    s.harness.get_by_label("Link").click();
    s.wait_for("the entry to get its row and file", |s| {
        s.harness.state_mut().get_linking().not()
            && s.harness
                .query_by_label(&format!("Upload to {MIRROR_NAME}"))
                .is_some()
    });
    {
        let conn = caco_core::db::open_connection(&s.home.join("library.db")).expect("open db");
        let record = caco_core::db::get_cacoward(&conn, entry)
            .expect("read entry")
            .expect("entry still there");
        let wad_id = record.wad_id.expect("the entry should have a WAD now");
        let wad = caco_core::db::get_wad(&conn, wad_id, false)
            .expect("read WAD")
            .expect("WAD still there");
        let kept = caco_core::config::keep_dir().join(goblin_name);
        assert_eq!(wad.cached_path.as_deref(), kept.to_str());
        assert_eq!(std::fs::read(&kept).unwrap(), goblin_bytes);
        s.harness.get_by_label("Not Now").click();
        s.settle(3, Duration::ZERO);
        let _ = std::fs::remove_file(&kept);
        s.purge_wad(wad_id);
    }

    // A Get can be called off, and then the folder stops being watched.
    let stopped = s.absent_cacoward("2025-winner-below-the-gaudy-goblin");
    s.harness
        .state_mut()
        .dispatch_action(ActionRequest::Get(GetTarget::Cacoward(stopped)));
    s.wait_for_page(&opened);
    s.settle(2, Duration::ZERO);
    s.harness
        .get_by_label("Waiting for download: Below the Gaudy Goblin...")
        .click();
    s.settle(3, Duration::ZERO);
    s.harness.get_by_label("Stop Waiting").click();
    s.settle(3, Duration::ZERO);
    assert!(
        s.harness
            .query_by_label("Waiting for download: Below the Gaudy Goblin...")
            .is_none()
    );
    std::fs::write(downloads.join("after-stopping.zip"), b"PK").expect("write file");
    s.settle(5, WORKER_PAUSE * 8);
    assert!(s.harness.query_by_label("Not This One").is_none());

    // A mirror that lists a file and cannot produce it: the link dialog has to say
    // what went wrong, or it looks like a WAD no mirror ever had.
    let broken = s.add_wad_on_a_broken_mirror();
    record(
        "link-mirror-failed",
        s.shot_dialog_after("dlg-link-mirror-failed", ActionRequest::Play(broken), |s| {
            s.settle(4, WORKER_PAUSE);
            s.harness
                .get_by_label_contains(&format!("Mirror {MIRROR_NAME}:"));
        }),
    );
    s.purge_wad(broken);

    // Only Play reaches the link dialog, and only for a WAD whose file is not
    // here. Last, because the WAD it adds is in the library from then on.
    let lost = s.add_lost_wad();
    record(
        "link-last-seen",
        s.shot_dialog_after("dlg-link-last-seen", ActionRequest::Play(lost), |s| {
            s.settle(4, WORKER_PAUSE);
            s.harness.get_by_label_contains("It was last seen at");
        }),
    );
    s.purge_wad(lost);

    assert!(
        escaped.is_empty(),
        "dialogs render outside the {:?} window: {}",
        shot_size(),
        escaped.join(", ")
    );
}

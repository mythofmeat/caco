//! The main surfaces, compared against images committed beside this file.
//!
//! `screenshots.rs` shows every surface and fails on what can be decided without looking:
//! a window off the screen, an unnamed input. It cannot say that something looks different
//! from yesterday, and neither can anything else, including on the weekly dependency update,
//! which merges and releases with nobody watching. This can: an egui upgrade that moves a
//! button fails here, and the images in the pull request are what a person reviews.
//!
//! ```bash
//! cargo test -p caco --test snapshots -- --ignored --nocapture
//! UPDATE_SNAPSHOTS=1 cargo test -p caco --test snapshots -- --ignored --nocapture
//! ```
//!
//! The second writes what it renders over `tests/snapshots/*.png`, for when the change is
//! the point. A failure leaves `*.new.png` and `*.diff.png` beside them, and CI uploads those
//! as the `snapshot-changes` artifact.
//!
//! Images made on a real GPU compare equal to CI's software renderer: measured on this
//! test's predecessor, identical content differs by at most 2 levels in 255, which is well
//! inside kittest's default threshold. So they can be made on any machine.
//!
//! Nothing here reads the real library, or anything else that differs between two machines
//! or two days: the library is made up, its dates are long past and in UTC, the paths are
//! fixed, and everything that would ask the network asks a port nothing listens on.
//! `#[ignore]`d like the screenshots, because `cargo test --workspace` must not need a GPU.

use std::path::{Path, PathBuf};
use std::time::Duration;

use caco::dialogs::storage::StorageTab;
use caco::state::{ActionRequest, ViewMode};
use caco_core::db::{self, NewWad, SourceType, Status, WadUpdate};
use caco_core::wad_analysis::{ANALYSIS_VERSION, MapClassification, MapInfo, WadAnalysis};
use caco_core::wad_stats::{MapStats, WadStats};
use egui_kittest::SnapshotResults;
use egui_kittest::kittest::Queryable;

/// A fixed path rather than a temporary one, since the settings dialog shows some of it.
const HOME: &str = "/tmp/caco-snapshots";

/// A port nothing listens on: it fails at once, every time.
const NOWHERE: &str = "http://127.0.0.1:9/api.php";

const SIZES: [[f32; 2]; 2] = [[1200.0, 800.0], [800.0, 400.0]];

/// How long to leave worker threads to answer between rounds of frames.
const WORKER_PAUSE: Duration = Duration::from_millis(60);

struct FixtureWad {
    title: &'static str,
    author: &'static str,
    year: i32,
    status: Status,
    rating: Option<i64>,
    iwad: Option<&'static str>,
    complevel: Option<i64>,
    /// `(started_at, seconds, sourceport)`.
    sessions: &'static [(&'static str, i64, &'static str)],
    /// When each playthrough was finished.
    completions: &'static [&'static str],
    /// Whether a title picture is in the thumbnail cache for it. The rest get the placeholder.
    thumbnail: bool,
}

const NYAN: &str = "/usr/bin/nyan-doom";
const UZDOOM: &str = "/usr/bin/uzdoom";

const LIBRARY: &[FixtureWad] = &[
    FixtureWad {
        title: "Lonesome Road",
        author: "Thomas Seifert",
        year: 2019,
        status: Status::InProgress,
        rating: None,
        iwad: Some("doom2"),
        complevel: Some(21),
        sessions: &[
            ("2020-11-02T12:00:00.000000000+00:00", 5400, NYAN),
            ("2020-11-20T12:00:00.000000000+00:00", 7200, NYAN),
        ],
        completions: &[],
        thumbnail: true,
    },
    FixtureWad {
        title: "Sunder",
        author: "Insane_Gazebo",
        year: 2017,
        status: Status::Completed,
        rating: Some(5),
        iwad: Some("doom2"),
        complevel: Some(9),
        sessions: &[
            ("2020-03-01T12:00:00.000000000+00:00", 36000, NYAN),
            ("2020-03-15T12:00:00.000000000+00:00", 28800, NYAN),
            ("2020-09-05T12:00:00.000000000+00:00", 14400, NYAN),
        ],
        completions: &[
            "2020-03-15T13:00:00.000000000+00:00",
            "2020-09-05T13:00:00.000000000+00:00",
        ],
        thumbnail: true,
    },
    FixtureWad {
        title: "Ancient Aliens",
        author: "skillsaw",
        year: 2016,
        status: Status::Completed,
        rating: Some(4),
        iwad: Some("doom2"),
        complevel: Some(21),
        sessions: &[("2020-01-10T12:00:00.000000000+00:00", 54000, NYAN)],
        completions: &["2020-01-20T12:00:00.000000000+00:00"],
        thumbnail: false,
    },
    FixtureWad {
        title: "A Megawad With Rather Too Long a Title to Fit on Any Card",
        author: "A Community Project of Thirty-Two Mappers, More or Less",
        year: 2018,
        status: Status::Abandoned,
        rating: Some(2),
        iwad: Some("tnt"),
        complevel: Some(4),
        sessions: &[("2019-12-05T12:00:00.000000000+00:00", 3600, UZDOOM)],
        completions: &[],
        thumbnail: true,
    },
    FixtureWad {
        title: "Valiant",
        author: "skillsaw",
        year: 2015,
        status: Status::Completed,
        rating: Some(5),
        iwad: Some("doom2"),
        complevel: Some(9),
        sessions: &[("2019-06-14T12:00:00.000000000+00:00", 90000, NYAN)],
        completions: &["2019-07-01T12:00:00.000000000+00:00"],
        thumbnail: true,
    },
    FixtureWad {
        title: "Eviternity",
        author: "Dragonfly",
        year: 2018,
        status: Status::Unplayed,
        rating: None,
        iwad: Some("doom2"),
        complevel: Some(21),
        sessions: &[],
        completions: &[],
        thumbnail: false,
    },
    FixtureWad {
        title: "Plutonia 2",
        author: "Team Plutonia",
        year: 2009,
        status: Status::InProgress,
        rating: Some(3),
        iwad: Some("plutonia"),
        complevel: Some(2),
        sessions: &[("2020-05-22T12:00:00.000000000+00:00", 10800, NYAN)],
        completions: &["2019-11-30T12:00:00.000000000+00:00"],
        thumbnail: true,
    },
    FixtureWad {
        title: "Hedon Bloodrite",
        author: "Zan_HedonDev",
        year: 2019,
        status: Status::Unplayed,
        rating: None,
        iwad: None,
        complevel: None,
        sessions: &[],
        completions: &[],
        thumbnail: true,
    },
    FixtureWad {
        title: "Going Down",
        author: "Cyriak",
        year: 2013,
        status: Status::Completed,
        rating: Some(5),
        iwad: Some("doom2"),
        complevel: Some(9),
        sessions: &[("2020-02-02T12:00:00.000000000+00:00", 25200, UZDOOM)],
        completions: &["2020-02-08T12:00:00.000000000+00:00"],
        thumbnail: false,
    },
    FixtureWad {
        title: "Scythe 2",
        author: "Erik Alm",
        year: 2005,
        status: Status::Completed,
        rating: Some(4),
        iwad: Some("doom2.wad"),
        complevel: Some(2),
        // Marked finished by hand, as a WAD finished before caco was: no sessions at all.
        sessions: &[],
        completions: &[],
        thumbnail: true,
    },
];

/// Dataset entries the fixture has in its library, for the year the Cacowards shots open on.
const CACOWARD_LINKS: &[(&str, &str)] = &[
    ("2023-winner-my-house", "Lonesome Road"),
    ("2023-winner-sepia", "Sunder"),
    ("2023-winner-godless-night", "Ancient Aliens"),
    ("2023-winner-dreamblood", "Eviternity"),
    (
        "2023-winner-hardfest-2",
        "A Megawad With Rather Too Long a Title to Fit on Any Card",
    ),
];

fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    // Before any thread that reads the environment exists: this is the first thing the one
    // test in this binary does.
    unsafe { std::env::set_var(key, value) };
}

/// Point everything caco reads at a directory of its own. Everything, since a value from the
/// machine this runs on would show up in a picture on the machine it is compared on.
fn isolated_home() -> PathBuf {
    let home = PathBuf::from(HOME);
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("bin")).expect("create scratch home");

    set_env("TZ", "UTC");
    set_env("CACO_HOME", home.join("data"));
    set_env("CACO_DB_PATH", home.join("data/library.db"));
    set_env("CACO_CONFIG", home.join("data/config.toml"));
    set_env("CACO_CACHE_HOME", home.join("cache"));
    set_env("CACO_CACHE_DIR", home.join("cache/wads"));
    set_env("CACO_DATA_DIR", home.join("data/data"));
    set_env("CACO_SYNC_DIR", home.join("data/sync"));
    set_env("CACO_TOKEN_FILE", home.join("machine/tokens.toml"));
    set_env("CACO_MACHINE_CONFIG", home.join("machine/machine.toml"));
    set_env("CACO_MACHINE_ID", home.join("machine/machine-id"));
    set_env("CACO_IDGAMES_INDEX", home.join("cache/idgames-index.db"));
    set_env("CACO_IDGAMES_API", NOWHERE);
    set_env("CACO_DOOMWIKI_API", NOWHERE);

    // Settings only draws its "detected" controls with a sourceport on `PATH`.
    std::fs::write(home.join("bin/nyan-doom"), "").expect("create scratch sourceport");
    let path = std::env::var("PATH").unwrap_or_default();
    set_env("PATH", format!("{}:{path}", home.join("bin").display()));

    std::fs::create_dir_all(home.join("data")).expect("create scratch data dir");
    let mut config = (*caco_core::config::load_config()).clone();
    config.sourceport = "nyan-doom".to_string();
    config.zdoom_sourceport = "uzdoom".to_string();
    config.idgames_index_check_days = 0;
    config.sourceport_update_check_days = 0;
    config.auto_doomwiki_enrich = false;
    config.downloads_dir = home.join("Downloads").display().to_string();
    caco_core::config::save_config(&config).expect("save scratch config");
    caco_core::config::reload_config();

    home
}

/// A plain picture to stand in for a title screen: enough to see where it lands, and small
/// in a PNG, since these end up in the repository many times over.
fn write_thumbnail(cache: &Path, wad_id: i64) {
    let dir = cache.join("thumbnails/v2");
    std::fs::create_dir_all(&dir).expect("create thumbnail cache");
    let shade = (wad_id % 5) as u8 * 30;
    let image = image::RgbaImage::from_fn(160, 100, |x, y| {
        if (x / 20 + y / 20).is_multiple_of(2) {
            image::Rgba([120 + shade, 60, 40 + shade / 2, 255])
        } else {
            image::Rgba([60, 40 + shade / 2, 80 + shade, 255])
        }
    });
    image
        .save(dir.join(format!("{wad_id}.png")))
        .expect("write thumbnail");
}

fn map_stats(lump: &str) -> MapStats {
    MapStats {
        lump: lump.to_string(),
        total_exits: 1,
        best_skill: 4,
        best_time: 7000,
        time_secs: 200.0,
        kills: 90,
        total_kills: 100,
        items: 20,
        total_items: 25,
        secrets: 3,
        total_secrets: 5,
        episode: 0,
        map_num: 0,
        best_max_time: -1,
        best_nm_time: -1,
        cumulative_kills: 0,
        total_time_secs: -1.0,
    }
}

/// A WAD part of the way through, for the progress the library shows of one.
fn record_progress(conn: &rusqlite::Connection, wad_id: i64) {
    let maps: Vec<String> = (1..=8).map(|n| format!("MAP{n:02}")).collect();
    let analysis = WadAnalysis {
        version: ANALYSIS_VERSION,
        maps: maps
            .iter()
            .map(|lump| MapInfo {
                lump: lump.clone(),
                classification: MapClassification::Required,
            })
            .collect(),
        total_maps: maps.len(),
        required_maps: maps.len(),
        secret_maps: Vec::new(),
        terminal_map: None,
        has_umapinfo: true,
    };
    db::save_analysis(conn, wad_id, &analysis).expect("save analysis");
    let stats = WadStats {
        format: "stats_txt".to_string(),
        maps: maps[..3].iter().map(|lump| map_stats(lump)).collect(),
        version: 1,
        header_total_kills: 0,
    };
    let json = caco_core::wad_stats::stats_to_json(&stats).expect("write stats");
    let update = WadUpdate::new().set_text("stats_snapshot", Some(json));
    db::update_wad(conn, wad_id, &update).expect("record stats");
}

fn fill_library(conn: &rusqlite::Connection, home: &Path) {
    for wad in LIBRARY {
        let id = db::add_wad(
            conn,
            &NewWad::new(wad.title, SourceType::Local)
                .author(wad.author)
                .year(wad.year)
                .status(wad.status),
        )
        .expect("add WAD");
        let update = WadUpdate::new()
            .set_int("rating", wad.rating)
            .set_text("custom_iwad", wad.iwad.map(str::to_string))
            .set_int("complevel", wad.complevel);
        db::update_wad(conn, id, &update).expect("set details");
        for (started_at, seconds, sourceport) in wad.sessions {
            conn.execute(
                "INSERT INTO sessions (wad_id, started_at, duration_seconds, sourceport) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, started_at, seconds, sourceport],
            )
            .expect("add session");
        }
        for completed_at in wad.completions {
            db::add_wad_completion(conn, id, None, None, Some(completed_at))
                .expect("add completion");
        }
        if wad.thumbnail {
            write_thumbnail(&home.join("cache"), id);
        }
        if wad.status == Status::InProgress {
            record_progress(conn, id);
        }
    }
}

/// Link the fixture's WADs to the awards they stand in for. The dataset is already in the
/// table by then, because the app writes it there as it is created, which this waits for.
fn link_cacowards(conn: &rusqlite::Connection) {
    for (key, title) in CACOWARD_LINKS {
        let wad_id: i64 = conn
            .query_row("SELECT id FROM wads WHERE title = ?1", [title], |row| {
                row.get(0)
            })
            .expect("the fixture has this WAD");
        let entry: i64 = conn
            .query_row("SELECT id FROM cacowards WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .expect("the dataset has this entry");
        db::link_wad(conn, entry, wad_id, true).expect("link entry");
    }
}

struct Shooter {
    harness: egui_kittest::Harness<'static, caco::app::CacoApp>,
    size: [f32; 2],
}

impl Shooter {
    fn new(size: [f32; 2], db_path: &Path) -> Self {
        let conn = db::open_connection(db_path).expect("open library");
        let db_path = db_path.to_path_buf();
        let harness = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::from(size))
            .build_eframe(|cc| {
                caco::theme::apply_doom_theme(&cc.egui_ctx);
                caco::app::CacoApp::new(conn, db_path, &cc.egui_ctx)
            });
        let mut shooter = Self { harness, size };
        shooter.settle();
        shooter
    }

    /// Frames until nothing is on its way. `run_ok` rather than `run`, for the reason
    /// `screenshots.rs` gives: a worker's repaint request would make `run` fail at random.
    fn settle(&mut self) {
        for _ in 0..6 {
            self.harness.run_ok();
            std::thread::sleep(WORKER_PAUSE);
        }
        for _ in 0..100 {
            if self.harness.state().thumbnails_pending() == 0 {
                break;
            }
            self.harness.run_ok();
            std::thread::sleep(WORKER_PAUSE);
        }
        self.harness.state_mut().dismiss_notification();
        for _ in 0..3 {
            self.harness.run_ok();
        }
    }

    fn snap(&mut self, surface: &str) {
        self.settle();
        let [width, height] = self.size;
        self.harness.snapshot(format!("{surface}-{width}x{height}"));
    }

    fn snap_dialog(&mut self, surface: &str, action: ActionRequest) {
        self.harness.state_mut().dispatch_action(action);
        self.snap(surface);
        self.harness.state_mut().close_dialog();
        self.settle();
    }

    fn view(&mut self, mode: ViewMode) {
        self.harness.state_mut().set_view(mode);
        self.settle();
    }

    fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click_accesskit();
        self.settle();
    }

    fn all_surfaces(&mut self, first_wad: i64) {
        self.snap("library-grid");
        self.click("List");
        self.snap("library-list");
        self.click("Grid");

        self.view(ViewMode::Stats);
        self.snap("stats");

        self.view(ViewMode::Cacowards);
        self.click("2023");
        self.snap("cacowards");

        self.view(ViewMode::Import);
        self.snap("import");
        self.view(ViewMode::Library);

        for (surface, action) in [
            ("settings", ActionRequest::Settings),
            ("enrich", ActionRequest::Enrich),
            ("storage-clean", ActionRequest::Storage(StorageTab::Clean)),
            ("storage-trash", ActionRequest::Storage(StorageTab::Trash)),
            ("iwads", ActionRequest::Resources),
            ("profiles", ActionRequest::Profiles),
            ("edit", ActionRequest::Edit(first_wad)),
            ("delete", ActionRequest::Delete(first_wad)),
            ("sessions", ActionRequest::Sessions(first_wad)),
            ("map-stats", ActionRequest::MapStats(first_wad)),
        ] {
            self.snap_dialog(surface, action);
        }
    }
}

#[test]
#[ignore = "needs a GPU adapter; run explicitly"]
fn the_main_surfaces_look_as_they_did() {
    let home = isolated_home();
    let db_path = home.join("data/library.db");

    let conn = db::open_connection(&db_path).expect("open library");
    db::init_db(&conn).expect("init library");
    fill_library(&conn, &home);
    let first_wad = conn
        .query_row(
            "SELECT id FROM wads WHERE title = ?1",
            [LIBRARY[0].title],
            |row| row.get(0),
        )
        .expect("the fixture has this WAD");

    let mut results = SnapshotResults::new();
    for (index, size) in SIZES.into_iter().enumerate() {
        let mut shooter = Shooter::new(size, &db_path);
        if index == 0 {
            // The app has written the dataset to the table by now, and the next reload
            // picks these up.
            link_cacowards(&conn);
            shooter.harness.state_mut().set_view(ViewMode::Library);
        }
        shooter.all_surfaces(first_wad);
        results.extend(shooter.harness.take_snapshot_results());
    }
    results.unwrap();
}

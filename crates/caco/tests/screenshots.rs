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

use std::path::{Path, PathBuf};
use std::time::Duration;

use caco::dialogs::storage::StorageTab;
use caco::import::state::SearchSource;
use caco::message::{AppMessage, EnrichReport};
use caco::state::{ActionRequest, ViewMode};
use caco_core::sourceports::RemoteRefs;
use caco_sources::SourceError;
use caco_sources::enrich_service::WikiBlocked;
use caco_sources::http::WafKind;
use egui_kittest::kittest::Queryable;

/// How long to leave worker threads to answer between rounds of frames.
const WORKER_PAUSE: Duration = Duration::from_millis(80);

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
    if real_db.exists() && real_db != scratch_db {
        std::fs::copy(&real_db, &scratch_db).expect("copy library");
    }

    unsafe { std::env::set_var("CACO_HOME", &home) };
    put_sourceport_on_path(&home);
    home
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

        let mut me = Self { harness };
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
        let img = self
            .harness
            .render()
            .unwrap_or_else(|e| panic!("render {name}: {e:?}"));
        let path = out_dir().join(format!("{name}.png"));
        img.save(&path)
            .unwrap_or_else(|e| panic!("save {name}: {e}"));
        println!("wrote {}", path.display());
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
        self.harness.state_mut().dispatch_action(action);
        self.settle(4, WORKER_PAUSE);
        after_open(self);
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
        tx.send(AppMessage::EnrichComplete(Ok(EnrichReport::Wads {
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

    /// Largest distance by which any floating window escapes the viewport.
    ///
    /// Read off egui's own area bookkeeping rather than the pixels, because a
    /// window clipped at the screen edge and a window that merely happens to
    /// end there look identical in a PNG.
    fn worst_overflow(&self) -> f32 {
        let ctx = &self.harness.ctx;
        let screen = ctx.content_rect();
        let layers: Vec<egui::LayerId> = ctx.memory(|m| {
            m.areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|l| l.order == egui::Order::Middle)
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

    assert!(
        escaped.is_empty(),
        "dialogs render outside the {:?} window: {}",
        shot_size(),
        escaped.join(", ")
    );
}

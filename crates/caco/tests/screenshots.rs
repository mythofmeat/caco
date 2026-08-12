//! Off-screen screenshots of every GUI surface.
//!
//! egui is immediate-mode and this app has no unit-testable layout: whether a
//! row fits inside its panel is only decided at paint time, against a real
//! font atlas and a real viewport size. So the only way to catch "this widget
//! is clipped" or "this window is taller than the window it lives in" is to
//! render a frame and look at it.
//!
//! `egui_kittest` renders through wgpu with no display server, so this runs
//! over ssh, in a tty, or in CI with a software adapter. It is `#[ignore]`d
//! for the same reason `ports_build.rs` is: `cargo test --workspace` must not
//! depend on a GPU. Run it explicitly:
//!
//! ```bash
//! cargo test -p caco --test screenshots -- --ignored --nocapture
//! ```
//!
//! PNGs land in `target/screenshots/`. `CACO_SHOT_SIZE=1200x800` overrides the
//! viewport; small sizes are the interesting ones, since that is where
//! fixed-size dialogs overflow.
//!
//! The library is a *copy* of the real database into a temp `CACO_HOME`, so
//! the shots show real WADs while nothing can write to the real library.

use std::path::{Path, PathBuf};

use caco::state::ActionRequest;

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

fn out_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/screenshots")
        .canonicalize()
        .unwrap_or_else(|_| {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
            std::fs::create_dir_all(&p).ok();
            p
        });
    std::fs::create_dir_all(&dir).ok();
    dir
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
    home
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

        // `CacoApp::new` only keeps this context to poke for a repaint when a
        // worker finishes; the harness drives frames by hand, so a throwaway
        // one is fine. The theme has to land on the context the harness
        // actually paints with, which only exists after `build_state`.
        let seed_ctx = egui::Context::default();
        let app = caco::app::CacoApp::new(conn, db_path, &seed_ctx);

        let harness = egui_kittest::Harness::builder()
            .with_size(shot_size())
            .build_state(|ctx, app: &mut caco::app::CacoApp| app.render(ctx), app);

        let mut me = Self { harness };
        caco::theme::apply_doom_theme(&me.harness.ctx);
        // Settle: first frame loads the library, later frames pick up
        // thumbnails as the worker threads answer.
        for _ in 0..8 {
            me.harness.run();
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        me
    }

    fn shot(&mut self, name: &str) {
        for _ in 0..3 {
            self.harness.run();
        }
        let img = self
            .harness
            .render()
            .unwrap_or_else(|e| panic!("render {name}: {e:?}"));
        let path = out_dir().join(format!("{name}.png"));
        img.save(&path)
            .unwrap_or_else(|e| panic!("save {name}: {e}"));
        println!("wrote {}", path.display());
    }

    /// Open a dialog through the same path a click takes, shoot it, close it.
    ///
    /// Returns how far the dialog spills outside the app window, per edge, in
    /// points. Anything positive means a user at this window size cannot reach
    /// part of the dialog — a title bar above the top edge, a Close button
    /// below the bottom one.
    fn shot_dialog(&mut self, name: &str, action: ActionRequest) -> f32 {
        self.harness.state_mut().dispatch_action(action);
        for _ in 0..4 {
            self.harness.run();
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        self.shot(name);
        let overflow = self.worst_overflow();
        self.harness.state_mut().close_dialog();
        self.harness.run();
        overflow
    }

    /// Largest distance by which any floating window escapes the viewport.
    ///
    /// Read off egui's own area bookkeeping rather than the pixels, because a
    /// window clipped at the screen edge and a window that merely happens to
    /// end there look identical in a PNG.
    fn worst_overflow(&self) -> f32 {
        let ctx = &self.harness.ctx;
        let screen = ctx.screen_rect();
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

    let mut escaped = Vec::new();
    for (name, action) in [
        ("stats", ActionRequest::Stats),
        ("cache", ActionRequest::Cache),
        ("profiles", ActionRequest::Profiles),
        ("companions", ActionRequest::Companions),
        ("enrich", ActionRequest::Enrich),
        ("gc", ActionRequest::Gc),
        ("trash", ActionRequest::Trash),
        ("resources", ActionRequest::Resources),
        ("ports", ActionRequest::Ports),
        ("collections", ActionRequest::Collections),
        ("settings", ActionRequest::Settings),
    ] {
        let over = s.shot_dialog(&format!("dlg-{name}"), action);
        println!("  {name}: overflow {over:.0}pt");
        // 1pt of slack absorbs rounding; anything more is a real spill.
        if over > 1.0 {
            escaped.push(format!("{name} (+{over:.0}pt)"));
        }
    }

    assert!(
        escaped.is_empty(),
        "dialogs render outside the {:?} window: {}",
        shot_size(),
        escaped.join(", ")
    );
}

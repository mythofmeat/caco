use std::path::PathBuf;
use std::sync::Arc;

/// `assets/caco.svg`, rasterised by `build.rs` to square straight RGBA.
const ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon.rgba"));

fn icon() -> egui::IconData {
    let side = (ICON_RGBA.len() / 4).isqrt() as u32;
    egui::IconData {
        rgba: ICON_RGBA.to_vec(),
        width: side,
        height: side,
    }
}

fn main() -> eframe::Result<()> {
    // Parse optional --db-path argument
    let args: Vec<String> = std::env::args().collect();
    let db_path = if let Some(idx) = args.iter().position(|a| a == "--db-path") {
        args.get(idx + 1).map(PathBuf::from)
    } else {
        None
    };

    // Adopt an installed sourceport when none is configured, so a fresh
    // install can launch something without a trip to Settings first.
    let detected = caco_core::config::ensure_sourceport_defaults();
    if let Some(ref sourceport) = detected.sourceport {
        eprintln!("Detected sourceport: {sourceport}");
    }

    let db_path = db_path.unwrap_or_else(caco_core::config::get_db_path);

    // `app_id` must match the `.desktop` file basename so Wayland
    // compositors map the window to caco.desktop and show its icon.
    // Without this, Wayland ignores `with_icon()` and falls back to a
    // generic icon.
    let viewport = egui::ViewportBuilder::default()
        .with_app_id("caco")
        .with_title("Caco")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([800.0, 400.0])
        .with_icon(Arc::new(icon()));

    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "caco",
        options,
        Box::new(move |cc| {
            // Apply Doom theme
            caco::theme::apply_doom_theme(&cc.egui_ctx);

            // Open database
            let conn = caco_core::db::open_connection(&db_path).expect("Failed to open database");
            caco_core::db::init_db(&conn).expect("Failed to initialize database");

            // Repair cached_path values left dangling by a cache relocation —
            // the cache directory is editable in Settings, and every row
            // records an absolute path into wherever it used to be.
            let relinked =
                caco_core::db::relink_cached_paths(&conn, &caco_core::config::get_cache_dir())
                    .unwrap_or(0);

            let mut app = caco::app::CacoApp::new(conn, db_path.clone(), &cc.egui_ctx);
            if let Some(ref sourceport) = detected.sourceport {
                app.notify(format!("Detected {sourceport}. Change it in Settings."));
            } else if relinked > 0 {
                app.notify(format!("Relinked {relinked} cached WAD file(s)."));
            }
            Ok(Box::new(app))
        }),
    )
}

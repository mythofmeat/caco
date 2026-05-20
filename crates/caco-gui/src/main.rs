use std::path::PathBuf;
use std::sync::Arc;

const ICON_BYTES: &[u8] = include_bytes!("../assets/caco.png");

/// Decode the embedded icon PNG into `egui::IconData`. Returns `None` if
/// decoding fails so a bad asset never panics startup.
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_BYTES).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

fn main() -> eframe::Result<()> {
    // Parse optional --db-path argument
    let args: Vec<String> = std::env::args().collect();
    let db_path = if let Some(idx) = args.iter().position(|a| a == "--db-path") {
        args.get(idx + 1).map(PathBuf::from)
    } else {
        None
    };

    let db_path = db_path.unwrap_or_else(caco_core::config::get_db_path);

    // `app_id` must match the `.desktop` file basename so Wayland
    // compositors map the window to caco.desktop and show its icon.
    // Without this, Wayland ignores `with_icon()` and falls back to a
    // generic icon because the eframe app name (`caco-gui`) doesn't
    // match `caco.desktop`.
    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("caco")
        .with_title("Caco")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([800.0, 400.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "caco-gui",
        options,
        Box::new(move |cc| {
            // Apply Doom theme
            caco_gui::theme::apply_doom_theme(&cc.egui_ctx);

            // Open database
            let conn = caco_core::db::open_connection(&db_path).expect("Failed to open database");
            caco_core::db::init_db(&conn).expect("Failed to initialize database");

            Ok(Box::new(caco_gui::app::CacoApp::new(
                conn,
                db_path.clone(),
                &cc.egui_ctx,
            )))
        }),
    )
}

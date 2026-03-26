use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Parse optional --db-path argument
    let args: Vec<String> = std::env::args().collect();
    let db_path = if let Some(idx) = args.iter().position(|a| a == "--db-path") {
        args.get(idx + 1).map(PathBuf::from)
    } else {
        None
    };

    let db_path = db_path.unwrap_or_else(caco_core::config::get_db_path);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Caco")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 400.0]),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native("caco-gui", options, Box::new(move |cc| {
        // Apply Doom theme
        caco_gui::theme::apply_doom_theme(&cc.egui_ctx);

        // Open database
        let conn = caco_core::db::open_connection(&db_path)
            .expect("Failed to open database");
        caco_core::db::init_db(&conn)
            .expect("Failed to initialize database");

        Ok(Box::new(caco_gui::app::CacoApp::new(conn, db_path.clone(), &cc.egui_ctx)))
    }))
}

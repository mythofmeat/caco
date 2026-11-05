//! How the app behaves when it is used: keys pressed and things clicked, through the real
//! `eframe::App` and the real workers.
//!
//! `screenshots.rs` and `snapshots.rs` say what the app looks like, and both need a GPU, so
//! neither is part of `cargo test`. Nothing here renders: egui lays everything out and paints
//! into a list without one, which is all it takes to ask what is on the screen and where.
//!
//! Everything points at a temporary directory, set up once for the whole binary, since the
//! environment is global. Each test has a library of its own in there.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use caco::state::ViewMode;
use caco_core::db::{self, NewWad, SourceType};
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;

const DEFAULT_WINDOW: [f32; 2] = [1200.0, 800.0];
const SMALLEST_WINDOW: [f32; 2] = [800.0, 400.0];

/// A port nothing listens on: it fails at once, every time.
const NOWHERE: &str = "http://127.0.0.1:9/api.php";

fn scratch() -> &'static Path {
    static SCRATCH: OnceLock<tempfile::TempDir> = OnceLock::new();
    SCRATCH
        .get_or_init(|| {
            let dir = tempfile::tempdir().expect("create scratch dir");
            let home = dir.path();
            let set = |key: &str, value: PathBuf| unsafe { std::env::set_var(key, value) };
            // Before anything that reads the environment, by this being the only way in.
            set("CACO_HOME", home.join("data"));
            set("CACO_CONFIG", home.join("data/config.toml"));
            set("CACO_CACHE_HOME", home.join("cache"));
            set("CACO_CACHE_DIR", home.join("cache/wads"));
            set("CACO_DATA_DIR", home.join("data/data"));
            set("CACO_TOKEN_FILE", home.join("machine/tokens.toml"));
            set("CACO_MACHINE_CONFIG", home.join("machine/machine.toml"));
            set("CACO_MACHINE_ID", home.join("machine/machine-id"));
            set("CACO_IDGAMES_INDEX", home.join("cache/idgames-index.db"));
            set("CACO_IDGAMES_API", PathBuf::from(NOWHERE));
            set("CACO_DOOMWIKI_API", PathBuf::from(NOWHERE));

            std::fs::create_dir_all(home.join("data")).expect("create scratch data dir");
            let mut config = (*caco_core::config::load_config()).clone();
            config.idgames_index_check_days = 0;
            config.sourceport_update_check_days = 0;
            config.auto_doomwiki_enrich = false;
            caco_core::config::save_config(&config).expect("save scratch config");
            caco_core::config::reload_config();
            dir
        })
        .path()
}

/// A library of `count` WADs called `WAD 000` and so on, in a folder of its own. A library
/// outside the data dir keeps its sync log beside it, and two sharing a folder would read each
/// other's WADs in as their own.
fn library(name: &str, count: usize) -> PathBuf {
    let dir = scratch().join(name);
    std::fs::create_dir_all(&dir).expect("create library dir");
    let path = dir.join("library.db");
    let conn = db::open_connection(&path).expect("open library");
    db::init_db(&conn).expect("init library");
    for n in 0..count {
        let title = format!("WAD {n:03}");
        db::add_wad(&conn, &NewWad::new(&title, SourceType::Local)).expect("add WAD");
    }
    path
}

type Harness = egui_kittest::Harness<'static, caco::app::CacoApp>;

fn open_app(db_path: &Path, size: [f32; 2]) -> Harness {
    let conn = db::open_connection(db_path).expect("open library");
    let db_path = db_path.to_path_buf();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::Vec2::from(size))
        .build_eframe(|cc| {
            caco::theme::apply_doom_theme(&cc.egui_ctx);
            caco::app::CacoApp::new(conn, db_path, &cc.egui_ctx)
        });
    settle(&mut harness);
    harness
}

/// `run_ok` rather than `run`: workers wake the context, and `run` fails on a UI that keeps
/// being asked to repaint. See `screenshots.rs`.
fn settle(harness: &mut Harness) {
    for _ in 0..4 {
        harness.run_ok();
    }
}

fn press(harness: &mut Harness, key: egui::Key) {
    harness.key_press(key);
    settle(harness);
}

fn type_text(harness: &mut Harness, text: &str) {
    harness.event(egui::Event::Text(text.to_string()));
    settle(harness);
}

/// The selected card is on the screen, all of it, between the bars at the top and bottom.
fn assert_selection_is_on_screen(harness: &Harness, size: [f32; 2]) {
    let title = harness.state().selected_wad_title().expect("a selection");
    let label = format!("{title}, Unplayed");
    let card = harness
        .query_by_label(&label)
        .unwrap_or_else(|| panic!("{title} is not on the screen"));
    let rect = card.rect();
    assert!(
        rect.min.y >= 0.0 && rect.max.y <= size[1],
        "{title}: {rect:?}"
    );
}

#[test]
fn moving_with_the_keyboard_keeps_the_selection_on_the_screen() {
    let db_path = library("keep-on-screen", 60);
    for size in [DEFAULT_WINDOW, SMALLEST_WINDOW] {
        let mut app = open_app(&db_path, size);
        let first = app.state().selected_wad_title().unwrap().to_string();

        for _ in 0..12 {
            press(&mut app, egui::Key::J);
            assert_selection_is_on_screen(&app, size);
        }
        assert_ne!(app.state().selected_wad_title().unwrap(), first);

        press(&mut app, egui::Key::End);
        assert_selection_is_on_screen(&app, size);
        press(&mut app, egui::Key::PageUp);
        assert_selection_is_on_screen(&app, size);
        press(&mut app, egui::Key::Home);
        assert_eq!(app.state().selected_wad_title().unwrap(), first);
        assert_selection_is_on_screen(&app, size);
    }
}

#[test]
fn moving_through_the_list_keeps_the_selection_on_the_screen() {
    let db_path = library("list-on-screen", 200);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.get_by_label("List").click_accesskit();
    settle(&mut app);

    let row_of_selection = |app: &Harness| {
        // Its row, and not the panel beside it, which says the same.
        let title = app.state().selected_wad_title().unwrap();
        let in_table = app
            .query_all_by_label(title)
            .find(|node| node.rect().min.x < 400.0)
            .unwrap_or_else(|| panic!("{title} is not on the screen"));
        let rect = in_table.rect();
        assert!(rect.max.y <= DEFAULT_WINDOW[1], "{title}: {rect:?}");
    };

    press(&mut app, egui::Key::End);
    row_of_selection(&app);
    press(&mut app, egui::Key::PageUp);
    row_of_selection(&app);
    press(&mut app, egui::Key::Home);
    row_of_selection(&app);
    for _ in 0..40 {
        press(&mut app, egui::Key::ArrowDown);
    }
    row_of_selection(&app);
}

/// With nothing selected nothing should look selected either, and Enter should have nothing to
/// act on. The first key after that picks up from the start.
#[test]
fn escape_deselects_and_the_next_key_starts_over() {
    let db_path = library("escape", 5);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    let first = app.state().selected_wad_title().unwrap().to_string();
    press(&mut app, egui::Key::J);
    press(&mut app, egui::Key::Escape);
    assert_eq!(app.state().selected_wad_title(), None);

    // Nothing to play: this would open a dialog on a missing file.
    press(&mut app, egui::Key::Enter);
    assert!(
        app.query_by_role_and_label(Role::Window, "WAD Unavailable")
            .is_none()
    );

    press(&mut app, egui::Key::J);
    assert_eq!(app.state().selected_wad_title().unwrap(), first);
}

#[test]
fn a_slash_goes_to_the_filter_and_is_not_typed_into_it() {
    let db_path = library("slash", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    type_text(&mut app, "/");

    let filter = app.get_by_label("Filter WADs");
    assert!(filter.is_focused());
    assert_eq!(filter.value().as_deref(), Some(""));
}

#[test]
fn typing_in_the_filter_does_not_trigger_the_librarys_keys() {
    let db_path = library("filter-typing", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    let first = app.state().selected_wad_title().unwrap().to_string();

    type_text(&mut app, "/");
    type_text(&mut app, "j");
    press(&mut app, egui::Key::J);

    assert_eq!(app.state().selected_wad_title().unwrap(), first);
    assert!(app.query_by_role_and_label(Role::Window, "Help").is_none());
}

#[test]
fn a_question_mark_opens_help() {
    let db_path = library("help", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    type_text(&mut app, "?");
    assert!(app.query_by_role_and_label(Role::Window, "Help").is_some());
}

#[test]
fn the_views_have_keys_in_the_order_the_sidebar_has_them() {
    let db_path = library("view-keys", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    app.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    settle(&mut app);
    assert!(app.query_by_label("Search idgames").is_some(), "Import");

    app.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num4);
    settle(&mut app);
    assert!(app.query_by_label("Playtime").is_some(), "Stats");

    app.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    settle(&mut app);
    assert!(app.query_by_label("Filter WADs").is_some(), "Library");
}

#[test]
fn deleting_with_the_delete_key_asks_first() {
    let db_path = library("delete-key", 2);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    press(&mut app, egui::Key::Delete);
    assert!(
        app.query_by_role_and_label(Role::Window, "Delete WAD")
            .is_some()
    );
}

#[test]
fn searching_from_import_goes_to_its_own_box() {
    let db_path = library("import-search", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Import);
    settle(&mut app);

    type_text(&mut app, "/");

    assert!(app.get_by_label("Search idgames").is_focused());
}

/// The library with one WAD selected and something to read about it.
fn described_library(name: &str) -> PathBuf {
    let path = library(name, 3);
    let conn = db::open_connection(&path).expect("open library");
    let id: i64 = conn
        .query_row("SELECT id FROM wads WHERE title = 'WAD 000'", [], |row| {
            row.get(0)
        })
        .expect("the library has this WAD");
    let update = db::WadUpdate::new().set_text(
        "description",
        Some("A road out of a dead city.".to_string()),
    );
    db::update_wad(&conn, id, &update).expect("describe WAD");
    db::add_tag(&conn, id, "megawad").expect("tag WAD");
    path
}

#[test]
fn the_panel_says_what_is_known_about_whatever_is_selected() {
    let db_path = described_library("panel-follows");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    assert!(app.query_by_label("A road out of a dead city.").is_some());
    assert!(app.query_by_label("megawad").is_some());

    press(&mut app, egui::Key::L);

    assert_ne!(app.state().selected_wad_title().unwrap(), "WAD 000");
    assert!(app.query_by_label("A road out of a dead city.").is_none());
}

#[test]
fn details_can_be_put_away_and_come_back_where_there_is_room() {
    let db_path = described_library("panel-toggle");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    press(&mut app, egui::Key::I);
    assert!(app.query_by_label("A road out of a dead city.").is_none());

    app.get_by_label("Details").click_accesskit();
    settle(&mut app);
    assert!(app.query_by_label("A road out of a dead city.").is_some());
}

#[test]
fn a_window_with_no_room_for_the_panel_opens_details_in_a_dialog() {
    let db_path = described_library("details-dialog");
    let mut app = open_app(&db_path, SMALLEST_WINDOW);
    assert!(
        app.query_by_label("A road out of a dead city.").is_none(),
        "there is no room for it beside the library"
    );

    press(&mut app, egui::Key::I);
    assert!(
        app.query_by_role_and_label(Role::Window, "WAD 000")
            .is_some()
    );
    assert!(app.query_by_label("A road out of a dead city.").is_some());

    press(&mut app, egui::Key::Escape);
    assert!(
        app.query_by_role_and_label(Role::Window, "WAD 000")
            .is_none()
    );
    // Escape was the dialog's, and said nothing about the selection.
    assert_eq!(app.state().selected_wad_title(), Some("WAD 000"));
}

#[test]
fn a_rating_is_given_by_clicking_on_it_and_taken_away_the_same_way() {
    let db_path = described_library("rating");
    let conn = db::open_connection(&db_path).expect("open library");
    let rating_of_first = || -> Option<i32> {
        conn.query_row(
            "SELECT rating FROM wads WHERE title = 'WAD 000'",
            [],
            |row| row.get(0),
        )
        .expect("the library has this WAD")
    };
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    app.get_by_label("Rate 4 stars").click_accesskit();
    settle(&mut app);
    assert_eq!(rating_of_first(), Some(4));

    app.get_by_label("Rate 4 stars").click_accesskit();
    settle(&mut app);
    assert_eq!(rating_of_first(), None);
}

#[test]
fn a_tag_in_the_panel_filters_the_library_by_it() {
    let db_path = described_library("tag-filter");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    app.get_by_label("megawad").click_accesskit();
    settle(&mut app);

    let filter = app.get_by_label("Filter WADs");
    assert_eq!(filter.value().as_deref(), Some("tag:megawad"));
}

fn wads_in_library(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM wads WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )
    .expect("count WADs")
}

/// Deleting is one key away, so taking it back should be too: the Trash is three clicks off.
#[test]
fn a_delete_can_be_undone_from_the_status_bar() {
    let db_path = library("undo-delete", 3);
    let conn = db::open_connection(&db_path).expect("open library");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    let first = app.state().selected_wad_title().unwrap().to_string();

    press(&mut app, egui::Key::Delete);
    app.get_by_label("Delete").click_accesskit();
    settle(&mut app);
    assert_eq!(wads_in_library(&conn), 2);
    assert!(
        app.query_by_label(&format!("Moved {first} to the Trash."))
            .is_some()
    );
    assert_ne!(app.state().selected_wad_title().unwrap(), first);

    app.get_by_label("Undo").click_accesskit();
    settle(&mut app);
    assert_eq!(wads_in_library(&conn), 3);
    assert_eq!(app.state().selected_wad_title().unwrap(), first);
}

#[test]
fn a_notification_can_be_dismissed() {
    let db_path = library("dismiss", 2);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    press(&mut app, egui::Key::Delete);
    app.get_by_label("Delete").click_accesskit();
    settle(&mut app);
    assert!(app.query_by_label("Undo").is_some());

    app.get_by_label("\u{00d7}").click_accesskit();
    settle(&mut app);

    assert!(app.query_by_label("Undo").is_none());
}

/// What the list is in, top to bottom.
fn rows_of_list(app: &Harness) -> Vec<String> {
    let mut rows: Vec<(f32, String)> = app
        .query_all_by_role(Role::Label)
        .filter_map(|node| {
            let text = node.value()?;
            text.starts_with("WAD ").then(|| (node.rect().min.y, text))
        })
        .filter(|(_, text)| text.len() == "WAD 000".len())
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.dedup_by(|a, b| a.1 == b.1);
    rows.into_iter().map(|(_, text)| text).collect()
}

#[test]
fn a_column_heading_sorts_the_list_and_turns_it_around() {
    let db_path = library("column-sort", 5);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    press(&mut app, egui::Key::I);
    app.get_by_label("List").click_accesskit();
    settle(&mut app);

    app.get_by_label("Title").click_accesskit();
    settle(&mut app);
    let ascending = rows_of_list(&app);
    assert_eq!(ascending.first().map(String::as_str), Some("WAD 000"));
    assert_eq!(ascending.last().map(String::as_str), Some("WAD 004"));

    app.get_by_label_contains("Title").click_accesskit();
    settle(&mut app);
    let descending = rows_of_list(&app);
    assert_eq!(descending.first().map(String::as_str), Some("WAD 004"));
}

#[test]
fn a_filter_that_finds_nothing_says_so_and_offers_the_way_back() {
    let db_path = library("no-match", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    type_text(&mut app, "/");
    type_text(&mut app, "nothing-has-this-name");
    std::thread::sleep(std::time::Duration::from_millis(200));
    settle(&mut app);
    assert!(app.query_by_label("No WADs match.").is_some());

    app.get_by_label("Show All WADs").click_accesskit();
    settle(&mut app);

    assert!(app.query_by_label("No WADs match.").is_none());
    assert_eq!(app.get_by_label("Filter WADs").value().as_deref(), Some(""));
}

#[test]
fn escape_backs_out_of_a_filter_before_it_touches_the_selection() {
    let db_path = library("escape-filter", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    type_text(&mut app, "/");
    type_text(&mut app, "WAD");
    std::thread::sleep(std::time::Duration::from_millis(200));
    settle(&mut app);

    // The first is the text box's, which lets go of the keyboard.
    press(&mut app, egui::Key::Escape);
    if app.get_by_label("Filter WADs").value().as_deref() == Some("WAD") {
        press(&mut app, egui::Key::Escape);
    }
    assert_eq!(app.get_by_label("Filter WADs").value().as_deref(), Some(""));
    assert!(app.state().selected_wad_title().is_some());
}

#[test]
fn a_search_worth_keeping_can_be_saved_as_a_collection() {
    let db_path = library("save-collection", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    type_text(&mut app, "/");
    type_text(&mut app, "status:unplayed");
    std::thread::sleep(std::time::Duration::from_millis(200));
    settle(&mut app);

    app.get_by_label("Save as Collection").click_accesskit();
    settle(&mut app);

    assert!(
        app.query_by_role_and_label(Role::Window, "Collections")
            .is_some()
    );
    assert_eq!(
        app.get_by_label("Query").value().as_deref(),
        Some("status:unplayed")
    );
}

#[test]
fn a_card_offers_play_once_the_pointer_is_on_it() {
    let db_path = library("card-play", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    let plays_before = app.query_all_by_label("Play").count();

    let card = app.get_by_label("WAD 001, Unplayed").rect().center();
    app.hover_at(egui::pos2(card.x, card.y));
    settle(&mut app);

    assert_eq!(app.query_all_by_label("Play").count(), plays_before + 1);
}

/// A file dropped on the window, as far as the app can tell.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// The form's button, which is not the sidebar's entry of the same name: that is drawn first.
fn press_import(app: &mut Harness) {
    let button = app
        .query_all_by_label("Import")
        .last()
        .expect("an Import button");
    button.click_accesskit();
    settle(app);
}

/// Until something a worker does has shown up, or the test has waited long enough to fail.
fn wait_until(app: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..200 {
        if done(app) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
        settle(app);
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn a_file_dropped_on_the_window_can_be_imported_and_shown() {
    let db_path = library("drop-file", 1);
    let file = scratch().join("my_new_map.wad");
    std::fs::write(&file, "PWAD").expect("write WAD file");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    app.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(file.clone())));
    settle(&mut app);

    // All it needed was a title, and it has one.
    let path = file.display().to_string();
    assert_eq!(
        app.get_by_label("File").value().as_deref(),
        Some(path.as_str())
    );
    assert_eq!(
        app.get_by_label("Title").value().as_deref(),
        Some("my new map")
    );

    press_import(&mut app);
    wait_until(&mut app, "the import", |app| {
        app.query_by_label("Imported my new map.").is_some()
    });

    app.get_by_label("Show").click_accesskit();
    settle(&mut app);
    assert_eq!(app.state().selected_wad_title(), Some("my new map"));
    assert!(app.query_by_label("Filter WADs").is_some());
}

/// Going to another tab used to leave the form saying "Importing..." for good.
#[test]
fn an_import_finishing_on_another_tab_still_finishes_where_it_started() {
    let db_path = library("import-tabs", 1);
    let file = scratch().join("elsewhere.wad");
    std::fs::write(&file, "PWAD").expect("write WAD file");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(file)));
    settle(&mut app);

    press_import(&mut app);
    app.get_by_label("idgames").click_accesskit();
    wait_until(&mut app, "the import", |app| {
        app.query_by_label("Imported elsewhere.").is_some()
    });

    app.get_by_label("Local File").click_accesskit();
    settle(&mut app);
    assert!(app.query_by_label("Importing\u{2026}").is_none());
    assert_eq!(app.get_by_label("File").value().as_deref(), Some(""));
}

#[test]
fn a_form_says_what_is_missing_and_says_it_on_enter_too() {
    let db_path = library("form-validate", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Import);
    settle(&mut app);
    app.get_by_label("URL").click_accesskit();
    settle(&mut app);

    press_import(&mut app);

    assert!(app.query_by_label("Download URL is required").is_some());
}

/// Finishing typing a filter used to launch whatever happened to be selected.
#[test]
fn enter_in_the_filter_finishes_typing_and_does_not_play_anything() {
    let db_path = library("enter-in-filter", 3);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    type_text(&mut app, "/");
    type_text(&mut app, "WAD");

    press(&mut app, egui::Key::Enter);
    std::thread::sleep(std::time::Duration::from_millis(100));
    settle(&mut app);

    assert!(
        app.query_by_role_and_label(Role::Window, "WAD Unavailable")
            .is_none()
    );

    // And it is the library's keyboard again: this one means it.
    press(&mut app, egui::Key::Enter);
    wait_until(&mut app, "the launch to find no file", |app| {
        app.query_by_role_and_label(Role::Window, "WAD Unavailable")
            .is_some()
    });
}

#[test]
fn enter_in_the_search_box_searches_and_does_not_import_what_is_selected() {
    let db_path = library("enter-in-search", 1);
    let conn = db::open_connection(&db_path).expect("open library");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Import);
    settle(&mut app);

    app.get_by_label("Search idgames").focus();
    settle(&mut app);
    type_text(&mut app, "anything");
    press(&mut app, egui::Key::Enter);

    wait_until(&mut app, "the search to finish", |app| {
        app.query_all_by_label("Searching...").next().is_none()
    });
    assert_eq!(wads_in_library(&conn), 1);
}

#[test]
fn a_year_can_be_gone_through_with_the_keys() {
    let db_path = library("cacoward-keys", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Cacowards);
    settle(&mut app);
    let crumbs = |app: &Harness| {
        app.query_all_by_role(Role::Label)
            .filter_map(|node| node.value())
            .find(|text| {
                text.parse::<i64>()
                    .is_ok_and(|year| (2000..2100).contains(&year))
            })
    };
    let newest = crumbs(&app).expect("the year in the top bar");

    press(&mut app, egui::Key::ArrowRight);
    let older = crumbs(&app).expect("the year in the top bar");
    assert!(older < newest, "{older} comes after {newest} on the strip");

    press(&mut app, egui::Key::ArrowLeft);
    assert_eq!(crumbs(&app).expect("the year in the top bar"), newest);
}

/// Nothing in this library is finished, so hiding what is has nothing to hide.
#[test]
fn hiding_what_is_finished_leaves_what_is_not() {
    let db_path = library("hide-finished", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Cacowards);
    settle(&mut app);
    let rows = |app: &Harness| app.query_all_by_label_contains("Not in Library").count();
    let before = rows(&app);
    assert!(before > 0);

    app.get_by_label("Hide Finished").click_accesskit();
    settle(&mut app);

    // What can't be played is out of the reckoning and goes, and nothing else does.
    assert!(rows(&app) <= before);
    assert!(rows(&app) > 0);
}

#[test]
fn a_number_in_stats_goes_to_what_it_counts() {
    let db_path = library("stats-goes", 3);
    let conn = db::open_connection(&db_path).expect("open library");
    conn.execute(
        "UPDATE wads SET status = 'completed' WHERE title = 'WAD 001'",
        [],
    )
    .expect("finish a WAD");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Stats);
    settle(&mut app);

    app.get_by_label("Completed: 1 WAD").click_accesskit();
    settle(&mut app);

    assert_eq!(app.state().selected_wad_title(), Some("WAD 001"));
    assert!(app.query_by_label("1 of 3 WADs").is_some());
}

#[test]
fn a_year_in_stats_goes_to_its_cacowards() {
    let db_path = library("stats-year", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    app.state_mut().set_view(ViewMode::Stats);
    settle(&mut app);

    app.get_by_label_contains("2011:").click_accesskit();
    settle(&mut app);

    assert!(app.query_by_label("Hide Finished").is_some());
}

/// Nothing used to say what a new library needed until Play failed for want of it.
#[test]
fn an_empty_library_says_what_there_is_to_do_first() {
    let db_path = library("first-run", 0);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    assert!(app.query_by_label("Your library is empty").is_some());
    assert!(app.query_by_label("No game files yet").is_some());

    app.get_by_label("See the Cacowards").click_accesskit();
    settle(&mut app);
    assert!(app.query_by_label("Hide Finished").is_some());
}

#[test]
fn the_main_button_of_a_dialog_is_on_a_key_too() {
    let db_path = library("ctrl-enter", 2);
    let conn = db::open_connection(&db_path).expect("open library");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);

    press(&mut app, egui::Key::D);
    app.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    settle(&mut app);

    assert_eq!(wads_in_library(&conn), 1);
    assert!(
        app.query_by_role_and_label(Role::Window, "Delete WAD")
            .is_none()
    );
}

#[test]
fn help_says_how_to_write_a_filter() {
    let db_path = library("help-filter", 1);
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    press(&mut app, egui::Key::F1);
    assert!(app.query_by_label("^status:abandoned").is_some());
}

/// Five in a row, wherever they are. They were in a column in Edit, once, and only looking at
/// a picture of it showed that.
#[test]
fn the_stars_for_a_rating_go_across_in_the_panel_and_in_edit() {
    let db_path = described_library("stars-across");
    let mut app = open_app(&db_path, DEFAULT_WINDOW);
    let in_a_row = |app: &Harness| {
        let first = app.get_all_by_label("Rate 1 star").last().unwrap().rect();
        let last = app.get_all_by_label("Rate 5 stars").last().unwrap().rect();
        assert_eq!(first.min.y, last.min.y, "{first:?} {last:?}");
        assert!(first.max.x <= last.min.x, "{first:?} {last:?}");
    };
    in_a_row(&app);

    press(&mut app, egui::Key::E);
    assert!(app.query_by_label("Save Changes").is_some());
    in_a_row(&app);
}

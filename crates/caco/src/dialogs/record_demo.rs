//! Record Demo: which map, and how hard, before a recording starts.
//!
//! A demo always starts a new game, and nyan-doom won't record one without being told the
//! skill, so this asks both. The maps are the ones caco read in the WAD (its analysis), in
//! order, each with the demos that start there and whether any got to the exit, and at what
//! skill, which is what someone working through a WAD a map at a time wants to see. Whether a
//! demo got to the exit comes from its record (`DemoFile::finished`): one caco didn't record,
//! or recorded before it noted that, can't say.

use caco_core::demos::{self, DemoFile, Finished, header::SKILLS};
use caco_core::player::RecordStart;
use caco_core::wad_analysis::{MapClassification, WadAnalysis, map_order};
use caco_core::wad_stats::WadStats;
use egui_extras::Column;
use rusqlite::Connection;

use crate::dialogs::{Dialog, Primary, count, dialog_table};
use crate::theme;

/// What a cell with nothing to say shows.
const NOTHING: &str = "\u{2014}";

pub struct RecordDemoDialogState {
    wad_id: i64,
    wad_title: String,
    /// The sourceport as the user knows it, for saying what it can't do.
    sourceport: String,
    rows: Vec<MapRow>,
    maps_from: MapsFrom,
    selected: usize,
    /// 1 to 5, as `-skill` takes it.
    skill: u8,
    /// Set by a key, so that the frame that draws the table scrolls the selection into view.
    reveal: bool,
}

#[derive(Debug, Clone)]
pub enum RecordDemoResult {
    Open,
    Record { wad_id: i64, start: RecordStart },
    Cancelled,
}

/// Where the list of maps came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapsFrom {
    /// The WAD, read by caco.
    Wad,
    /// Only what has been played, since caco hasn't read the WAD yet.
    Played,
    /// Nowhere, so the one choice is a new game's start.
    Nothing,
}

/// A map as the table shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MapRow {
    /// Its lump name, or `None` for wherever a new game starts.
    map: Option<String>,
    /// What kind of map it is, where it isn't one on the way to the end.
    kind: Option<&'static str>,
    /// Whether the sourceport can be sent there.
    startable: bool,
    /// The demos that start on it.
    demos: usize,
    /// The skills of those that got to its exit, numbered from 0 as a demo numbers them,
    /// hardest first.
    finished: Vec<u8>,
    /// How many got to its exit.
    reached: usize,
    /// How many are known not to have.
    unfinished: usize,
}

impl MapRow {
    fn named(map: Option<String>, kind: Option<&'static str>, sourceport: &str) -> Self {
        let startable = caco_core::sourceports::get_start_args(
            sourceport,
            map.as_deref(),
            RecordStart::DEFAULT_SKILL,
        )
        .is_some();
        Self {
            map,
            kind,
            startable,
            demos: 0,
            finished: Vec::new(),
            reached: 0,
            unfinished: 0,
        }
    }

    /// How many of its demos nothing says the outcome of.
    fn unknown(&self) -> usize {
        self.demos - self.reached - self.unfinished
    }

    fn label(&self) -> &str {
        self.map.as_deref().unwrap_or("From the start")
    }

    /// What the Finished column says, and whether that is good news.
    fn finished_text(&self) -> (String, bool) {
        if !self.finished.is_empty() {
            let skills: Vec<&str> = self
                .finished
                .iter()
                .filter_map(|&skill| SKILLS.get(usize::from(skill)).map(|(short, _)| *short))
                .collect();
            return (skills.join(", "), true);
        }
        let text = match (self.demos, self.unfinished) {
            (0, _) => "",
            (_, 0) => "Can't tell",
            _ => "Not yet",
        };
        (text.to_string(), false)
    }
}

impl RecordDemoDialogState {
    pub fn new(conn: &Connection, wad_id: i64) -> Option<Self> {
        let wad = caco_core::db::get_wad(conn, wad_id, false).ok()??;
        let sourceport = caco_core::player::sourceport_for(&wad);
        let analysis = caco_core::db::get_analysis(conn, wad_id).ok().flatten();
        let stats = wad
            .stats_snapshot
            .as_deref()
            .and_then(|s| caco_core::wad_stats::stats_from_json(s).ok());
        let demos = caco_core::config::find_wad_data_dir(&wad.data_key, &wad.title)
            .map(|dir| demos::find_demo_files(&dir))
            .unwrap_or_default();
        Some(Self::from_parts(
            wad_id,
            wad.title,
            &sourceport,
            analysis.as_ref(),
            stats.as_ref(),
            &demos,
        ))
    }

    fn from_parts(
        wad_id: i64,
        wad_title: String,
        sourceport: &str,
        analysis: Option<&WadAnalysis>,
        stats: Option<&WadStats>,
        demos: &[DemoFile],
    ) -> Self {
        let (rows, maps_from) = map_rows(analysis, stats, demos, sourceport);
        // The next map with nothing to show for it, for someone going through them in turn.
        let selected = rows
            .iter()
            .position(|r| r.startable && r.finished.is_empty())
            .or_else(|| rows.iter().position(|r| r.startable))
            .unwrap_or(0);
        Self {
            wad_id,
            wad_title,
            sourceport: std::path::Path::new(sourceport).file_stem().map_or_else(
                || sourceport.to_string(),
                |s| s.to_string_lossy().into_owned(),
            ),
            rows,
            maps_from,
            selected,
            skill: last_skill(demos).unwrap_or(RecordStart::DEFAULT_SKILL),
            reveal: true,
        }
    }

    pub fn render(&mut self, ctx: &egui::Context) -> RecordDemoResult {
        let mut result = RecordDemoResult::Open;
        let mut record = self.handle_keys(ctx);

        let selected = self.rows.get(self.selected).cloned();
        let skill = SKILLS[usize::from(self.skill - 1)].0;
        let label = match selected.as_ref().and_then(|row| row.map.as_deref()) {
            Some(map) => format!("Record {map} at {skill}"),
            None => format!("Record at {skill}"),
        };
        let why = format!(
            "{} can only be sent to maps with Doom's own names, such as MAP07 or E1M7.",
            self.sourceport
        );
        let note = (self.maps_from == MapsFrom::Played).then_some(
            "caco hasn't read this WAD's maps yet, so these are the ones you've played.",
        );

        Dialog::new("Record Demo", [560.0, 460.0])
            .about(&self.wad_title)
            .show(ctx, |dialog| {
                let body = dialog.pinned(|ui| self.render_skills(ui));
                // With no maps to pick from, a sentence, in a window its height.
                let footer = if self.maps_from == MapsFrom::Nothing {
                    body.scroll(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(
                                    "caco hasn't read this WAD's maps yet, so the demo starts \
                                     wherever a new game does.",
                                )
                                .color(theme::TEXT_SECONDARY),
                            )
                            .wrap(),
                        );
                    })
                } else {
                    body.fill(|ui, height| {
                        if self.render_maps(ui, height) {
                            record = true;
                        }
                    })
                };
                let footer = match note {
                    Some(note) => footer.above(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(note).color(theme::TEXT_SECONDARY),
                            )
                            .wrap(),
                        );
                    }),
                    None => footer,
                };
                footer.buttons(|row| {
                    let ready = selected.as_ref().is_some_and(|r| r.startable);
                    let ended =
                        row.end(Some("Cancel"), Some(Primary::new(&label).when(ready, &why)));
                    if ended.dismissed {
                        result = RecordDemoResult::Cancelled;
                    }
                    if ended.primary {
                        record = true;
                    }
                })
            });

        if record && let Some(row) = selected.filter(|r| r.startable) {
            return RecordDemoResult::Record {
                wad_id: self.wad_id,
                start: RecordStart {
                    map: row.map,
                    skill: self.skill,
                },
            };
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return RecordDemoResult::Cancelled;
        }
        result
    }

    /// Up and Down go through the maps, Left and Right through the skills, and Enter records.
    /// All of them taken, since the dialog is what they were pressed for.
    fn handle_keys(&mut self, ctx: &egui::Context) -> bool {
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        let last = self.rows.len().saturating_sub(1);
        if pressed(egui::Key::ArrowDown) {
            self.selected = (self.selected + 1).min(last);
            self.reveal = true;
        }
        if pressed(egui::Key::ArrowUp) {
            self.selected = self.selected.saturating_sub(1);
            self.reveal = true;
        }
        if pressed(egui::Key::ArrowRight) {
            self.skill = (self.skill + 1).min(SKILLS.len() as u8);
        }
        if pressed(egui::Key::ArrowLeft) {
            self.skill = (self.skill - 1).max(1);
        }
        pressed(egui::Key::Enter)
    }

    fn render_skills(&mut self, ui: &mut egui::Ui) {
        // As tall as a button from the start, or the label is centred on a shorter row than
        // the buttons that follow it.
        let height =
            ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
        let size = egui::vec2(ui.available_width(), height);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.label("Skill");
            for (n, (short, full)) in (1u8..).zip(SKILLS) {
                ui.selectable_value(&mut self.skill, n, short)
                    .on_hover_text(full);
            }
        });
    }

    /// The maps, one a row. True if one was double-clicked, which records it.
    fn render_maps(&mut self, ui: &mut egui::Ui, height: f32) -> bool {
        const HEADERS: [&str; 3] = ["Map", "Demos", "Finished"];
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let mut clicked = None;
        let mut double_clicked = false;
        let reveal = std::mem::take(&mut self.reveal).then_some(self.selected);
        let selected = self.selected;
        let why = format!(
            "{} can only be sent to maps with Doom's own names.",
            self.sourceport
        );

        let mut table = dialog_table(ui, height - row_height)
            .sense(egui::Sense::click())
            .column(Column::remainder().at_least(160.0).clip(true))
            .column(Column::initial(70.0).at_least(56.0))
            .column(Column::initial(150.0).at_least(90.0).clip(true));
        if let Some(row) = reveal {
            table = table.scroll_to_row(row, None);
        }
        table
            .header(row_height + 2.0, |mut header| {
                for label in HEADERS {
                    header.col(|ui| {
                        ui.label(theme::column_header(label));
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, self.rows.len(), |mut row| {
                    let idx = row.index();
                    let map = &self.rows[idx];
                    row.set_selected(selected == idx);
                    row.col(|ui| {
                        let colour = if map.startable {
                            theme::TEXT_PRIMARY
                        } else {
                            theme::TEXT_MUTED
                        };
                        ui.colored_label(colour, map.label());
                        if let Some(kind) = map.kind {
                            ui.colored_label(
                                theme::TEXT_SECONDARY,
                                egui::RichText::new(kind).small(),
                            );
                        }
                    });
                    row.col(|ui| {
                        if map.demos == 0 {
                            ui.colored_label(theme::TEXT_SECONDARY, NOTHING);
                        } else {
                            ui.label(map.demos.to_string());
                        }
                    });
                    row.col(|ui| {
                        let (text, good) = map.finished_text();
                        let colour = if good {
                            theme::COLOR_SUCCESS
                        } else {
                            theme::TEXT_SECONDARY
                        };
                        ui.colored_label(colour, text);
                    });
                    let mut response = row.response();
                    if !map.startable {
                        response = response.on_hover_text(&why);
                    } else if map.demos > 0 {
                        response = response.on_hover_text(demos_hint(map));
                    }
                    if response.clicked() {
                        clicked = Some(idx);
                    }
                    if response.double_clicked() {
                        clicked = Some(idx);
                        double_clicked = true;
                    }
                });
            });

        if let Some(idx) = clicked {
            self.selected = idx;
        }
        double_clicked
    }
}

/// What a row's tooltip says about its demos: how many got to the exit, how many didn't, and
/// how many caco can't tell about, because it didn't record them or recorded them before it
/// noted that.
fn demos_hint(row: &MapRow) -> String {
    let mut parts = Vec::new();
    if row.reached > 0 {
        parts.push(format!("{} got to the exit", row.reached));
    }
    if row.unfinished > 0 {
        parts.push(format!("{} didn't", row.unfinished));
    }
    if row.unknown() > 0 {
        parts.push(format!("{} can't say", row.unknown()));
    }
    format!(
        "{}: {}.",
        count(row.demos, "demo", "demos"),
        parts.join(", ")
    )
}

/// The maps to offer, and where they came from: the WAD's, bar those nothing leads to, in
/// order; or, until caco has read it, those played; or, with neither, a new game's start.
fn map_rows(
    analysis: Option<&WadAnalysis>,
    stats: Option<&WadStats>,
    demos: &[DemoFile],
    sourceport: &str,
) -> (Vec<MapRow>, MapsFrom) {
    let mut maps: Vec<(String, Option<&'static str>)> = match analysis {
        Some(analysis) => analysis
            .maps
            .iter()
            .filter_map(|m| {
                let kind = match m.classification {
                    MapClassification::Required => None,
                    MapClassification::OptionalSecret => Some("Secret"),
                    MapClassification::OptionalBonus => Some("Bonus"),
                    MapClassification::OptionalCredits => Some("Credits"),
                    MapClassification::Unreachable => return None,
                };
                Some((m.lump.clone(), kind))
            })
            .collect(),
        None => stats
            .map(|s| {
                s.played_maps()
                    .iter()
                    .map(|m| (m.lump.clone(), None))
                    .collect()
            })
            .unwrap_or_default(),
    };
    maps.sort_by(|a, b| map_order(&a.0, &b.0));
    let maps_from = match (analysis, maps.is_empty()) {
        (_, true) => MapsFrom::Nothing,
        (Some(_), false) => MapsFrom::Wad,
        (None, false) => MapsFrom::Played,
    };

    let mut rows: Vec<MapRow> = maps
        .into_iter()
        .map(|(map, kind)| MapRow::named(Some(map), kind, sourceport))
        .collect();
    if rows.is_empty() {
        rows.push(MapRow::named(None, None, sourceport));
    }

    for demo in demos {
        let Some(header) = &demo.header else { continue };
        let Some(start) = &header.start else { continue };
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.map.as_deref().is_some_and(|map| start.is(map)))
        else {
            continue;
        };
        row.demos += 1;
        match demo.finished() {
            Finished::Yes => {
                row.reached += 1;
                if let Some(skill) = header.skill
                    && !row.finished.contains(&skill)
                {
                    row.finished.push(skill);
                }
            }
            Finished::No => row.unfinished += 1,
            Finished::Unknown => {}
        }
    }
    for row in &mut rows {
        row.finished.sort_unstable_by(|a, b| b.cmp(a));
    }
    (rows, maps_from)
}

/// The skill of the demo caco recorded last, as `-skill` numbers it, since the next is likely
/// to be the same.
fn last_skill(demos: &[DemoFile]) -> Option<u8> {
    demos
        .iter()
        .filter_map(|d| Some((d.recorded_at()?, d.header.as_ref()?.skill?)))
        .max_by(|a, b| a.0.cmp(b.0))
        .map(|(_, skill)| skill + 1)
        .filter(|skill| (1..=SKILLS.len() as u8).contains(skill))
}

#[cfg(test)]
mod tests {
    use super::*;
    use caco_core::demos::DemoHeader;
    use caco_core::demos::header::{DemoFormat, StartMap};
    use caco_core::wad_analysis::{ANALYSIS_VERSION, MapInfo};

    fn analysis_of(maps: &[(&str, MapClassification)]) -> WadAnalysis {
        WadAnalysis {
            version: ANALYSIS_VERSION,
            maps: maps
                .iter()
                .map(|(lump, classification)| MapInfo {
                    lump: lump.to_string(),
                    classification: *classification,
                })
                .collect(),
            total_maps: maps.len(),
            required_maps: 0,
            secret_maps: Vec::new(),
            terminal_map: None,
            has_umapinfo: true,
        }
    }

    /// A demo of MAP`map` at `skill`, from 0, which caco recorded with `exited` noted, or
    /// didn't record at all.
    fn demo(map: u8, skill: u8, exited: Option<Option<&[&str]>>, at: &str) -> DemoFile {
        let record = exited.map(|exited| {
            serde_json::from_value(serde_json::json!({
                "version": 1,
                "recorded_at": at,
                "sourceport": "nyan-doom",
                "profile": "default",
                "exited": exited,
            }))
            .expect("record")
        });
        DemoFile {
            path: format!("/d/demos/{map}-{at}.lmp").into(),
            name: format!("{map}-{at}.lmp"),
            header: Some(DemoHeader {
                format: DemoFormat::Vanilla(109),
                skill: Some(skill),
                start: Some(StartMap::Numbers { episode: 1, map }),
                players: 1,
                tics: Some(35),
                footer: None,
            }),
            record,
        }
    }

    fn labels(rows: &[MapRow]) -> Vec<&str> {
        rows.iter().map(MapRow::label).collect()
    }

    #[test]
    fn the_maps_are_the_wads_in_order_with_what_their_demos_did() {
        use MapClassification::*;
        let analysis = analysis_of(&[
            ("MAP02", Required),
            ("MAP31", OptionalSecret),
            ("MAP01", Required),
            ("TESTMAP", Unreachable),
        ]);
        let demos = [
            demo(1, 3, Some(Some(&["MAP01"])), "2026-11-18T09:00:00Z"),
            demo(
                1,
                4,
                Some(Some(&["MAP01", "MAP02"])),
                "2026-11-18T10:00:00Z",
            ),
            demo(1, 3, Some(Some(&[])), "2026-11-18T11:00:00Z"),
            demo(2, 3, Some(Some(&[])), "2026-11-18T12:00:00Z"),
            demo(2, 3, None, "2026-11-18T13:00:00Z"),
            demo(31, 2, Some(None), "2026-11-18T14:00:00Z"),
        ];
        let (rows, from) = map_rows(Some(&analysis), None, &demos, "nyan-doom");
        assert_eq!(from, MapsFrom::Wad);
        assert_eq!(labels(&rows), ["MAP01", "MAP02", "MAP31"]);
        assert_eq!(rows[2].kind, Some("Secret"));

        // Two got to the exit, hardest first, and one didn't.
        assert_eq!(
            (rows[0].demos, rows[0].reached, rows[0].unfinished),
            (3, 2, 1)
        );
        assert_eq!(rows[0].finished_text(), ("NM, UV".to_string(), true));
        // A continuous run from MAP01 is not a demo of MAP02.
        assert_eq!(rows[1].demos, 2);
        assert_eq!(rows[1].finished_text(), ("Not yet".to_string(), false));
        assert_eq!(demos_hint(&rows[1]), "2 demos: 1 didn't, 1 can't say.");
        assert_eq!(rows[2].finished_text(), ("Can't tell".to_string(), false));

        // The next map with nothing to show for it comes up first.
        let dialog = RecordDemoDialogState::from_parts(
            1,
            "Scythe".to_string(),
            "/opt/nyan-doom",
            Some(&analysis),
            None,
            &demos,
        );
        assert_eq!(dialog.selected, 1);
        assert_eq!(dialog.sourceport, "nyan-doom");
        // At the skill of the last demo caco recorded: HMP is 3 to `-skill`.
        assert_eq!(dialog.skill, 3);
    }

    #[test]
    fn a_map_the_sourceport_cant_be_sent_to_is_shown_and_cant_be_picked() {
        let analysis = analysis_of(&[
            ("BTW01", MapClassification::Required),
            ("BTW02", MapClassification::Required),
        ]);
        let (rows, _) = map_rows(Some(&analysis), None, &[], "nyan-doom");
        assert!(rows.iter().all(|row| !row.startable));
        let (rows, _) = map_rows(Some(&analysis), None, &[], "uzdoom");
        assert!(rows.iter().all(|row| row.startable));

        let dialog = RecordDemoDialogState::from_parts(
            1,
            "The Warrens".to_string(),
            "nyan-doom",
            Some(&analysis),
            None,
            &[],
        );
        assert_eq!(dialog.selected, 0);
        assert_eq!(dialog.skill, RecordStart::DEFAULT_SKILL);
    }

    #[test]
    fn without_the_wads_maps_it_goes_by_what_was_played_or_by_the_start() {
        let stats: WadStats = serde_json::from_value(serde_json::json!({
            "format": "stats_txt",
            "maps": [
                { "lump": "MAP03", "best_skill": 4, "total_exits": 1 },
                { "lump": "MAP01", "best_skill": 4, "total_exits": 2 },
            ],
        }))
        .expect("stats");
        let (rows, from) = map_rows(None, Some(&stats), &[], "nyan-doom");
        assert_eq!(from, MapsFrom::Played);
        assert_eq!(labels(&rows), ["MAP01", "MAP03"]);

        let (rows, from) = map_rows(None, None, &[], "nyan-doom");
        assert_eq!(from, MapsFrom::Nothing);
        assert_eq!(labels(&rows), ["From the start"]);
        assert!(rows[0].startable);
        assert_eq!(rows[0].map, None);
    }
}

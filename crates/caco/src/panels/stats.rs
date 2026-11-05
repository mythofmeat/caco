//! Library statistics, for someone who collects these: how the library is doing, when it got
//! played and finished, how far through the Cacowards it is, and what is in it.
//!
//! Each number has one meaning, and the same one as everywhere else: "Completed" is the
//! number on the library's filter, and a year's progress is the one on its Cacowards chip.
//! This used to show five figures about completion on one screen, each defined differently and
//! none explained, and leave half the page empty.
//!
//! A view rather than a dialog: nothing in it is an action, and you go there to read.
//!
//! Charts are painted by hand rather than through a plotting crate, since two bar charts and
//! some thin bars are all there are. Every mark's value is in its tooltip and its accessible
//! name, so that none of it can only be got at by pointing at it.

use caco_core::db::Status;
use caco_core::db::sessions::{StatsSnapshot, Tally};

use crate::state::AppState;
use crate::theme;

/// Width below which the two-column body stacks instead.
const NARROW: f32 = 720.0;

const COLUMN_GAP: f32 = 24.0;
const SECTION_GAP: f32 = 18.0;

/// A chart's height, axis labels included.
const CHART_HEIGHT: f32 = 132.0;
const AXIS_GUTTER: f32 = 34.0;
const AXIS_BAND: f32 = 16.0;
/// Bars are no thicker than this, however few of them there are.
const MAX_BAR_WIDTH: f32 = 24.0;
const BAR_GAP: f32 = 2.0;
/// How wide a month's label needs to be not to touch the next.
const MONTH_LABEL_WIDTH: f32 = 30.0;
/// How far back the charts go.
const MAX_MONTHS: usize = 24;

const BAR_ROW_HEIGHT: f32 = 18.0;
const BAR_HEIGHT: f32 = 10.0;

pub fn render(ui: &mut egui::Ui, state: &AppState) {
    let Some(snap) = &state.stats.snapshot else {
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.colored_label(theme::TEXT_SECONDARY, "No statistics available.");
        });
        return;
    };

    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(24, 16))
        .show(ui, |ui| {
            headline_tiles(ui, snap, state);
            ui.add_space(SECTION_GAP);

            if ui.available_width() < NARROW {
                playtime_chart(ui, snap);
                ui.add_space(SECTION_GAP);
                completions_chart(ui, snap);
                ui.add_space(SECTION_GAP);
                by_status(ui, snap);
                ui.add_space(SECTION_GAP);
                cacowards(ui, state);
            } else {
                // Halve the row, less the gap, so neither column claims the
                // slack — an unbalanced split reads as a bug at wide sizes.
                let column = (ui.available_width() - COLUMN_GAP) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(column);
                        playtime_chart(ui, snap);
                        ui.add_space(SECTION_GAP);
                        completions_chart(ui, snap);
                    });
                    ui.add_space(COLUMN_GAP - ui.spacing().item_spacing.x);
                    ui.vertical(|ui| {
                        ui.set_width(column);
                        by_status(ui, snap);
                        ui.add_space(SECTION_GAP);
                        cacowards(ui, state);
                    });
                });
            }

            ui.add_space(SECTION_GAP);
            breakdowns(ui, snap);
        });
}

/// The four numbers worth seeing without reading: a tile row, each with what it means.
fn headline_tiles(ui: &mut egui::Ui, snap: &StatsSnapshot, state: &AppState) {
    let completed = snap.with_status(Status::Completed);
    let share = if snap.total_wads > 0 {
        format!(
            "{:.0}% of your library",
            completed as f64 / snap.total_wads as f64 * 100.0
        )
    } else {
        "of nothing, so far".to_string()
    };
    let (award_total, award_done) = cacoward_totals(state);

    let tiles = [
        (
            "Playtime",
            caco_core::player::format_duration(snap.total_playtime),
            count(snap.total_sessions, "session", "sessions"),
        ),
        (
            "WADs",
            snap.total_wads.to_string(),
            format!("{} in progress", snap.with_status(Status::InProgress)),
        ),
        ("Completed", completed.to_string(), share),
        (
            "Cacowards completed",
            award_done.to_string(),
            format!("of {award_total}, over every year"),
        ),
    ];

    let gap = ui.spacing().item_spacing.x;
    let width = ((ui.available_width() - gap * 3.0) / 4.0).max(110.0);
    ui.horizontal_wrapped(|ui| {
        for (label, value, caption) in tiles {
            tile(ui, width, label, &value, &caption);
        }
    });
}

fn tile(ui: &mut egui::Ui, width: f32, label: &str, value: &str, caption: &str) {
    egui::Frame::new()
        .fill(theme::BG_MEDIUM)
        .corner_radius(8)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            // What the margin and the stroke take is outside this.
            ui.set_width(width - 26.0);
            // A column, whatever the row around the tiles is.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.colored_label(theme::TEXT_SECONDARY, egui::RichText::new(label).size(11.0));
                ui.colored_label(
                    theme::TEXT_PRIMARY,
                    egui::RichText::new(value).size(20.0).strong(),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(caption)
                            .size(11.0)
                            .color(theme::TEXT_MUTED),
                    )
                    .truncate(),
                );
            });
        });
}

fn section_header(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(
            theme::TEXT_PRIMARY,
            egui::RichText::new(title).size(13.0).strong(),
        );
        if !detail.is_empty() {
            ui.colored_label(theme::TEXT_MUTED, egui::RichText::new(detail).size(11.0));
        }
    });
    ui.add_space(4.0);
}

fn count(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---------------------------------------------------------------------------
// Months
// ---------------------------------------------------------------------------

/// A year and a month, in an order that compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Month {
    year: i32,
    month: u32,
}

impl Month {
    /// From `2026-03`.
    fn parse(period: &str) -> Option<Self> {
        let (year, month) = period.split_once('-')?;
        Some(Self {
            year: year.parse().ok()?,
            month: month
                .parse()
                .ok()
                .filter(|month| (1..=12).contains(month))?,
        })
    }

    fn next(self) -> Self {
        if self.month == 12 {
            Self {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Self {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    fn date(self) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::from_ymd_opt(self.year, self.month, 1)
    }

    /// "March 2026"
    fn long(self) -> String {
        self.date()
            .map_or_else(String::new, |date| date.format("%B %Y").to_string())
    }

    /// "Mar"
    fn short(self) -> String {
        self.date()
            .map_or_else(String::new, |date| date.format("%b").to_string())
    }
}

/// What goes under each bar. Every `stride`th one has a label, and the first of those in each
/// year says which year, so that the axis says when whichever months it ends up labelling.
fn axis_labels(months: &[Month], stride: usize) -> Vec<Option<String>> {
    let mut last_year = None;
    months
        .iter()
        .enumerate()
        .map(|(index, month)| {
            if !index.is_multiple_of(stride) {
                return None;
            }
            let new_year = last_year != Some(month.year);
            last_year = Some(month.year);
            Some(if new_year {
                month.year.to_string()
            } else {
                month.short()
            })
        })
        .collect()
}

/// Every month from the first to the last of these, gaps included, so that a chart shows the
/// time that nothing happened as well, and at most the last `MAX_MONTHS`, so that an old
/// library does not squeeze them to nothing. It ends with the last month anything happened in
/// rather than with today, which would make what this shows depend on the date.
fn months_covering(periods: impl Iterator<Item = Month>) -> Vec<Month> {
    let mut first = None::<Month>;
    let mut last = None::<Month>;
    for month in periods {
        first = Some(first.map_or(month, |first| first.min(month)));
        last = Some(last.map_or(month, |last| last.max(month)));
    }
    let (Some(first), Some(last)) = (first, last) else {
        return Vec::new();
    };
    let mut months = Vec::new();
    let mut month = first;
    while month <= last {
        months.push(month);
        month = month.next();
    }
    let skip = months.len().saturating_sub(MAX_MONTHS);
    months.split_off(skip)
}

/// The months both charts cover, so that they line up one over the other.
fn chart_months(snap: &StatsSnapshot) -> Vec<Month> {
    months_covering(
        snap.activity
            .iter()
            .map(|period| period.period.as_str())
            .chain(
                snap.completions_by_month
                    .iter()
                    .map(|(period, _)| period.as_str()),
            )
            .filter_map(Month::parse),
    )
}

// ---------------------------------------------------------------------------
// Charts
// ---------------------------------------------------------------------------

struct Bar {
    month: Month,
    value: f64,
    /// What it stands for, in words.
    says: String,
}

fn playtime_chart(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    section_header(ui, "Playtime by month", "");
    let bars: Vec<Bar> = chart_months(snap)
        .into_iter()
        .map(|month| {
            let period = snap
                .activity
                .iter()
                .find(|period| Month::parse(&period.period) == Some(month));
            Bar {
                month,
                value: period.map_or(0.0, |period| period.total_playtime as f64 / 3600.0),
                says: match period {
                    Some(period) => format!(
                        "{}: {}, in {} of {}",
                        month.long(),
                        caco_core::player::format_duration(period.total_playtime),
                        count(period.session_count, "session", "sessions"),
                        count(period.wad_count, "WAD", "WADs"),
                    ),
                    None => format!("{}: nothing played", month.long()),
                },
            }
        })
        .collect();
    bar_chart(
        ui,
        "playtime",
        &bars,
        theme::CHART_BAR,
        |hours| format!("{hours:.0}h"),
        "Nothing played yet.",
    );
}

fn completions_chart(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    let detail = if snap.total_completions > 0 {
        format!(
            "{} in all, counting playing one again",
            snap.total_completions
        )
    } else {
        String::new()
    };
    section_header(ui, "Finished by month", &detail);
    let bars: Vec<Bar> = chart_months(snap)
        .into_iter()
        .map(|month| {
            let finished = snap
                .completions_by_month
                .iter()
                .find(|(period, _)| Month::parse(period) == Some(month))
                .map_or(0, |(_, finished)| *finished);
            Bar {
                month,
                value: finished as f64,
                says: format!("{}: {}", month.long(), times_finished(finished)),
            }
        })
        .collect();
    bar_chart(
        ui,
        "finished",
        &bars,
        theme::status_color(Status::Completed),
        |n| format!("{n:.0}"),
        "Nothing finished yet.",
    );
}

fn times_finished(n: i64) -> String {
    match n {
        0 => "nothing finished".to_string(),
        1 => "finished once".to_string(),
        n => format!("finished {n} times"),
    }
}

/// The least tidy number at or above `value`: 1, 2 or 5 times a power of ten. Where an axis ends.
fn nice_ceiling(value: f64) -> f64 {
    if value <= 0.0 {
        return 1.0;
    }
    let magnitude = 10f64.powf(value.log10().floor());
    let scaled = value / magnitude;
    let tidy = if scaled <= 1.0 {
        1.0
    } else if scaled <= 2.0 {
        2.0
    } else if scaled <= 5.0 {
        5.0
    } else {
        10.0
    };
    tidy * magnitude
}

/// Where an axis has its lines: the bottom, the top, and halfway if that is a whole number,
/// since half a completion is not a thing.
fn axis_ticks(top: f64) -> Vec<f64> {
    let middle = top / 2.0;
    if middle.fract() == 0.0 {
        vec![0.0, middle, top]
    } else {
        vec![0.0, top]
    }
}

/// How many bars each label covers, so that they do not run together.
fn label_stride(slot: f32) -> usize {
    (MONTH_LABEL_WIDTH / slot.max(1.0)).ceil().max(1.0) as usize
}

/// One series of columns on one axis. The bars are thin, rounded at the end that says how much
/// and square at the baseline. All of the value of each is in its tooltip and accessible name,
/// and it lifts when pointed at.
fn bar_chart(
    ui: &mut egui::Ui,
    id: &str,
    bars: &[Bar],
    color: egui::Color32,
    tick_label: impl Fn(f64) -> String,
    nothing_yet: &str,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CHART_HEIGHT),
        egui::Sense::hover(),
    );
    if bars.iter().all(|bar| bar.value <= 0.0) {
        ui.painter().text(
            rect.left_top(),
            egui::Align2::LEFT_TOP,
            nothing_yet,
            egui::FontId::proportional(12.0),
            theme::TEXT_SECONDARY,
        );
        return;
    }

    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(AXIS_GUTTER, 2.0),
        rect.max - egui::vec2(0.0, AXIS_BAND),
    );
    let max = bars.iter().map(|bar| bar.value).fold(0.0, f64::max);
    let top = nice_ceiling(max);
    let painter = ui.painter();
    let small = egui::FontId::proportional(10.0);

    for tick in axis_ticks(top) {
        let y = plot.max.y - plot.height() * (tick / top) as f32;
        painter.hline(plot.x_range(), y, egui::Stroke::new(1.0_f32, theme::BORDER));
        painter.text(
            egui::pos2(plot.min.x - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            tick_label(tick),
            small.clone(),
            theme::TEXT_MUTED,
        );
    }

    let slot = plot.width() / bars.len() as f32;
    let bar_width = (slot - BAR_GAP).clamp(1.0, MAX_BAR_WIDTH);
    let months: Vec<Month> = bars.iter().map(|bar| bar.month).collect();
    let labels = axis_labels(&months, label_stride(slot));

    for (index, bar) in bars.iter().enumerate() {
        let slot_rect = egui::Rect::from_min_max(
            egui::pos2(plot.min.x + slot * index as f32, plot.min.y),
            egui::pos2(plot.min.x + slot * (index + 1) as f32, plot.max.y),
        );
        // The whole slot is the target, since a short bar is a small thing to hit.
        let response = ui
            .interact(
                slot_rect,
                ui.scope_id().with(id).with(index),
                egui::Sense::hover(),
            )
            .on_hover_text(&bar.says);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &bar.says));

        if bar.value > 0.0 {
            // At least enough to be seen, however little it stands for.
            let height = (plot.height() * (bar.value / top) as f32).max(2.0);
            let mark = egui::Rect::from_center_size(
                egui::pos2(slot_rect.center().x, plot.max.y - height / 2.0),
                egui::vec2(bar_width, height),
            );
            let fill = if response.hovered() {
                theme::lift(color)
            } else {
                color
            };
            ui.painter().rect_filled(
                mark,
                egui::CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                fill,
            );
        }

        if let Some(label) = &labels[index] {
            ui.painter().text(
                egui::pos2(slot_rect.center().x, plot.max.y + 3.0),
                egui::Align2::CENTER_TOP,
                label,
                small.clone(),
                theme::TEXT_MUTED,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Rows of thin bars
// ---------------------------------------------------------------------------

/// A label, a thin bar and a figure on a line, with the columns lined up from row to row.
fn bar_row(
    ui: &mut egui::Ui,
    label: &str,
    label_width: f32,
    fraction: f32,
    figure: &str,
    color: egui::Color32,
    says: &str,
) {
    /// What the figure is given at the right, since it is a number and a short one.
    const FIGURE_WIDTH: f32 = 72.0;

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), BAR_ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let response = response.on_hover_text(says);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, says));

    let painter = ui.painter_at(rect);
    let y = rect.center().y;
    let font = egui::FontId::proportional(12.0);
    theme::paint_truncated(
        &painter,
        egui::pos2(rect.min.x, y - 8.0),
        label,
        font.clone(),
        theme::TEXT_SECONDARY,
        label_width - 6.0,
    );
    painter.text(
        egui::pos2(rect.max.x, y),
        egui::Align2::RIGHT_CENTER,
        figure,
        font,
        theme::TEXT_PRIMARY,
    );

    let track = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + label_width, y - BAR_HEIGHT / 2.0),
        egui::pos2(rect.max.x - FIGURE_WIDTH, y + BAR_HEIGHT / 2.0),
    );
    if track.width() <= 0.0 {
        return;
    }
    painter.rect_filled(track, 2.0, theme::BG_MEDIUM);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let fill = egui::Rect::from_min_size(
            track.min,
            egui::vec2((track.width() * fraction).max(2.0), track.height()),
        );
        let fill_color = if response.hovered() {
            theme::lift(color)
        } else {
            color
        };
        painter.rect_filled(fill, 2.0, fill_color);
    }
}

/// How the library is split. Each status in its own colour, and its figure beside it.
fn by_status(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    section_header(ui, "Your library", "");
    ui.spacing_mut().item_spacing.y = 0.0;
    let total = snap.total_wads.max(1) as f32;
    for &status in Status::ALL {
        let n = snap.with_status(status);
        let style = theme::status_style(status);
        let share = n as f32 / total;
        bar_row(
            ui,
            style.word,
            88.0,
            share,
            &format!("{n}  {:.0}%", share * 100.0),
            style.color,
            &format!("{}: {}", style.word, count(n, "WAD", "WADs")),
        );
    }
    ui.spacing_mut().item_spacing.y = 4.0;
}

/// (entries, completed), over every year, by the reckoning each year's own chip goes by.
fn cacoward_totals(state: &AppState) -> (usize, usize) {
    state
        .cacowards
        .years()
        .into_iter()
        .map(|year| state.cacowards.year_summary(year))
        .fold((0, 0), |(total, done), (year_total, year_done)| {
            (total + year_total, done + year_done)
        })
}

/// How far through each year's awards the library is.
fn cacowards(ui: &mut egui::Ui, state: &AppState) {
    let years = state.cacowards.years();
    if years.is_empty() {
        return;
    }
    section_header(ui, "Cacowards by year", "");
    let column = (ui.available_width() - COLUMN_GAP) / 2.0;
    let per_column = years.len().div_ceil(2);
    let color = theme::status_color(Status::Completed);
    ui.horizontal_top(|ui| {
        for (index, chunk) in years.chunks(per_column).enumerate() {
            // A gap between them and not after them, which would widen the page.
            if index > 0 {
                ui.add_space(COLUMN_GAP - ui.spacing().item_spacing.x);
            }
            ui.vertical(|ui| {
                ui.set_width(column);
                ui.spacing_mut().item_spacing.y = 0.0;
                for year in chunk {
                    let (total, done) = state.cacowards.year_summary(*year);
                    let fraction = if total > 0 {
                        done as f32 / total as f32
                    } else {
                        0.0
                    };
                    bar_row(
                        ui,
                        &year.to_string(),
                        38.0,
                        fraction,
                        &format!("{done} of {total}"),
                        color,
                        &format!("{year}: {done} of {total} completed"),
                    );
                }
            });
        }
    });
}

/// A ranked list's title, its lines, and how to write the figure on each.
type Breakdown<'a> = (&'a str, &'a [Tally], &'a dyn Fn(i64) -> String);

/// What the library is made of: four short ranked lists, as many across as fit.
fn breakdowns(ui: &mut egui::Ui, snap: &StatsSnapshot) {
    let duration = |seconds: i64| {
        // Hours are enough for a figure beside a bar, and the tooltip has the rest.
        format!("{}h", seconds / 3600)
    };
    let number = |n: i64| n.to_string();
    let lists: [Breakdown<'_>; 4] = [
        ("Most played authors", &snap.top_authors, &duration),
        ("Sourceports", &snap.sourceports, &duration),
        ("IWADs", &snap.iwads, &number),
        ("Complevels", &snap.complevels, &number),
    ];

    let gap = COLUMN_GAP;
    let available = ui.available_width();
    let across = if available < NARROW { 2.0 } else { 4.0 };
    let column = (available - gap * (across - 1.0)) / across;
    for row in lists.chunks(across as usize) {
        ui.horizontal_top(|ui| {
            for (index, (title, tallies, figure)) in row.iter().enumerate() {
                if index > 0 {
                    ui.add_space(gap - ui.spacing().item_spacing.x);
                }
                ui.vertical(|ui| {
                    ui.set_width(column);
                    ranked_list(ui, title, tallies, figure);
                });
            }
        });
        ui.add_space(SECTION_GAP);
    }
}

fn ranked_list(ui: &mut egui::Ui, title: &str, tallies: &[Tally], figure: &dyn Fn(i64) -> String) {
    section_header(ui, title, "");
    if tallies.is_empty() {
        ui.colored_label(
            theme::TEXT_MUTED,
            egui::RichText::new("Nothing yet.").size(12.0),
        );
        return;
    }
    let max = tallies
        .iter()
        .map(|tally| tally.value)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    ui.spacing_mut().item_spacing.y = 0.0;
    for tally in tallies {
        let value = figure(tally.value);
        ranked_row(ui, tally, tally.value as f32 / max, &value);
    }
}

/// A name over a thin bar, since a name is as long as it is and a column of them can be narrow.
fn ranked_row(ui: &mut egui::Ui, tally: &Tally, fraction: f32, figure: &str) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), BAR_ROW_HEIGHT + 4.0),
        egui::Sense::hover(),
    );
    let says = format!("{}: {figure}", tally.name);
    let response = response.on_hover_text(&says);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &says));

    let painter = ui.painter_at(rect);
    let font = egui::FontId::proportional(12.0);
    let figure_width = painter
        .layout_no_wrap(figure.to_string(), font.clone(), theme::TEXT_PRIMARY)
        .size()
        .x;
    theme::paint_truncated(
        &painter,
        rect.left_top(),
        &tally.name,
        font.clone(),
        theme::TEXT_SECONDARY,
        rect.width() - figure_width - 6.0,
    );
    painter.text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        figure,
        font,
        theme::TEXT_PRIMARY,
    );
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.max.y - 5.0),
        egui::pos2(rect.max.x, rect.max.y - 2.0),
    );
    painter.rect_filled(track, 1.0, theme::BG_MEDIUM);
    let fill = egui::Rect::from_min_size(
        track.min,
        egui::vec2((track.width() * fraction).max(2.0), track.height()),
    );
    let color = if response.hovered() {
        theme::lift(theme::CHART_BAR)
    } else {
        theme::CHART_BAR
    };
    painter.rect_filled(fill, 1.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(period: &str) -> Month {
        Month::parse(period).unwrap()
    }

    #[test]
    fn a_month_with_nothing_in_it_is_still_on_the_chart() {
        let months =
            months_covering([month("2026-11"), month("2026-02"), month("2025-12")].into_iter());
        let names: Vec<String> = months.iter().map(|m| m.long()).collect();
        assert_eq!(names.first().unwrap(), "December 2025");
        assert_eq!(names.last().unwrap(), "November 2026");
        assert_eq!(months.len(), 12, "the months between count too");
    }

    #[test]
    fn a_long_history_keeps_its_most_recent_months() {
        let months = months_covering([month("2010-01"), month("2026-06")].into_iter());
        assert_eq!(months.len(), MAX_MONTHS);
        assert_eq!(*months.last().unwrap(), month("2026-06"));
    }

    #[test]
    fn nothing_covers_no_months_and_nonsense_is_not_a_month() {
        assert!(months_covering(std::iter::empty()).is_empty());
        assert_eq!(Month::parse("2026-13"), None);
        assert_eq!(Month::parse("whenever"), None);
    }

    /// However the labels fall, the axis says which year it is, even if no January gets one.
    #[test]
    fn the_first_label_of_each_year_says_which_year() {
        let months = months_covering([month("2019-11"), month("2020-04")].into_iter());
        let labels = |stride| -> Vec<String> {
            axis_labels(&months, stride)
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect()
        };
        assert_eq!(labels(1), ["2019", "Dec", "2020", "Feb", "Mar", "Apr"]);
        assert_eq!(labels(2), ["2019", "", "2020", "", "Mar", ""]);
        // The second label is February: the first one that year, with no January to go on.
        assert_eq!(labels(3), ["2019", "", "", "2020", "", ""]);
    }

    #[test]
    fn a_month_reads_as_a_person_would_say_it() {
        assert_eq!(month("2026-03").short(), "Mar");
        assert_eq!(month("2026-03").long(), "March 2026");
    }

    #[test]
    fn an_axis_ends_on_a_tidy_number() {
        assert_eq!(nice_ceiling(0.0), 1.0);
        assert_eq!(nice_ceiling(0.3), 0.5);
        assert_eq!(nice_ceiling(1.0), 1.0);
        assert_eq!(nice_ceiling(3.0), 5.0);
        assert_eq!(nice_ceiling(107.9), 200.0);
        assert_eq!(nice_ceiling(5000.0), 5000.0);
    }

    #[test]
    fn half_a_completion_is_not_a_tick() {
        assert_eq!(axis_ticks(200.0), [0.0, 100.0, 200.0]);
        assert_eq!(axis_ticks(5.0), [0.0, 5.0]);
        assert_eq!(axis_ticks(1.0), [0.0, 1.0]);
    }

    #[test]
    fn labels_skip_bars_when_the_bars_are_narrower_than_a_label() {
        assert_eq!(label_stride(40.0), 1);
        assert_eq!(label_stride(30.0), 1);
        assert_eq!(label_stride(16.0), 2);
        assert_eq!(label_stride(8.0), 4);
    }

    #[test]
    fn finishing_something_reads_as_a_person_would_say_it() {
        assert_eq!(times_finished(0), "nothing finished");
        assert_eq!(times_finished(1), "finished once");
        assert_eq!(times_finished(3), "finished 3 times");
    }
}

//! The one shape every dialog has: a header, a body and a footer. See `docs/ui.md`.
//!
//! Nothing else in the crate opens a window, and there is no way of leaving a part out: a
//! body hands back a footer, and only a footer makes the [`DialogDone`] the closure has to
//! return. Where the buttons go is decided here too, so it cannot be decided differently
//! anywhere else.
//!
//! ```ignore
//! Dialog::new("Enrich", [640.0, 460.0]).show(ctx, |dialog| {
//!     dialog
//!         .scroll(|ui| self.render_output(ui))
//!         .buttons(|row| {
//!             if row.close() {
//!                 result = EnrichResult::Closed;
//!             }
//!         })
//! });
//! ```
//!
//! Sizes used to be budgeted against the screen with a constant per dialog for "everything
//! that isn't the body", which was a guess, and which egui's window then grew past: a window
//! is as tall as its content was last frame, and a body filling what is available fills that,
//! so anything added below it pushed the window down a little further each frame until
//! it hit the cap. That is why Profiles took up the whole screen with nothing in it. Here
//! the footer's height is measured, and the body is told exactly how tall it may be.

use crate::theme;

/// Margin left between a dialog and the app window's edge, per side.
const GUTTER: f32 = 24.0;

/// Between the window's edge and what is in it, across.
const MARGIN_X: i8 = 20;

/// What the footer is taken to be on the first frame, before there is a measurement. Too
/// much is the safe mistake: for one frame, nobody can see it.
const FOOTER_GUESS: f32 = 60.0;

/// How far a body is inset from the rules above and below it.
const BODY_MARGIN_Y: f32 = 12.0;

/// The floor for a body, below which it is not worth showing. At the 800x400 minimum a dialog
/// has about 250 to work with, so nothing asked for by a real footer gets near it.
const MIN_BODY: f32 = 60.0;

/// Clear of the status bar below it and the window's edge beside it.
const FROM_THE_CORNER: [f32; 2] = [-16.0, -36.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// In the middle, over a dimmed app.
    Modal,
    /// In the corner, over anything, for a question nobody is waiting to answer.
    Corner,
}

/// A dialog about to be shown: see the module's docs.
pub struct Dialog {
    title: String,
    about: Option<String>,
    size: [f32; 2],
    id: egui::Id,
    placement: Placement,
}

impl Dialog {
    /// `size` is what it would like to be, and it gets less if the window has less. A dialog
    /// whose body shrinks to fit can end up shorter than this. It is never wider.
    pub fn new(title: impl Into<String>, size: [f32; 2]) -> Self {
        let title = title.into();
        Self {
            id: egui::Id::unique(("dialog", &title)),
            title,
            about: None,
            size,
            placement: Placement::Modal,
        }
    }

    /// A line under the title saying what this is about, usually a WAD.
    pub fn about(mut self, about: impl Into<String>) -> Self {
        self.about = Some(about.into());
        self
    }

    /// For a title that changes, which egui would otherwise take for a new window.
    pub fn keyed(mut self, key: &str) -> Self {
        self.id = egui::Id::unique(("dialog", key));
        self
    }

    /// Ask from the corner, in front of everything, rather than in the middle of the app.
    pub fn in_the_corner(mut self) -> Self {
        self.placement = Placement::Corner;
        self
    }

    pub fn show(self, ctx: &egui::Context, content: impl FnOnce(DialogBody<'_>) -> DialogDone) {
        let screen = ctx.content_rect().size();
        let room = egui::vec2(
            (screen.x - GUTTER * 2.0).max(240.0),
            (screen.y - GUTTER * 2.0).max(160.0),
        );
        let size = egui::Vec2::from(self.size).min(room);
        let frame = theme::dialog_frame();
        let frame_margin = frame.total_margin().sum();

        let window = egui::Window::new(&self.title)
            .id(self.id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .frame(frame)
            .default_size(size)
            .min_width(size.x)
            .max_width(size.x)
            .max_height(room.y)
            .constrain(true);
        let window = match self.placement {
            Placement::Modal => window.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]),
            Placement::Corner => window
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_BOTTOM, FROM_THE_CORNER),
        };

        window.show(ctx, |ui| {
            let top = ui.cursor().top();
            ui.spacing_mut().item_spacing.y = 0.0;
            render_header(ui, &self.title, self.about.as_deref());
            content(DialogBody {
                ui,
                measure: Measure {
                    id: self.id,
                    top,
                    height: size.y - frame_margin.y,
                },
            });
        });
    }
}

/// What the parts of a dialog need to agree on its height.
#[derive(Clone, Copy)]
struct Measure {
    id: egui::Id,
    /// Where the window's content starts.
    top: f32,
    height: f32,
}

impl Measure {
    fn footer_key(self) -> egui::Id {
        self.id.with("footer height")
    }

    /// What is left for the body, given what is above it and what the footer took last frame.
    fn body_height(self, ui: &egui::Ui) -> f32 {
        let above = ui.cursor().top() - self.top;
        let footer = ui
            .ctx()
            .data(|data| data.get_temp::<f32>(self.footer_key()))
            .unwrap_or(FOOTER_GUESS);
        (self.height - above - footer).max(MIN_BODY)
    }
}

fn render_header(ui: &mut egui::Ui, title: &str, about: Option<&str>) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(MARGIN_X, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .size(15.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                )
                .truncate(),
            );
            if let Some(about) = about {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(about)
                            .size(12.0)
                            .color(theme::TEXT_SECONDARY),
                    )
                    .truncate(),
                );
            }
        });
    rule(ui);
}

/// A line across the whole dialog, up to its edges.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::BORDER);
}

/// Spacing back to what a body expects, after the dialog's own parts have packed tight.
fn body_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.y = 4.0;
}

/// Where a dialog's content goes. Ends in a footer, one way or the other.
pub struct DialogBody<'a> {
    ui: &'a mut egui::Ui,
    measure: Measure,
}

impl<'a> DialogBody<'a> {
    /// What stays in view over the part that scrolls: tabs, a filter, an error.
    pub fn pinned(self, add: impl FnOnce(&mut egui::Ui)) -> Self {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: MARGIN_X,
                right: MARGIN_X,
                top: 8,
                bottom: 0,
            })
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                body_spacing(ui);
                add(ui);
            });
        self
    }

    /// A body that scrolls if it has to, and shrinks to what it holds if it doesn't.
    pub fn scroll(self, add: impl FnOnce(&mut egui::Ui)) -> DialogFooter<'a> {
        let height = self.measure.body_height(self.ui);
        egui::ScrollArea::vertical()
            .id_salt("dialog body")
            .auto_shrink([false, true])
            .max_height(height)
            .show(self.ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(MARGIN_X, BODY_MARGIN_Y as i8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        body_spacing(ui);
                        add(ui);
                    });
            });
        DialogFooter::new(self.ui, self.measure)
    }

    /// A body that takes the whole height, and is told what that is. For one that looks after
    /// its own scrolling: panes side by side, or a table. It should stay inside what it is
    /// given, since the window would grow to fit if it did not.
    pub fn fill(self, add: impl FnOnce(&mut egui::Ui, f32)) -> DialogFooter<'a> {
        let height = self.measure.body_height(self.ui) - BODY_MARGIN_Y * 2.0;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(MARGIN_X, BODY_MARGIN_Y as i8))
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_height(height);
                body_spacing(ui);
                add(ui, height);
            });
        DialogFooter::new(self.ui, self.measure)
    }
}

/// The part of a dialog that never scrolls away.
pub struct DialogFooter<'a> {
    ui: &'a mut egui::Ui,
    measure: Measure,
    top: f32,
}

impl<'a> DialogFooter<'a> {
    fn new(ui: &'a mut egui::Ui, measure: Measure) -> Self {
        let top = ui.cursor().top();
        rule(ui);
        Self { ui, measure, top }
    }

    /// What has to stay in view above the buttons, however the body is scrolled: how
    /// something went, or a question they are about to answer.
    pub fn above(self, add: impl FnOnce(&mut egui::Ui)) -> Self {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: MARGIN_X,
                right: MARGIN_X,
                top: 10,
                bottom: 0,
            })
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                body_spacing(ui);
                add(ui);
            });
        self
    }

    /// The buttons, which finish the dialog: see [`FooterRow`].
    pub fn buttons(self, add: impl FnOnce(&mut FooterRow<'_>)) -> DialogDone {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(MARGIN_X, 10))
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| add(&mut FooterRow { ui }));
            });

        let height = self.ui.cursor().top() - self.top;
        let key = self.measure.footer_key();
        let changed = self.ui.ctx().data_mut(|data| {
            let last = data.get_temp::<f32>(key);
            data.insert_temp(key, height);
            last.is_none_or(|last| (last - height).abs() > 0.5)
        });
        if changed {
            self.ui.ctx().request_repaint();
        }
        DialogDone(())
    }
}

/// What a dialog's content closure returns, and only [`DialogFooter::buttons`] makes one.
pub struct DialogDone(());

/// The row of buttons along the bottom of a dialog. What you might do goes on the left, and
/// what ends the dialog on the right, in that order and only in that order.
pub struct FooterRow<'a> {
    ui: &'a mut egui::Ui,
}

/// Which of a footer's right-hand buttons was pressed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ended {
    pub dismissed: bool,
    pub primary: bool,
}

impl FooterRow<'_> {
    /// What you might do besides ending the dialog. Laid out from the left, and called before
    /// the buttons on the right, or those would have taken the row.
    pub fn left(&mut self, add: impl FnOnce(&mut egui::Ui)) {
        add(self.ui);
    }

    /// The one button a dialog with nothing to lose has. True if it was pressed.
    pub fn close(&mut self) -> bool {
        self.end(Some("Close"), None).dismissed
    }

    /// What ends the dialog, at the right of the row: the way out, and to its right the main
    /// thing to do. Leave out the way out while there isn't one, as during a build, rather
    /// than drawing it disabled.
    pub fn end(&mut self, dismiss: Option<&str>, primary: Option<Primary<'_>>) -> Ended {
        let mut ended = Ended::default();
        self.ui
            .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(primary) = primary {
                    ended.primary = primary.add(ui);
                }
                if let Some(dismiss) = dismiss {
                    ended.dismissed = ui.button(dismiss).clicked();
                }
            });
        ended
    }
}

/// A footer's main button.
pub struct Primary<'a> {
    label: &'a str,
    danger: bool,
    /// Why it can't be pressed yet, if it can't.
    waiting_for: Option<&'a str>,
}

impl<'a> Primary<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            danger: false,
            waiting_for: None,
        }
    }

    /// For what cannot be taken back.
    pub fn danger(label: &'a str) -> Self {
        Self {
            danger: true,
            ..Self::new(label)
        }
    }

    /// Hold it back until `ready`, saying what it is waiting for. This is the one case for a
    /// disabled button: a person can do something about it, and now knows what.
    pub fn when(mut self, ready: bool, waiting_for: &'a str) -> Self {
        self.waiting_for = (!ready).then_some(waiting_for);
        self
    }

    fn add(self, ui: &mut egui::Ui) -> bool {
        let button = if self.danger {
            theme::danger_button(self.label)
        } else {
            theme::primary_button(self.label)
        };
        let response = ui.add_enabled(self.waiting_for.is_none(), button);
        match self.waiting_for {
            Some(why) => response.on_disabled_hover_text(why).clicked(),
            None => response.clicked(),
        }
    }
}

/// Tabs along the top of a body. The one look they have, since there used to be three.
pub fn tab_bar<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    tabs: &[(T, impl AsRef<str>)],
) {
    ui.horizontal(|ui| {
        for (tab, label) in tabs {
            let active = *current == *tab;
            let text = egui::RichText::new(label.as_ref())
                .size(13.0)
                .color(if active {
                    theme::TEXT_ACCENT
                } else {
                    theme::TEXT_SECONDARY
                });
            if ui.selectable_label(active, text).clicked() {
                *current = *tab;
            }
        }
    });
}

/// How the last thing somebody did in a dialog went.
pub struct StatusLine {
    text: String,
    is_error: bool,
}

impl StatusLine {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        let color = if self.is_error {
            theme::COLOR_ERROR
        } else {
            theme::TEXT_SECONDARY
        };
        ui.colored_label(color, &self.text);
    }
}

/// A field's visible label. Returned so the field can name itself after it with
/// `labelled_by`: without that, seeing the two side by side is the only thing connecting them.
pub fn form_label(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.colored_label(
        theme::TEXT_SECONDARY,
        egui::RichText::new(label).size(12.0).strong(),
    )
}

/// Scroll a region whose height has already been decided by whoever owns the window, as a
/// tab's list is by the dialog around it.
pub fn scroll_body<R>(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(height.max(MIN_BODY))
        .show(ui, add)
        .inner
}

/// Plural `word`, spelled out: `3 WADs`, never `3 WAD(s)`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn x_of(harness: &Harness<'_>, label: &str) -> f32 {
        harness.get_by_label(label).rect().min.x
    }

    #[test]
    fn the_buttons_go_in_the_same_order_whatever_order_they_are_asked_for_in() {
        let harness = Harness::builder()
            .with_size(egui::vec2(800.0, 400.0))
            .build_ui(|ui| {
                Dialog::new("Test", [500.0, 300.0]).show(ui.ctx(), |dialog| {
                    dialog
                        .scroll(|ui| {
                            ui.label("Body");
                        })
                        .buttons(|row| {
                            row.left(|ui| {
                                let _ = ui.button("Other");
                            });
                            row.end(Some("Cancel"), Some(Primary::new("Save")));
                        })
                });
            });
        let (other, cancel, save) = (
            x_of(&harness, "Other"),
            x_of(&harness, "Cancel"),
            x_of(&harness, "Save"),
        );
        assert!(other < cancel, "{other} < {cancel}");
        assert!(cancel < save, "{cancel} < {save}");
    }

    /// The bug this was written to end: a body that fills the window with a footer below it
    /// grew the window a little further every frame, up to the screen.
    #[test]
    fn a_dialog_stays_the_size_it_asked_for() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_ui(|ui| {
                Dialog::new("Test", [500.0, 300.0]).show(ui.ctx(), |dialog| {
                    dialog
                        .fill(|ui, height| {
                            scroll_body(ui, height, |ui| {
                                ui.label("Body");
                            });
                        })
                        .buttons(|row| {
                            row.close();
                        })
                });
            });
        for _ in 0..20 {
            harness.step();
        }
        let window = harness.get_by_role_and_label(egui::accesskit::Role::Window, "Test");
        let size = window.rect();
        assert_eq!(size.width().round(), 500.0);
        assert_eq!(size.height().round(), 300.0);
    }

    #[test]
    fn a_dialog_on_a_small_window_stays_inside_it() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(800.0, 400.0))
            .build_ui(|ui| {
                Dialog::new("Test", [900.0, 700.0]).show(ui.ctx(), |dialog| {
                    dialog
                        .scroll(|ui| {
                            for line in 0..100 {
                                ui.label(format!("Line {line}"));
                            }
                        })
                        .buttons(|row| {
                            row.close();
                        })
                });
            });
        for _ in 0..10 {
            harness.step();
        }
        let window = harness
            .get_by_role_and_label(egui::accesskit::Role::Window, "Test")
            .rect();
        assert_eq!(window.width().round(), 800.0 - GUTTER * 2.0);
        assert_eq!(window.height().round(), 400.0 - GUTTER * 2.0);
        let close = harness.get_by_label("Close").rect();
        assert!(close.max.y <= 400.0, "{close:?}");
    }

    #[test]
    fn a_short_body_makes_a_short_dialog() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_ui(|ui| {
                Dialog::new("Test", [500.0, 600.0]).show(ui.ctx(), |dialog| {
                    dialog
                        .scroll(|ui| {
                            ui.label("One line");
                        })
                        .buttons(|row| {
                            row.close();
                        })
                });
            });
        for _ in 0..10 {
            harness.step();
        }
        let window = harness
            .get_by_role_and_label(egui::accesskit::Role::Window, "Test")
            .rect();
        assert!(window.height() < 200.0, "{window:?}");
    }

    #[test]
    fn counts_are_spelled_out() {
        assert_eq!(count(1, "WAD", "WADs"), "1 WAD");
        assert_eq!(count(3, "WAD", "WADs"), "3 WADs");
        assert_eq!(count(0, "file", "files"), "0 files");
    }
}

# UI rules

The GUI grew feature by feature, and each surface made its own choices about colour, layout
and words, some of which contradicted each other (#58). These are the choices now, so that a
new surface has one set to follow and nobody has to work them out from the old ones. Where a
rule can be checked by a machine it is, and the check is named.

## Colour

- **Every colour is a token in `theme.rs`.** Nothing else in the crate names a colour.
  (`tests/ui_rules.rs`)
- **Text is readable on every surface it sits on**: 4.5:1 at the least, the WCAG AA line for
  small text. The tokens are chosen to pass, and a new token has to.
  (`theme::tests::text_tokens_are_readable_on_every_surface`)
- **One status palette.** Each status has one colour and one word, and they come from
  `theme::status_style`, wherever a status is shown. A Cacoward entry's two extra states, not
  in the library and unsupported, come from `theme::cacoward_style`, so a card cannot have
  words of its own.

  | Status | Colour | Means |
  |---|---|---|
  | Unplayed | warm grey | nothing has happened yet, so it has no colour to speak of |
  | In Progress | blue | what you are playing: *Continue Playing*, *Now Playing*, the progress bars |
  | Completed | green | finished, and the same green as anything else that went well |
  | Abandoned | rose | your decision, not a failure, so it is not the red of an error |

- **Colour is never the only signal.** A status colour always comes with its word.
- **Red and yellow are for trouble.** `COLOR_ERROR` and `COLOR_WARNING` mean something went
  wrong or is about to. An ordinary message in the status bar is plain text.
- **Finished work is easy to read.** A completed WAD is not dimmed, and green is the loudest
  status. What recedes is what is out of the game: unsupported Cacowards.
- **Accent means act.** The filled accent button is the one thing a surface wants you to do
  next (`theme::primary_button`), so there is at most one per footer or card, and its text is
  dark, which is the only readable choice on that orange.

## Dialogs

Every dialog is built with `dialogs::Dialog`, which owns all of this, and has no way of
leaving a part out: a body hands back a footer, and only a footer produces what the closure
has to return. Nothing else in the crate opens a window. (`tests/ui_rules.rs`)

- **Header**: left-aligned title, with an optional line under it for what the dialog is about,
  usually a WAD. Not a place for paths.
- **Body**: scrolls, and shrinks to what it holds, or is handed its height and looks after
  itself. Either way the header and footer stay put, and it fits an 800x400 window.
- **Footer**: what you might do, on the left. What ends the dialog, on the right, always in
  this order:

  ```
  [ other actions ]                                    [ Cancel ] [ Save ]
  ```

  The dismiss button is `Close` when there is nothing to lose, `Cancel` when there is, and the
  primary button sits to the right of it. A question that has to stay in view while the body
  scrolls goes above the buttons (`DialogFooter::above`).
- **A destructive action is asked about first, in the footer**: the question above the
  buttons, `Cancel` and the action in their usual places, and Escape backs out of the question
  rather than out of the dialog.

## Copy

- **Say what the user gets, in the user's words.** "Fills in the ones you haven't set", not
  "Existing values are never overwritten". How caco keeps a promise goes in `CLAUDE.md` or the
  README.
- **Not the implementation**: no prefix, recipe, remote, orphan, snapshot, row, schema or
  metadata where a person's word will do, and no absolute paths in a dialog's opening
  sentences. A path appears where the path is the point: a folder field, or where a lost file
  was last seen.
- **No internal ids.** A WAD's rowid means nothing to anyone.
- **Plurals are spelled out.** `3 WADs`, never `3 WAD(s)`.
- **One word per thing.** A status has the word `theme` gives it. A *sourceport* is never a
  port. Something caco can't fetch again is *not on a mirror or idgames*, and something nothing
  uses is *unused*.

## Controls

- **Don't draw a control that can't act.** A Cancel with nothing running, a Delete with nothing
  selected or a Close during a build is not there at all. (`screenshots.rs` fails on a disabled
  dismiss button.) The exception is one that is waiting for you to do something, such as Test
  It on a half-filled form, which says what it is waiting for.
- **No status text for the idle state.** An empty report area is empty, not "Idle."
- **Inputs are named after their label**: see `CLAUDE.md`.

## Space

- **800x400 is a real window.** It is `main.rs`'s minimum, and everything has to work there.
  Below `theme::SHORT_WINDOW` tall, the banners collapse to a line so the content gets the room.
- **Lists are for finding, grids are for browsing.** Anything people scan has a compact form.

## Evidence

`tests/snapshots.rs` compares the main surfaces with the images in `tests/snapshots/`, in CI,
on a library it makes up. A change to how something looks fails there until the images are
updated, so the images in the pull request are the review, including on the weekly dependency
update, which would otherwise merge and release with nobody looking. `screenshots.rs` renders
everything else, against a real library if there is one, and uploads the lot from CI.

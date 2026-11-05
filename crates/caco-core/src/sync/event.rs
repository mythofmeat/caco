//! One line of a machine's log.
//!
//! The format is its own thing, and not a copy of `library.db`. Field names are chosen
//! here and mapped to columns by [`super::capture`], so a migration that renames a
//! column changes that table and not what any log says, which is how a log written by
//! this version goes on replaying on later ones. `tests` replays one from a file that
//! is never regenerated, to hold that to account.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::clock::Clock;

/// What this build writes, and the newest it can read. A line from a newer version waits
/// in `sync_parked` for an upgrade instead of being half understood, because there is no
/// getting back a line that was marked seen and read wrongly.
pub const VERSION: u32 = 1;

/// How much an event's fields are worth against what is already there.
///
/// An import on two machines that had not heard of each other makes the same WAD twice,
/// and its `add` is not anyone changing their mind. It must not overwrite a rating given
/// on the other machine, however late it comes, and it must lose the same way whichever
/// arrives first, so [`Op::Add`] sorts below every [`Op::Set`], before the clocks are
/// compared. A row that was there when the log began is logged as an add for the same
/// reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Add,
    Set,
    Delete,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Add => "add",
            Op::Set => "set",
            Op::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "add" => Some(Op::Add),
            "set" => Some(Op::Set),
            "delete" => Some(Op::Delete),
            _ => None,
        }
    }

    /// Sorts in front of a clock, so that comparing two such strings compares the ops
    /// and then the clocks: see [`Op`].
    pub fn rank(self) -> char {
        match self {
            Op::Add => 'a',
            Op::Set | Op::Delete => 's',
        }
    }
}

/// Something that happened on some machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// The format this line is in.
    pub v: u32,
    /// Unique to this event, which is what makes reading a line twice harmless.
    pub id: String,
    /// Which machine it happened on.
    pub machine: String,
    pub clock: Clock,
    /// When it happened by that machine's reckoning, as RFC 3339. Only for people: the
    /// order goes by `clock`.
    pub at: String,
    pub op: Op,
    /// What kind of thing it happened to: see [`super::capture::ENTITIES`].
    pub kind: String,
    /// Which one of those.
    pub key: String,
    /// What it now says.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
}

/// What a line of a log turned out to be.
#[derive(Debug, PartialEq)]
pub enum Line {
    Event(Box<Event>),
    /// A format newer than this build, which is kept for when it is upgraded: the id, so
    /// that it is only kept once, and the clock, to apply it in order then.
    TooNew {
        id: String,
        clock: Clock,
    },
    /// Not something any version of caco wrote: a torn write, or a mangled merge.
    Unreadable,
}

/// Read one line of a log.
pub fn parse_line(line: &str) -> Line {
    // In two steps, because a newer version is free to change everything but these.
    #[derive(Deserialize)]
    struct Header {
        v: u32,
        id: String,
        clock: Clock,
    }
    let Ok(header) = serde_json::from_str::<Header>(line) else {
        return Line::Unreadable;
    };
    if header.v > VERSION {
        return Line::TooNew {
            id: header.id,
            clock: header.clock,
        };
    }
    match serde_json::from_str::<Event>(line) {
        Ok(event) if event.machine_is_valid() => Line::Event(Box::new(event)),
        _ => Line::Unreadable,
    }
}

impl Event {
    fn machine_is_valid(&self) -> bool {
        super::machine::is_machine_id(&self.machine)
    }

    /// As it is written to a log: one line, without the newline.
    pub fn to_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Event {
        let mut fields = Map::new();
        fields.insert("status".into(), Value::String("completed".into()));
        Event {
            v: VERSION,
            id: "abc".into(),
            machine: "laptop-3fa9c1d2".into(),
            clock: Clock::at(1_700_000_000_000, "laptop-3fa9c1d2"),
            at: "2023-11-14T22:13:20.000Z".into(),
            op: Op::Set,
            kind: "wad".into(),
            key: "idgames-12345".into(),
            fields,
        }
    }

    #[test]
    fn test_an_event_reads_back_as_it_was_written() {
        let event = event();
        let line = event.to_line().unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(parse_line(&line), Line::Event(Box::new(event)));
    }

    /// A newline in a note must not end the line, or one note could tear an event in two.
    #[test]
    fn test_text_with_newlines_stays_on_one_line() {
        let mut event = event();
        event
            .fields
            .insert("notes".into(), Value::String("first\nsecond\r\n".into()));
        let line = event.to_line().unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(parse_line(&line), Line::Event(Box::new(event)));
    }

    #[test]
    fn test_a_newer_format_is_kept_rather_than_guessed_at() {
        let clock = Clock::at(5, "m");
        let line = format!(
            r#"{{"v":{},"id":"x","clock":"{clock}","anything":"at all"}}"#,
            VERSION + 1
        );
        assert_eq!(
            parse_line(&line),
            Line::TooNew {
                id: "x".into(),
                clock
            }
        );
    }

    /// Extra fields on a line of a known version are what additions look like.
    #[test]
    fn test_a_field_this_build_does_not_know_is_no_reason_to_refuse_a_line() {
        let line = event().to_line().unwrap();
        let line = line.replacen('{', r#"{"something_new":[1,2,3],"#, 1);
        assert!(matches!(parse_line(&line), Line::Event(_)));
    }

    #[test]
    fn test_what_no_version_wrote_is_unreadable() {
        let good = event().to_line().unwrap();
        for bad in [
            "",
            "not json",
            "{}",
            "[1,2]",
            &good[..good.len() / 2],
            r#"{"v":1,"id":"x","clock":"nonsense"}"#,
        ] {
            assert_eq!(parse_line(bad), Line::Unreadable, "{bad}");
        }
    }

    #[test]
    fn test_a_machine_that_could_not_be_one_is_unreadable() {
        let mut event = event();
        event.machine = "../../etc".into();
        assert_eq!(parse_line(&event.to_line().unwrap()), Line::Unreadable);
    }
}

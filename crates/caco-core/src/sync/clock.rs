//! Ordering what happened on machines whose clocks disagree.
//!
//! A hybrid logical clock: the wall clock, plus a counter for when it has nothing new
//! to say, plus the machine, to break a tie the same way everywhere. A machine's next
//! event always sorts after every event it has seen, so an edit made after reading
//! another machine's edit beats it even if the other machine's clock runs ahead.
//! Clocks only decide what is otherwise a coin toss: two machines that edit the same
//! field without having seen each other get the later wall clock, which is the best
//! guess there is, and the same answer on both.
//!
//! Spelled so that comparing the strings compares the clocks, which is what lets
//! [`super::merge`] keep them in SQLite as text.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Thirteen digits of milliseconds reach the year 2286.
const MAX_WALL_MS: u64 = 9_999_999_999_999;
const MAX_COUNTER: u32 = 999_999;

/// When an event happened, as far as any machine can tell.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Clock {
    wall_ms: u64,
    counter: u32,
    machine: String,
}

impl Clock {
    /// A clock for something that happened at `wall_ms`, for rows that were already
    /// there when the log began: no counter, because nothing is being ordered after
    /// anything.
    pub fn at(wall_ms: u64, machine: &str) -> Self {
        Self {
            wall_ms: wall_ms.min(MAX_WALL_MS),
            counter: 0,
            machine: machine.to_string(),
        }
    }

    pub fn wall_ms(&self) -> u64 {
        self.wall_ms
    }
}

impl Ord for Clock {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.wall_ms, self.counter, &self.machine).cmp(&(
            other.wall_ms,
            other.counter,
            &other.machine,
        ))
    }
}

impl PartialOrd for Clock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:013}.{:06}.{}",
            self.wall_ms, self.counter, self.machine
        )
    }
}

impl FromStr for Clock {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("not a clock: {s}");
        let mut parts = s.splitn(3, '.');
        let (Some(wall), Some(counter), Some(machine)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        // Exactly these widths, or comparing the strings would stop comparing the clocks.
        if wall.len() != 13 || counter.len() != 6 || !super::machine::is_machine_id(machine) {
            return Err(bad());
        }
        let digits = |text: &str| text.bytes().all(|b| b.is_ascii_digit());
        if !digits(wall) || !digits(counter) {
            return Err(bad());
        }
        Ok(Self {
            wall_ms: wall.parse().map_err(|_| bad())?,
            counter: counter.parse().map_err(|_| bad())?,
            machine: machine.to_string(),
        })
    }
}

impl TryFrom<String> for Clock {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Clock> for String {
    fn from(clock: Clock) -> Self {
        clock.to_string()
    }
}

/// The newest moment this library has heard of, which the next event must sort after.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hlc {
    wall_ms: u64,
    counter: u32,
}

impl Hlc {
    /// The clock of something happening now, at `wall_ms` by this machine's reckoning.
    pub fn tick(&mut self, wall_ms: u64, machine: &str) -> Clock {
        let wall_ms = wall_ms.min(MAX_WALL_MS);
        if wall_ms > self.wall_ms {
            self.wall_ms = wall_ms;
            self.counter = 0;
        } else if self.counter < MAX_COUNTER {
            self.counter += 1;
        } else {
            // A million events in one millisecond, or a clock read off a damaged line.
            // Either way the order is what has to hold.
            self.wall_ms = (self.wall_ms + 1).min(MAX_WALL_MS);
            self.counter = 0;
        }
        Clock {
            wall_ms: self.wall_ms,
            counter: self.counter,
            machine: machine.to_string(),
        }
    }

    /// Note an event seen, so that whatever happens next sorts after it. Goes along with
    /// a clock that runs ahead, rather than refusing it, because the only cost is that
    /// later events carry its time, and refusing would let its edits win over later ones.
    pub fn observe(&mut self, clock: &Clock) {
        if (clock.wall_ms, clock.counter) > (self.wall_ms, self.counter) {
            self.wall_ms = clock.wall_ms;
            self.counter = clock.counter;
        }
    }

    /// `"<wall>.<counter>"`, as `sync_state` keeps it.
    pub fn to_state(self) -> String {
        format!("{}.{}", self.wall_ms, self.counter)
    }

    /// Reads [`Self::to_state`]. Starts over if it cannot, which costs nothing lasting:
    /// the next merge observes every clock it has not seen.
    pub fn from_state(state: &str) -> Self {
        state
            .split_once('.')
            .and_then(|(wall, counter)| Some((wall.parse().ok()?, counter.parse().ok()?)))
            .map(|(wall_ms, counter)| Self { wall_ms, counter })
            .unwrap_or_default()
    }
}

/// The milliseconds a timestamp in the library means, in any spelling one has been
/// written in: RFC 3339 from caco, `CURRENT_TIMESTAMP` from SQLite (UTC, no zone) and
/// the zoneless ISO the Python version wrote.
pub fn wall_ms_of(text: &str) -> Option<u64> {
    use chrono::{DateTime, NaiveDateTime};

    let text = text.trim();
    let millis = DateTime::parse_from_rfc3339(text)
        .map(|at| at.timestamp_millis())
        .or_else(|_| {
            NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f"))
                .map(|at| at.and_utc().timestamp_millis())
        })
        .ok()?;
    u64::try_from(millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comparing_the_strings_compares_the_clocks() {
        let clocks = [
            Clock::at(5, "a"),
            Clock::at(5, "b"),
            Clock {
                wall_ms: 5,
                counter: 1,
                machine: "a".into(),
            },
            Clock::at(1_700_000_000_000, "a"),
            Clock::at(MAX_WALL_MS, "a"),
        ];
        for a in &clocks {
            for b in &clocks {
                assert_eq!(a.cmp(b), a.to_string().cmp(&b.to_string()), "{a} vs {b}");
            }
        }
    }

    #[test]
    fn test_a_clock_survives_being_written_down() {
        let mut hlc = Hlc::default();
        let clock = hlc.tick(1_700_000_000_123, "laptop-3fa9c1d2");
        assert_eq!(clock.to_string().parse::<Clock>().unwrap(), clock);

        let json = serde_json::to_string(&clock).unwrap();
        assert_eq!(serde_json::from_str::<Clock>(&json).unwrap(), clock);
    }

    #[test]
    fn test_a_clock_that_would_not_sort_is_refused() {
        for bad in [
            "",
            "5.0.a",
            "0000000000005.000000",
            "000000000000a.000000.a",
            "0000000000005.00000a.a",
            "0000000000005.000000.",
            "0000000000005.000000.has space",
            "0000000000005.000000.../../etc",
        ] {
            assert!(bad.parse::<Clock>().is_err(), "{bad}");
        }
    }

    #[test]
    fn test_ticks_always_move_forward_even_with_the_wall_clock_going_back() {
        let mut hlc = Hlc::default();
        let a = hlc.tick(1_000, "m");
        let b = hlc.tick(1_000, "m");
        let c = hlc.tick(500, "m");
        let d = hlc.tick(2_000, "m");
        assert!(a < b && b < c && c < d);
    }

    /// The whole reason for not just using the wall clock: a machine whose clock is an
    /// hour behind still gets the last word, if it had heard the other's.
    #[test]
    fn test_what_comes_after_seeing_an_event_sorts_after_it() {
        let mut ahead = Hlc::default();
        let theirs = ahead.tick(10_000_000, "ahead");

        let mut behind = Hlc::default();
        behind.observe(&theirs);
        let ours = behind.tick(100, "behind");
        assert!(ours > theirs);
    }

    #[test]
    fn test_a_tie_goes_the_same_way_everywhere() {
        let mut a = Hlc::default();
        let mut b = Hlc::default();
        let from_a = a.tick(1_000, "a");
        let from_b = b.tick(1_000, "b");
        assert!(from_b > from_a);
    }

    #[test]
    fn test_a_counter_that_runs_out_still_moves_forward() {
        let mut hlc = Hlc {
            wall_ms: 1_000,
            counter: MAX_COUNTER,
        };
        let before = Clock {
            wall_ms: 1_000,
            counter: MAX_COUNTER,
            machine: "m".into(),
        };
        assert!(hlc.tick(1_000, "m") > before);
    }

    /// A damaged line must not be able to stop time, or overflow it.
    #[test]
    fn test_the_last_moment_there_is_still_ticks() {
        let mut hlc = Hlc::default();
        hlc.observe(&Clock::at(MAX_WALL_MS, "m"));
        let a = hlc.tick(5, "m");
        let b = hlc.tick(5, "m");
        assert!(b > a);
        assert_eq!(a.wall_ms(), MAX_WALL_MS);
    }

    #[test]
    fn test_state_round_trips_and_nonsense_starts_over() {
        let mut hlc = Hlc::default();
        hlc.tick(1_234, "m");
        hlc.tick(1_234, "m");
        assert_eq!(Hlc::from_state(&hlc.to_state()), hlc);
        assert_eq!(Hlc::from_state("nonsense"), Hlc::default());
    }

    #[test]
    fn test_every_spelling_of_a_time_in_a_real_library() {
        for (text, expected) in [
            ("2026-11-05T01:06:07.647882333+00:00", 1_793_840_767_647),
            ("2026-11-05T01:06:07.647Z", 1_793_840_767_647),
            ("2026-11-05 01:06:07", 1_793_840_767_000),
            ("2026-11-05T01:06:07.5", 1_793_840_767_500),
        ] {
            assert_eq!(wall_ms_of(text), Some(expected), "{text}");
        }
        assert_eq!(wall_ms_of("yesterday"), None);
        assert_eq!(wall_ms_of("1969-01-01T00:00:00Z"), None);
    }
}

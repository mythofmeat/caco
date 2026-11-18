//! What the status bar says when a session ends, or a demo's playback does.

use caco_core::player::{self, AutoCompleteResult, DemoResult, PlayResult};

use crate::message::Notification;

/// The one line a finished session gets: a crash first, then a recording that came to nothing,
/// then a finished WAD, then map exits that had to be read from a save or could not be read at
/// all, then how long it was. What a session recorded is said after whichever of those it is.
pub(super) fn play_notification(played: &PlayResult) -> Option<Notification> {
    let recorded = recorded_note(played.recorded.as_deref());
    if played.crashed() {
        let crashed = format!(
            "Sourceport crashed (exit code {})",
            played.exit_code.unwrap_or(-1)
        );
        return Some(Notification::warning(then(crashed, recorded.as_deref())));
    }
    if played.recorded.as_ref().is_some_and(Vec::is_empty) {
        return Some(Notification::warning(
            "Nothing was recorded: the sourceport didn't write a demo.".to_string(),
        ));
    }
    if played.auto_complete == AutoCompleteResult::Completed {
        return Some(Notification::info(then(
            "All maps completed! Marked as finished.".to_string(),
            recorded.as_deref(),
        )));
    }
    let from_saves = &played.stats.from_saves;
    if let Some(sourceport) = &played.stats.log_missing_from {
        let text = if from_saves.is_empty() {
            format!(
                "{sourceport} didn't say which maps you finished, so any from this session \
                 may not be counted."
            )
        } else {
            format!(
                "{sourceport} didn't say which maps you finished, so caco went by your \
                 latest save and counted {}.",
                maps(from_saves)
            )
        };
        return Some(Notification::warning(then(text, recorded.as_deref())));
    }
    let played_for = played
        .duration
        .map(|seconds| format!("Played for {}", player::format_duration(seconds)));
    if !from_saves.is_empty() {
        let counted = format!("Counted {} from your saves.", maps(from_saves));
        let text = match played_for {
            Some(played_for) => format!("{played_for}. {counted}"),
            None => counted,
        };
        return Some(Notification::info(then(text, recorded.as_deref())));
    }
    match (played_for, recorded) {
        (Some(played_for), recorded) => {
            Some(Notification::info(then(played_for, recorded.as_deref())))
        }
        (None, Some(recorded)) => Some(Notification::info(recorded)),
        (None, None) => None,
    }
}

/// The line a demo's playback gets: whether the sourceport fell over, or how long it ran.
pub(super) fn demo_notification(watched: &DemoResult) -> Notification {
    if watched.crashed() {
        return Notification::warning(format!(
            "The sourceport crashed playing {} back (exit code {}).",
            watched.demo,
            watched.exit_code.unwrap_or(-1)
        ));
    }
    Notification::info(format!(
        "Played back {} for {}.",
        watched.demo,
        player::format_duration(watched.duration)
    ))
}

/// What a session recorded, if it recorded anything.
fn recorded_note(recorded: Option<&[String]>) -> Option<String> {
    match recorded? {
        [] => None,
        [one] => Some(format!("Recorded {one}.")),
        many => Some(format!("Recorded {} demos.", many.len())),
    }
}

/// One sentence, then another if there is one.
fn then(first: String, next: Option<&str>) -> String {
    match next {
        None => first,
        Some(next) if first.ends_with(['.', '!', '?']) => format!("{first} {next}"),
        Some(next) => format!("{first}. {next}"),
    }
}

/// A few maps by name, and more than that by how many.
fn maps(lumps: &[String]) -> String {
    match lumps {
        [] => "no maps".to_string(),
        [one] => one.clone(),
        [init @ .., last] if lumps.len() <= 3 => format!("{} and {last}", init.join(", ")),
        _ => format!("{} maps", lumps.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Severity;
    use caco_core::player::StatsReport;

    fn session(stats: StatsReport) -> PlayResult {
        PlayResult {
            duration: Some(1500),
            exit_code: Some(0),
            auto_complete: AutoCompleteResult::Incomplete {
                exited: 2,
                required: 32,
            },
            stats,
            recorded: None,
        }
    }

    fn said(played: &PlayResult) -> Option<(Severity, String)> {
        play_notification(played).map(|n| (n.severity, n.text))
    }

    fn lumps(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn test_an_ordinary_session_says_how_long_it_was() {
        assert_eq!(
            said(&session(StatsReport::default())),
            Some((Severity::Info, "Played for 25m 0s".to_string()))
        );
    }

    #[test]
    fn test_a_session_with_no_log_is_a_warning() {
        let lost = session(StatsReport {
            log_missing_from: Some("uzdoom".to_string()),
            from_saves: Vec::new(),
        });
        assert_eq!(
            said(&lost),
            Some((
                Severity::Warning,
                "uzdoom didn't say which maps you finished, so any from this session may not \
                 be counted."
                    .to_string()
            ))
        );

        let saved = session(StatsReport {
            log_missing_from: Some("uzdoom".to_string()),
            from_saves: lumps(&["MAP03"]),
        });
        assert_eq!(
            said(&saved),
            Some((
                Severity::Warning,
                "uzdoom didn't say which maps you finished, so caco went by your latest save \
                 and counted MAP03."
                    .to_string()
            ))
        );
    }

    #[test]
    fn test_maps_counted_from_saves_are_mentioned() {
        let played = session(StatsReport {
            log_missing_from: None,
            from_saves: lumps(&["MAP01", "MAP02"]),
        });
        assert_eq!(
            said(&played),
            Some((
                Severity::Info,
                "Played for 25m 0s. Counted MAP01 and MAP02 from your saves.".to_string()
            ))
        );
    }

    #[test]
    fn test_a_crash_or_a_finished_wad_comes_first() {
        let mut played = session(StatsReport {
            log_missing_from: Some("uzdoom".to_string()),
            from_saves: lumps(&["MAP32"]),
        });
        played.auto_complete = AutoCompleteResult::Completed;
        assert_eq!(
            said(&played).unwrap().1,
            "All maps completed! Marked as finished."
        );

        played.exit_code = Some(139);
        assert_eq!(
            said(&played),
            Some((
                Severity::Warning,
                "Sourceport crashed (exit code 139)".to_string()
            ))
        );
    }

    #[test]
    fn test_a_recording_says_what_it_recorded_after_everything_else() {
        let mut played = session(StatsReport::default());
        played.recorded = Some(lumps(&["scythe_20261118_090000.lmp"]));
        assert_eq!(
            said(&played),
            Some((
                Severity::Info,
                "Played for 25m 0s. Recorded scythe_20261118_090000.lmp.".to_string()
            ))
        );

        played.auto_complete = AutoCompleteResult::Completed;
        assert_eq!(
            said(&played).unwrap().1,
            "All maps completed! Marked as finished. Recorded scythe_20261118_090000.lmp."
        );

        played.recorded = Some(lumps(&["a.lmp", "a-00002.lmp"]));
        assert!(said(&played).unwrap().1.ends_with("Recorded 2 demos."));
    }

    #[test]
    fn test_a_recording_that_came_to_nothing_is_a_warning() {
        let mut played = session(StatsReport::default());
        played.recorded = Some(Vec::new());
        assert_eq!(
            said(&played),
            Some((
                Severity::Warning,
                "Nothing was recorded: the sourceport didn't write a demo.".to_string()
            ))
        );

        played.exit_code = Some(1);
        assert_eq!(said(&played).unwrap().1, "Sourceport crashed (exit code 1)");
        played.recorded = Some(lumps(&["run.lmp"]));
        assert_eq!(
            said(&played).unwrap().1,
            "Sourceport crashed (exit code 1). Recorded run.lmp."
        );
    }

    #[test]
    fn test_a_demo_says_how_it_ended() {
        let watched = DemoResult {
            demo: "run.lmp".to_string(),
            duration: 95,
            exit_code: Some(0),
        };
        let line = demo_notification(&watched);
        assert_eq!(
            (line.severity, line.text.as_str()),
            (Severity::Info, "Played back run.lmp for 1m 35s.")
        );
        let crashed = DemoResult {
            exit_code: Some(139),
            ..watched
        };
        let line = demo_notification(&crashed);
        assert_eq!(
            (line.severity, line.text.as_str()),
            (
                Severity::Warning,
                "The sourceport crashed playing run.lmp back (exit code 139)."
            )
        );
    }

    #[test]
    fn test_many_maps_are_counted_rather_than_named() {
        assert_eq!(maps(&lumps(&["E1M1"])), "E1M1");
        assert_eq!(
            maps(&lumps(&["E1M1", "E1M2", "E1M3"])),
            "E1M1, E1M2 and E1M3"
        );
        assert_eq!(maps(&lumps(&["E1M1", "E1M2", "E1M3", "E1M4"])), "4 maps");
    }
}

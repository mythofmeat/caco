//! What the status bar says when a session ends.

use caco_core::player::{self, AutoCompleteResult, PlayResult};

use crate::message::Notification;

/// The one line a finished session gets: a crash first, then a finished WAD, then map exits
/// that had to be read from a save or could not be read at all, then how long it was.
pub(super) fn play_notification(played: &PlayResult) -> Option<Notification> {
    if played.crashed() {
        return Some(Notification::warning(format!(
            "Sourceport crashed (exit code {})",
            played.exit_code.unwrap_or(-1)
        )));
    }
    if played.auto_complete == AutoCompleteResult::Completed {
        return Some(Notification::info(
            "All maps completed! Marked as finished.".to_string(),
        ));
    }
    let from_saves = &played.stats.from_saves;
    if let Some(sourceport) = &played.stats.log_missing_from {
        return Some(Notification::warning(if from_saves.is_empty() {
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
        }));
    }
    let played_for = played
        .duration
        .map(|seconds| format!("Played for {}", player::format_duration(seconds)));
    if !from_saves.is_empty() {
        let counted = format!("Counted {} from your saves.", maps(from_saves));
        return Some(Notification::info(match played_for {
            Some(played_for) => format!("{played_for}. {counted}"),
            None => counted,
        }));
    }
    played_for.map(Notification::info)
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
    fn test_many_maps_are_counted_rather_than_named() {
        assert_eq!(maps(&lumps(&["E1M1"])), "E1M1");
        assert_eq!(
            maps(&lumps(&["E1M1", "E1M2", "E1M3"])),
            "E1M1, E1M2 and E1M3"
        );
        assert_eq!(maps(&lumps(&["E1M1", "E1M2", "E1M3", "E1M4"])), "4 maps");
    }
}

use std::collections::HashMap;
use std::sync::LazyLock;

/// Human-readable names for common complevels.
pub static COMPLEVEL_NAMES: LazyLock<HashMap<i32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "Doom v1.2"),
        (1, "Doom v1.666"),
        (2, "Doom v1.9 / Vanilla"),
        (3, "Ultimate Doom"),
        (4, "Final Doom"),
        (9, "Boom"),
        (11, "MBF"),
        (21, "MBF21"),
    ])
});

/// Aliases: string name -> complevel int.
pub static COMPLEVEL_ALIASES: LazyLock<HashMap<&'static str, i32>> = LazyLock::new(|| {
    HashMap::from([
        ("vanilla", 2),
        ("boom", 9),
        ("mbf", 11),
        ("mbf21", 21),
        ("limit-removing", 2),
        ("lr", 2),
    ])
});

/// Get human-readable name for a complevel.
pub fn complevel_name(cl: Option<i32>) -> &'static str {
    match cl {
        Some(n) => COMPLEVEL_NAMES.get(&n).copied().unwrap_or("Unknown"),
        None => "Unknown",
    }
}

/// Get human-readable name for a complevel, returning a dynamic string for unknown values.
pub fn complevel_name_string(cl: Option<i32>) -> String {
    match cl {
        Some(n) => match COMPLEVEL_NAMES.get(&n) {
            Some(name) => (*name).to_string(),
            None => format!("Complevel {n}"),
        },
        None => "Unknown".to_string(),
    }
}

/// Maximum valid complevel value (MBF21).
pub const MAX_COMPLEVEL: i32 = 21;

/// Parse a complevel from a string — accepts integer (0–21) or alias name.
///
/// Returns the complevel int, or None if invalid or out of range.
pub fn parse_complevel(value: &str) -> Option<i32> {
    // Try as integer first
    if let Ok(n) = value.parse::<i32>() {
        if (0..=MAX_COMPLEVEL).contains(&n) {
            return Some(n);
        }
        return None;
    }
    // Try as alias
    COMPLEVEL_ALIASES.get(value.to_lowercase().as_str()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complevel_name() {
        assert_eq!(complevel_name(Some(2)), "Doom v1.9 / Vanilla");
        assert_eq!(complevel_name(Some(9)), "Boom");
        assert_eq!(complevel_name(Some(21)), "MBF21");
        assert_eq!(complevel_name(None), "Unknown");
        assert_eq!(complevel_name(Some(99)), "Unknown");
    }

    #[test]
    fn test_complevel_name_string() {
        assert_eq!(complevel_name_string(Some(9)), "Boom");
        assert_eq!(complevel_name_string(Some(99)), "Complevel 99");
        assert_eq!(complevel_name_string(None), "Unknown");
    }

    #[test]
    fn test_parse_complevel_integer() {
        assert_eq!(parse_complevel("9"), Some(9));
        assert_eq!(parse_complevel("21"), Some(21));
        assert_eq!(parse_complevel("0"), Some(0));
    }

    #[test]
    fn test_parse_complevel_alias() {
        assert_eq!(parse_complevel("vanilla"), Some(2));
        assert_eq!(parse_complevel("Boom"), Some(9));
        assert_eq!(parse_complevel("MBF21"), Some(21));
        assert_eq!(parse_complevel("lr"), Some(2));
        assert_eq!(parse_complevel("limit-removing"), Some(2));
    }

    #[test]
    fn test_parse_complevel_invalid() {
        assert_eq!(parse_complevel("invalid"), None);
        assert_eq!(parse_complevel(""), None);
        assert_eq!(parse_complevel("22"), None);
        assert_eq!(parse_complevel("77"), None);
        assert_eq!(parse_complevel("-1"), None);
    }
}

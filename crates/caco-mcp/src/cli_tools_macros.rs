//! Helpers for rendering tool args into CLI argv.

/// Push `--flag` if boolean is true.
pub fn push_flag(argv: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        argv.push(flag.to_string());
    }
}

/// Push `--flag value` if value is Some.
pub fn push_opt<T: std::fmt::Display>(argv: &mut Vec<String>, flag: &str, value: Option<&T>) {
    if let Some(v) = value {
        argv.push(flag.to_string());
        argv.push(v.to_string());
    }
}

/// Push a repeated `--flag value1 --flag value2 ...` for a Vec.
pub fn push_multi<T: std::fmt::Display>(argv: &mut Vec<String>, flag: &str, values: &[T]) {
    for v in values {
        argv.push(flag.to_string());
        argv.push(v.to_string());
    }
}

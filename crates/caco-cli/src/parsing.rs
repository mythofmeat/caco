//! Sort extraction and modify action parsing from CLI arguments.

/// Join query args into a single string, quoting terms that contain whitespace
/// so the query parser's `shell_split` can reconstruct the original tokens.
pub fn join_query_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.contains(char::is_whitespace) {
                if !a.contains('"') {
                    format!("\"{a}\"")
                } else {
                    format!("'{a}'")
                }
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Valid sort field names.
pub const SORT_FIELDS: &[&str] = &[
    "id",
    "playtime",
    "rating",
    "created",
    "title",
    "author",
    "last_played",
    "year",
];

/// Extract inline sort from args (e.g., "title+" or "playtime-").
///
/// Returns (remaining_args, optional (sort_field, sort_desc)).
pub fn extract_sort_from_args(args: &[String]) -> (Vec<String>, Option<(String, bool)>) {
    let mut remaining = Vec::new();
    let mut sort_info = None;

    for arg in args {
        if sort_info.is_some() {
            remaining.push(arg.clone());
            continue;
        }

        // Check for field+ or field- suffix
        if arg.ends_with('+') || arg.ends_with('-') {
            let field = &arg[..arg.len() - 1];
            if SORT_FIELDS.contains(&field) {
                let desc = arg.ends_with('-');
                sort_info = Some((field.to_string(), desc));
                continue;
            }
        }

        // Check for bare field name (defaults to descending)
        if SORT_FIELDS.contains(&arg.as_str()) {
            sort_info = Some((arg.clone(), true));
            continue;
        }

        remaining.push(arg.clone());
    }

    (remaining, sort_info)
}

/// CLI field name to DB column name mapping.
pub fn field_to_column(field: &str) -> &str {
    match field {
        "iwad" => "custom_iwad",
        "sourceport" => "custom_sourceport",
        "args" => "custom_args",
        "idgames-id" => "idgames_id",
        "config" => "custom_config",
        other => other,
    }
}

/// User-facing modify fields.
pub const MODIFY_FIELDS: &[&str] = &[
    "title",
    "author",
    "year",
    "description",
    "status",
    "rating",
    "notes",
    "iwad",
    "sourceport",
    "args",
    "complevel",
    "config",
    "idgames-id",
    "version",
];

/// A parsed modify action from CLI arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum ModifyAction {
    SetField { field: String, value: String },
    ClearField { field: String },
    AddTag { tag: String },
    RemoveAllTags,
    RemoveTag { pattern: String },
    BeatenAdd { count: i64 },
    BeatenRemove { count: i64 },
    BeatenRemoveTimestamp { timestamp: String },
    BeatenSet { count: i64 },
}

/// Result of parsing modify arguments.
pub type ModifyParseResult = (Vec<String>, Vec<ModifyAction>, Option<(String, bool)>);

/// Parse modify args into (query_terms, actions, optional_sort).
///
/// Recognizes:
/// - `field=value` — set field
/// - `!field` — clear field
/// - `tag=value` — add tag
/// - `!tag` — remove all tags
/// - `!tag:pattern` — remove matching tags
/// - `beaten+[N]` — add completions
/// - `beaten-N` or `beaten-TIMESTAMP` — remove completions
/// - `beaten=N` — set completion count
/// - Anything else is a query term
pub fn parse_modify_args(args: &[String]) -> Result<ModifyParseResult, String> {
    let (remaining, sort_info) = extract_sort_from_args(args);

    let mut query_terms = Vec::new();
    let mut actions = Vec::new();

    for arg in &remaining {
        // Check for beaten ops first
        if let Some(count_str) = arg.strip_prefix("beaten+") {
            let count = if count_str.is_empty() {
                1
            } else {
                count_str
                    .parse::<i64>()
                    .map_err(|_| format!("invalid beaten count: {count_str}"))?
            };
            actions.push(ModifyAction::BeatenAdd { count });
            continue;
        }

        if let Some(val) = arg.strip_prefix("beaten-") {
            // Try as integer first
            if let Ok(count) = val.parse::<i64>() {
                actions.push(ModifyAction::BeatenRemove { count });
            } else {
                // Treat as timestamp
                actions.push(ModifyAction::BeatenRemoveTimestamp {
                    timestamp: val.to_string(),
                });
            }
            continue;
        }

        if let Some(val) = arg.strip_prefix("beaten=") {
            let count = val
                .parse::<i64>()
                .map_err(|_| format!("invalid beaten count: {val}"))?;
            actions.push(ModifyAction::BeatenSet { count });
            continue;
        }

        // Check for !field (clear) patterns
        if let Some(name) = arg.strip_prefix('!') {
            if name == "tag" {
                actions.push(ModifyAction::RemoveAllTags);
                continue;
            }
            if let Some(pattern) = name.strip_prefix("tag:") {
                actions.push(ModifyAction::RemoveTag {
                    pattern: pattern.to_string(),
                });
                continue;
            }
            // Clear a field
            let col = field_to_column(name);
            if caco_core::db::ALLOWED_UPDATE_FIELDS.contains(col) || MODIFY_FIELDS.contains(&name) {
                actions.push(ModifyAction::ClearField {
                    field: name.to_string(),
                });
                continue;
            }
        }

        // Check for field=value patterns
        if let Some((field, value)) = arg.split_once('=') {
            if field == "tag" {
                actions.push(ModifyAction::AddTag {
                    tag: value.to_lowercase(),
                });
                continue;
            }
            if MODIFY_FIELDS.contains(&field) {
                actions.push(ModifyAction::SetField {
                    field: field.to_string(),
                    value: value.to_string(),
                });
                continue;
            }
        }

        // Otherwise it's a query term
        query_terms.push(arg.clone());
    }

    Ok((query_terms, actions, sort_info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sort_ascending() {
        let args: Vec<String> = vec!["scythe".into(), "title+".into()];
        let (remaining, sort) = extract_sort_from_args(&args);
        assert_eq!(remaining, vec!["scythe"]);
        assert_eq!(sort, Some(("title".to_string(), false)));
    }

    #[test]
    fn test_extract_sort_descending() {
        let args: Vec<String> = vec!["playtime-".into()];
        let (remaining, sort) = extract_sort_from_args(&args);
        assert!(remaining.is_empty());
        assert_eq!(sort, Some(("playtime".to_string(), true)));
    }

    #[test]
    fn test_extract_sort_bare_field() {
        let args: Vec<String> = vec!["rating".into()];
        let (remaining, sort) = extract_sort_from_args(&args);
        assert!(remaining.is_empty());
        assert_eq!(sort, Some(("rating".to_string(), true)));
    }

    #[test]
    fn test_extract_sort_no_sort() {
        let args: Vec<String> = vec!["scythe".into(), "status:playing".into()];
        let (remaining, sort) = extract_sort_from_args(&args);
        assert_eq!(remaining.len(), 2);
        assert!(sort.is_none());
    }

    #[test]
    fn test_parse_modify_set_field() {
        let args: Vec<String> = vec!["id:1".into(), "status=finished".into()];
        let (query, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(query, vec!["id:1"]);
        assert_eq!(
            actions,
            vec![ModifyAction::SetField {
                field: "status".to_string(),
                value: "finished".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_modify_clear_field() {
        let args: Vec<String> = vec!["id:1".into(), "!notes".into()];
        let (query, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(query, vec!["id:1"]);
        assert_eq!(
            actions,
            vec![ModifyAction::ClearField {
                field: "notes".to_string()
            }]
        );
    }

    #[test]
    fn test_parse_modify_tag_ops() {
        let args: Vec<String> = vec!["id:1".into(), "tag=megawad".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(
            actions,
            vec![ModifyAction::AddTag {
                tag: "megawad".to_string()
            }]
        );

        let args: Vec<String> = vec!["id:1".into(), "!tag".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(actions, vec![ModifyAction::RemoveAllTags]);

        let args: Vec<String> = vec!["id:1".into(), "!tag:caco*".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(
            actions,
            vec![ModifyAction::RemoveTag {
                pattern: "caco*".to_string()
            }]
        );
    }

    #[test]
    fn test_parse_modify_beaten_ops() {
        let args: Vec<String> = vec!["id:1".into(), "beaten+".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(actions, vec![ModifyAction::BeatenAdd { count: 1 }]);

        let args: Vec<String> = vec!["id:1".into(), "beaten+3".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(actions, vec![ModifyAction::BeatenAdd { count: 3 }]);

        let args: Vec<String> = vec!["id:1".into(), "beaten-2".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(actions, vec![ModifyAction::BeatenRemove { count: 2 }]);

        let args: Vec<String> = vec!["id:1".into(), "beaten=5".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(actions, vec![ModifyAction::BeatenSet { count: 5 }]);

        let args: Vec<String> = vec!["id:1".into(), "beaten-2024-06-15".into()];
        let (_, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(
            actions,
            vec![ModifyAction::BeatenRemoveTimestamp {
                timestamp: "2024-06-15".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_modify_mixed() {
        let args: Vec<String> = vec![
            "id:1".into(),
            "status=finished".into(),
            "beaten+1".into(),
            "tag=megawad".into(),
        ];
        let (query, actions, _) = parse_modify_args(&args).unwrap();
        assert_eq!(query, vec!["id:1"]);
        assert_eq!(actions.len(), 3);
    }

    #[test]
    fn test_field_to_column() {
        assert_eq!(field_to_column("iwad"), "custom_iwad");
        assert_eq!(field_to_column("sourceport"), "custom_sourceport");
        assert_eq!(field_to_column("args"), "custom_args");
        assert_eq!(field_to_column("idgames-id"), "idgames_id");
        assert_eq!(field_to_column("config"), "custom_config");
        assert_eq!(field_to_column("title"), "title");
    }

    #[test]
    fn test_join_query_args_no_spaces() {
        let args: Vec<String> = vec!["status:playing".into(), "tag:megawad".into()];
        assert_eq!(join_query_args(&args), "status:playing tag:megawad");
    }

    #[test]
    fn test_join_query_args_with_spaces() {
        // Simulates: caco ls "tag:multi word"  (shell strips outer quotes)
        let args: Vec<String> = vec!["tag:multi word".into()];
        assert_eq!(join_query_args(&args), "\"tag:multi word\"");
    }

    #[test]
    fn test_join_query_args_mixed() {
        let args: Vec<String> = vec!["status:playing".into(), "tag:multi word".into()];
        assert_eq!(join_query_args(&args), "status:playing \"tag:multi word\"");
    }

    #[test]
    fn test_join_query_args_with_double_quotes() {
        let args: Vec<String> = vec!["title:some \"thing\"".into()];
        assert_eq!(join_query_args(&args), "'title:some \"thing\"'");
    }
}

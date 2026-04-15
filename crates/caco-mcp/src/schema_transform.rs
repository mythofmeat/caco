//! Schema transforms applied via `#[schemars(transform = ...)]`.
//!
//! Anthropic's API rejects tool `input_schema` that use `oneOf`/`allOf`/`anyOf`
//! at the top level. `#[serde(tag = "action")]` enums generate exactly such a
//! schema, so we flatten them: every variant's fields are merged into one set
//! of optional properties, and `action` becomes a single `string` enum of all
//! the tag values. This loses per-variant required-field enforcement at the
//! schema layer (the Rust server still enforces it via serde), but it's the
//! only shape the API accepts.

use schemars::Schema;
use serde_json::{Map, Value};

/// Set `type: "object"` at the schema root if it has no `type` field.
pub fn ensure_object_type(schema: &mut Schema) {
    if schema.get("type").is_none() {
        schema.insert("type".into(), Value::String("object".into()));
    }
}

/// Flatten an internally-tagged enum's `oneOf` root into a single merged
/// object schema.
///
/// Input shape (from `#[serde(tag = "action")]`):
/// ```json
/// { "oneOf": [
///     { "type": "object", "properties": { "action": {"const": "list"}, ... }, "required": ["action"] },
///     { "type": "object", "properties": { "action": {"const": "clear"}, ... }, "required": ["action"] },
/// ] }
/// ```
///
/// Output:
/// ```json
/// { "type": "object",
///   "properties": { "action": {"type": "string", "enum": ["list", "clear"]}, ... },
///   "required": ["action"] }
/// ```
pub fn flatten_action_enum(schema: &mut Schema) {
    schema.insert("type".into(), Value::String("object".into()));

    let Some(variants) = schema.get("oneOf").and_then(|v| v.as_array()).cloned() else {
        return;
    };

    let mut merged_props: Map<String, Value> = Map::new();
    let mut action_values: Vec<Value> = Vec::new();

    for variant in &variants {
        let Some(variant_obj) = variant.as_object() else { continue };
        let Some(props) = variant_obj.get("properties").and_then(Value::as_object) else {
            continue;
        };
        for (key, val) in props {
            if key == "action" {
                if let Some(c) = val.get("const")
                    && !action_values.contains(c)
                {
                    action_values.push(c.clone());
                }
            } else if !merged_props.contains_key(key) {
                merged_props.insert(key.clone(), val.clone());
            }
        }
    }

    merged_props.insert(
        "action".into(),
        Value::Object(
            [
                ("type".to_string(), Value::String("string".into())),
                ("enum".to_string(), Value::Array(action_values)),
            ]
            .into_iter()
            .collect(),
        ),
    );

    schema.insert("properties".into(), Value::Object(merged_props));
    schema.insert(
        "required".into(),
        Value::Array(vec![Value::String("action".into())]),
    );
    schema.remove("oneOf");
}

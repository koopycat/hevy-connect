//! Turning a result into text: format selection, `--fields` projection,
//! explicit truncation, and error rendering.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use crate::args::Parsed;
use crate::error::{Error, Result};

const STRING_LIMIT: usize = 240;
const ARRAY_LIMIT: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Toon,
    Json,
}

/// The format to report errors in, decided from raw arguments because a
/// failure may happen before (or because) flags could be validated.
pub fn requested_format(argv: &[String]) -> Format {
    let json = argv.iter().any(|a| a == "--json" || a == "--format=json")
        || argv
            .iter()
            .rposition(|a| a == "--format")
            .is_some_and(|i| argv.get(i + 1).is_some_and(|v| v == "json"));
    if json { Format::Json } else { Format::Toon }
}

pub fn render(value: &Value, format: Format) -> String {
    match format {
        Format::Json => {
            serde_json::to_string_pretty(value).expect("a JSON value always serializes")
        }
        Format::Toon => serde_toon::to_string(value).expect("a JSON value always encodes as TOON"),
    }
}

pub fn render_error(error: &Error, format: Format) -> String {
    render(&error.to_value(), format)
}

/// The output flags shared by every command.
#[derive(Debug)]
pub struct OutputOptions {
    pub format: Format,
    pub full: bool,
    pub fields: Option<Vec<String>>,
}

impl OutputOptions {
    pub fn from_args(parsed: &Parsed) -> Result<Self> {
        let explicit = parsed.value("format");
        if explicit.is_some_and(|f| f != "toon" && f != "json") {
            return Err(Error::validation(
                "--format must be either \"toon\" or \"json\".",
            ));
        }
        if parsed.switch("json") && explicit == Some("toon") {
            return Err(Error::validation(
                "--json cannot be combined with --format toon.",
            ));
        }
        let format = if parsed.switch("json") || explicit == Some("json") {
            Format::Json
        } else {
            Format::Toon
        };
        let full = parsed.switch("full");
        let fields = parsed.value("fields").map(parse_fields).transpose()?;
        if full && fields.is_some() {
            return Err(Error::validation(
                "--full cannot be combined with --fields.",
            ));
        }
        Ok(Self {
            format,
            full,
            fields,
        })
    }

    /// Render a result whose `--fields` selection was already applied, if any.
    pub fn render(&self, value: Value) -> String {
        let value = if self.full {
            value
        } else {
            truncate_output(value)
        };
        render(&value, self.format)
    }

    /// Render a result, applying `--fields` to the whole of it first.
    pub fn render_projected(&self, value: Value) -> Result<String> {
        let value = match &self.fields {
            Some(fields) => project_fields(&value, fields, None)?,
            None => value,
        };
        Ok(self.render(value))
    }
}

fn parse_fields(text: &str) -> Result<Vec<String>> {
    let fields: Vec<&str> = text.split(',').map(str::trim).collect();
    if fields.iter().any(|f| f.is_empty()) {
        return Err(Error::validation(
            "--fields must be a comma-separated list of non-empty paths.",
        ));
    }
    let mut unique: Vec<String> = Vec::new();
    for field in fields {
        let safe = field.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        });
        if !safe {
            return Err(Error::validation(format!("Unsafe field path: {field}.")));
        }
        if !unique.iter().any(|existing| existing == field) {
            unique.push(field.to_owned());
        }
    }
    Ok(unique)
}

/// Every dotted path present anywhere in `value`, sorted.
fn field_paths(value: &Value, prefix: &str, paths: &mut BTreeSet<String>) {
    match value {
        Value::Array(items) => items
            .iter()
            .for_each(|item| field_paths(item, prefix, paths)),
        Value::Object(map) => {
            for (key, entry) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                paths.insert(path.clone());
                field_paths(entry, &path, paths);
            }
        }
        _ => {}
    }
}

fn project_path(value: &Value, segments: &[&str]) -> Value {
    let Some((head, tail)) = segments.split_first() else {
        return value.clone();
    };
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| project_path(item, segments))
                .collect(),
        ),
        Value::Object(map) => match map.get(*head) {
            None => Value::Null,
            Some(selected) => {
                let inner = if tail.is_empty() {
                    selected.clone()
                } else {
                    project_path(selected, tail)
                };
                json!({ *head: inner })
            }
        },
        _ => Value::Null,
    }
}

fn merge_projected(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Value::Array(
            left.into_iter()
                .enumerate()
                .map(|(index, entry)| {
                    merge_projected(entry, right.get(index).cloned().unwrap_or(Value::Null))
                })
                .collect(),
        ),
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, value) in right {
                match left.get_mut(&key) {
                    Some(slot) => *slot = merge_projected(std::mem::take(slot), value),
                    None => {
                        left.insert(key, value);
                    }
                }
            }
            Value::Object(left)
        }
        (_, right) => right,
    }
}

/// Keep only the dotted `fields` of `value` (of each element, for arrays).
/// `declared` names the selectable fields when they are known up front;
/// otherwise they are discovered from the value itself.
pub fn project_fields(
    value: &Value,
    fields: &[String],
    declared: Option<&[&str]>,
) -> Result<Value> {
    let available: Vec<String> = match declared {
        Some(names) => names.iter().map(|n| (*n).to_owned()).collect(),
        None => {
            let mut paths = BTreeSet::new();
            field_paths(value, "", &mut paths);
            paths.into_iter().collect()
        }
    };
    let unknown: Vec<&str> = fields
        .iter()
        .filter(|f| !available.contains(f))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let plural = if unknown.len() == 1 { "" } else { "s" };
        let suggestion = if available.is_empty() {
            "No fields are available for this output.".to_owned()
        } else {
            format!("Available fields: {}", available.join(", "))
        };
        return Err(
            Error::validation(format!("Unknown field{plural}: {}.", unknown.join(", ")))
                .with_suggestion(suggestion),
        );
    }

    let mut result = match value {
        Value::Array(items) => Value::Array(items.iter().map(|_| json!({})).collect()),
        _ => json!({}),
    };
    for field in fields {
        let segments: Vec<&str> = field.split('.').collect();
        result = merge_projected(result, project_path(value, &segments));
    }
    Ok(result)
}

#[derive(Default)]
struct Truncation {
    truncated: bool,
    omitted_items: usize,
}

fn truncate_value(value: Value, state: &mut Truncation, keep_all_items: bool) -> Value {
    match value {
        Value::String(text) if text.chars().count() > STRING_LIMIT => {
            state.truncated = true;
            let kept: String = text.chars().take(STRING_LIMIT).collect();
            Value::String(kept + "…")
        }
        Value::Array(items) => {
            let limit = if keep_all_items {
                items.len()
            } else {
                ARRAY_LIMIT
            };
            if items.len() > limit {
                state.truncated = true;
                state.omitted_items += items.len() - limit;
            }
            Value::Array(
                items
                    .into_iter()
                    .take(limit)
                    .map(|item| truncate_value(item, state, false))
                    .collect(),
            )
        }
        Value::Object(map) => {
            // A list result states its own count, so its items are never cut.
            let is_list = map.get("resultCount").is_some_and(Value::is_number)
                && map.get("results").is_some_and(Value::is_array);
            Value::Object(
                map.into_iter()
                    .map(|(key, entry)| {
                        let keep = is_list && key == "results";
                        (key, truncate_value(entry, state, keep))
                    })
                    .collect(),
            )
        }
        other => other,
    }
}

/// Cut long strings and arrays, and annotate the result so the loss is explicit.
pub fn truncate_output(value: Value) -> Value {
    let mut state = Truncation::default();
    let result = truncate_value(value, &mut state, false);
    if !state.truncated {
        return result;
    }
    let mut map = match result {
        Value::Object(map) => map,
        other => Map::from_iter([("value".to_owned(), other)]),
    };
    map.insert("truncated".into(), true.into());
    if state.omitted_items > 0 {
        map.insert("omittedItems".into(), state.omitted_items.into());
    }
    map.insert(
        "truncationHelp".into(),
        "Use --full to return untruncated wire data.".into(),
    );
    Value::Object(map)
}

/// Quote a value for a POSIX shell command shown in help output.
pub fn shell_argument(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_/:.+=-".contains(&b));
    if plain {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests;

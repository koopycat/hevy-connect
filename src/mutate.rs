//! Mutations: `create` and `update` for every writable resource.
//!
//! Safety contract: nothing is sent without `--confirm`; `--dry-run` validates
//! and previews entirely offline; POST and PUT are never retried; a
//! measurement update reads the current record and sends a complete merged
//! replacement, so it can never silently erase a field.

use serde_json::{Map, Value, json};

use crate::args::Parsed;
use crate::client::Client;
use crate::config::Environment;
use crate::error::{Error, Result};
use crate::input::read_json_object;
use crate::list::MAX_AUTO_ITEMS;
use crate::output::{OutputOptions, shell_argument};
use crate::resource::Spec;
use crate::time::validate_date;
use crate::wire::{array, object, safe_integer, string};

const MAX_DUPLICATE_SCAN_PAGES: u64 = 500;
const DUPLICATE_SCAN_PAGE_SIZE: u64 = 100;

/// The documented measurement fields, as in `PutBodyMeasurement`.
const MEASUREMENT_FIELDS: [&str; 17] = [
    "weight_kg",
    "lean_mass_kg",
    "fat_percent",
    "neck_cm",
    "shoulder_cm",
    "chest_cm",
    "left_bicep_cm",
    "right_bicep_cm",
    "left_forearm_cm",
    "right_forearm_cm",
    "abdomen",
    "waist",
    "hips",
    "left_thigh",
    "right_thigh",
    "left_calf",
    "right_calf",
];

/// Envelope, label, create requirements, additional update requirements.
/// Only workout's create fields are required by Hevy's spec; all other fields
/// are CLI policy. Hevy does not document workout/routine PUT replacement behavior.
type BodyShape = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

fn body_shape(spec: &Spec) -> Option<BodyShape> {
    match spec.name {
        "workout" => Some((
            "workout",
            "Workout",
            &["title", "start_time", "end_time", "exercises"],
            &["description", "is_private"],
        )),
        // Routine create requirements are CLI policy, not OpenAPI requirements.
        "routine" => Some((
            "routine",
            "Routine",
            &["title", "exercises"],
            &["folder_id", "notes"],
        )),
        // The exercise spec has no valid required-properties list; require
        // these locally (the duplicate-title check also needs a title).
        "exercise" => Some((
            "exercise",
            "Exercise",
            &[
                "title",
                "exercise_type",
                "equipment_category",
                "muscle_group",
                "other_muscles",
            ],
            &[],
        )),
        // The folder spec does not mark title required.
        "folder" => Some(("routine_folder", "Routine folder", &["title"], &[])),
        _ => None,
    }
}

/// The request body for a wrapped resource: the input wrapped in its envelope
/// (unless already wrapped), with documented or policy-required fields.
fn wrapped_body(
    input: Map<String, Value>,
    key: &str,
    description: &str,
    create_fields: &[&str],
    update_fields: &[&str],
    update: bool,
) -> Result<Value> {
    let inner = if input.contains_key(key) {
        if input.len() != 1 {
            return Err(Error::validation(format!(
                "The {key} envelope must not contain sibling fields."
            )));
        }
        match input.into_iter().next() {
            Some((_, Value::Object(inner))) => inner,
            _ => {
                return Err(Error::validation(format!(
                    "The {key} envelope must contain an object."
                )));
            }
        }
    } else {
        input
    };

    let missing: Vec<&str> = create_fields
        .iter()
        .chain(update_fields.iter().filter(|_| update))
        .copied()
        .filter(|field| !inner.contains_key(*field))
        .collect();
    if !missing.is_empty() {
        return Err(Error::validation(format!(
            "{description} is missing required field(s): {}.",
            missing.join(", ")
        )));
    }
    // Hevy defaults this optional field to false. Never let a workout update
    // omit or send a malformed value that could change a private workout.
    if update && key == "workout" && !inner["is_private"].is_boolean() {
        return Err(Error::validation("Workout is_private must be a boolean."));
    }
    Ok(json!({ key: inner }))
}

fn measurement_body(input: Map<String, Value>, update: bool) -> Result<Value> {
    let allowed: Vec<&str> = if update {
        MEASUREMENT_FIELDS.to_vec()
    } else {
        [&["date"][..], &MEASUREMENT_FIELDS[..]].concat()
    };
    let unknown: Vec<&str> = input
        .keys()
        .map(String::as_str)
        .filter(|field| !allowed.contains(field))
        .collect();
    if !unknown.is_empty() {
        let plural = if unknown.len() == 1 { "" } else { "s" };
        return Err(Error::validation(format!(
            "Unknown measurement field{plural}: {}.",
            unknown.join(", ")
        ))
        .with_suggestion(format!("Allowed fields: {}", allowed.join(", "))));
    }
    if update && input.is_empty() {
        return Err(Error::validation(
            "Measurement update patch must not be empty.",
        ));
    }
    for field in MEASUREMENT_FIELDS {
        if input
            .get(field)
            .is_some_and(|value| !value.is_null() && !value.is_number())
        {
            return Err(Error::validation(format!(
                "Measurement {field} must be a finite number or null."
            )));
        }
    }
    if !update {
        match input.get("date") {
            None => {
                return Err(Error::validation(
                    "Measurement is missing required field(s): date.",
                ));
            }
            Some(Value::String(date)) => {
                validate_date(date)?;
            }
            Some(_) => {
                return Err(Error::validation(
                    "Measurement date must be a YYYY-MM-DD string.",
                ));
            }
        }
    }
    Ok(Value::Object(input))
}

/// The validated request body (for measurements, the bare fields).
fn prepare_body(spec: &Spec, input: Map<String, Value>, update: bool) -> Result<Value> {
    match body_shape(spec) {
        Some((key, description, create_fields, update_fields)) => wrapped_body(
            input,
            key,
            description,
            create_fields,
            update_fields,
            update,
        ),
        None => measurement_body(input, update),
    }
}

/// The fields a body sends, for previews.
fn body_fields(spec: &Spec, body: &Value) -> Vec<String> {
    let inner = match body_shape(spec) {
        Some((key, ..)) => &body[key],
        None => body,
    };
    inner
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// `--file` plus exactly one of `--confirm` and `--dry-run`, then the input itself.
struct Submission {
    file: String,
    dry_run: bool,
    input: Map<String, Value>,
}

impl Submission {
    fn read(env: &Environment, parsed: &Parsed) -> Result<Self> {
        let file = parsed
            .value("file")
            .ok_or_else(|| {
                Error::validation("--file <path|-> is required. Inline JSON is not accepted.")
            })?
            .to_owned();
        let (dry_run, confirm) = (parsed.switch("dry-run"), parsed.switch("confirm"));
        if !dry_run && !confirm {
            return Err(Error::validation(
                "This mutation requires --confirm, or use --dry-run.",
            ));
        }
        if dry_run && confirm {
            return Err(Error::validation(
                "--dry-run cannot be combined with --confirm.",
            ));
        }
        let input = read_json_object(&file, &env.cwd)?;
        Ok(Self {
            file,
            dry_run,
            input,
        })
    }

    /// The command that would perform this mutation for real.
    fn confirm_command(&self, spec: &Spec, action: &str) -> Value {
        json!([format!(
            "hevy-axi {} {action} --file {} --confirm",
            spec.name,
            shell_argument(&self.file)
        )])
    }

    /// The fields every preview (dry run) or result starts with.
    fn outcome(&self, method: &str, path: &str, idempotent: bool) -> Vec<(&'static str, Value)> {
        let mut outcome = vec![
            if self.dry_run {
                ("dryRun", true.into())
            } else {
                ("status", "success".into())
            },
            ("method", method.into()),
            ("path", path.into()),
            ("idempotent", idempotent.into()),
        ];
        if !self.dry_run {
            outcome.push(("retried", false.into()));
        }
        outcome
    }
}

fn to_object(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Mutation metadata shared by previews and results. `full_replacement`
/// describes the CLI's required input, not a guarantee about Hevy's PUT behavior.
fn semantics(spec: &Spec, update: bool) -> Vec<(&'static str, Value)> {
    if update {
        vec![("semantics", "full_replacement".into())]
    } else if spec.name == "folder" {
        vec![
            ("insertionIndex", 0.into()),
            ("shiftsExistingFolders", true.into()),
        ]
    } else {
        Vec::new()
    }
}

/// Create any resource, or replace one that is not a measurement (those
/// update through [`update_measurement`]).
pub fn run(
    env: &Environment,
    parsed: &Parsed,
    spec: &Spec,
    update: bool,
    path: &str,
    options: &OutputOptions,
) -> Result<String> {
    let submission = Submission::read(env, parsed)?;
    let body = prepare_body(spec, submission.input.clone(), update)?;
    let creates_exercise = spec.name == "exercise" && !update;
    let mut out = submission.outcome(if update { "PUT" } else { "POST" }, path, update);
    out.extend(semantics(spec, update));

    if submission.dry_run {
        if creates_exercise {
            // Dry-run stays offline, so the title check runs only on --confirm.
            out.push((
                "duplicateCheck",
                "runs on --confirm; refuses an existing title unless --allow-duplicate".into(),
            ));
        }
        let envelope = body_shape(spec).map_or("none", |(key, ..)| key);
        out.push((
            "bodySummary",
            json!({ "envelope": envelope, "fields": body_fields(spec, &body) }),
        ));
        if options.full {
            out.push(("body", body));
        }
        let action = if update { "update <id>" } else { "create" };
        out.push(("help", submission.confirm_command(spec, action)));
        return Ok(options.render(to_object(out)));
    }

    let client = Client::configured(env)?;
    if creates_exercise {
        let allow_duplicate = parsed.switch("allow-duplicate");
        let check = check_exercise_title_unique(&client, &body, allow_duplicate)?;
        out.push(("duplicateCheck", check));
    }
    let response = if update {
        client.put(path, &body)?
    } else {
        client.post(path, &body)?
    };
    if creates_exercise {
        let id = created_exercise_id(&response);
        let next = match &id {
            Some(id) => format!("hevy-axi exercise view {}", shell_argument(id)),
            None => "hevy-axi exercise list --all --fields id,title".to_owned(),
        };
        out.push(("id", id.map_or(Value::Null, Value::from)));
        out.push(("help", json!([next])));
    }
    out.push(("result", response));
    options.render_projected(to_object(out))
}

/// The new template's ID as a string, or `None` if the response holds no
/// usable one. Hevy documents `{"id": <integer>}`, but has also answered with
/// the bare ID as text and with that JSON object inside a string.
fn created_exercise_id(response: &Value) -> Option<String> {
    match response {
        Value::Object(map) => match map.get("id")? {
            Value::String(id) => plain_id(id),
            id => safe_integer(Some(id)).map(|id| id.to_string()),
        },
        // Only unwrap text that looks like JSON: an ID such as `1e10` would
        // otherwise parse as a number.
        Value::String(text) if text.trim_start().starts_with(['{', '"']) => {
            created_exercise_id(&serde_json::from_str(text).ok()?)
        }
        Value::String(text) => plain_id(text.trim()),
        id => safe_integer(Some(id)).map(|id| id.to_string()),
    }
}

fn plain_id(id: &str) -> Option<String> {
    let plain = !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    plain.then(|| id.to_owned())
}

fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Refuse to create an exercise whose title already exists, built-in or
/// custom, unless `--allow-duplicate` is given. Every template page is read;
/// if the scan cannot finish within the safety caps it fails closed, so
/// nothing is created.
fn check_exercise_title_unique(
    client: &Client,
    body: &Value,
    allow_duplicate: bool,
) -> Result<Value> {
    let title = string(body["exercise"].get("title"))
        .ok_or_else(|| Error::validation("Exercise title must be a non-empty string."))?;
    let wanted = normalize_title(title);
    let mut matches: Vec<String> = Vec::new();
    let mut scanned = 0;
    let mut page = 1;
    let mut page_count = 1;
    while page <= page_count {
        if page > MAX_DUPLICATE_SCAN_PAGES {
            return Err(Error::validation(format!(
                "Duplicate check exceeds the safety cap of {MAX_DUPLICATE_SCAN_PAGES} pages. Nothing was created."
            )));
        }
        let query = [
            ("page", page.to_string()),
            ("pageSize", DUPLICATE_SCAN_PAGE_SIZE.to_string()),
        ];
        let response = client.get("/v1/exercise_templates", &query)?;
        let wire = object(&response, "exercise list response")?;
        let templates = array(wire.get("exercise_templates"), "exercise template array")?;
        page_count = safe_integer(wire.get("page_count"))
            .ok_or_else(|| Error::protocol("Hevy returned invalid pagination metadata."))?;
        scanned += templates.len();
        if scanned > MAX_AUTO_ITEMS {
            return Err(Error::validation(format!(
                "Duplicate check exceeds the safety cap of {MAX_AUTO_ITEMS} exercise templates. Nothing was created."
            )));
        }
        for template in templates {
            let template = object(template, "exercise template")?;
            if string(template.get("title"))
                .is_some_and(|existing| normalize_title(existing) == wanted)
            {
                matches.push(string(template.get("id")).unwrap_or("unknown").to_owned());
            }
        }
        page += 1;
    }

    if !matches.is_empty() && !allow_duplicate {
        return Err(Error::validation(format!(
            "An exercise titled \"{title}\" already exists (ID {}). Nothing was created.",
            matches.join(", ")
        ))
        .with_suggestion("Pass --allow-duplicate to create it anyway."));
    }
    Ok(
        json!({ "performed": true, "matchesFound": matches.len(), "overridden": !matches.is_empty() }),
    )
}

/// Whether two optional JSON values are the same, comparing numbers by value.
fn same_value(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

/// Safely patch a measurement: read the current record, merge the patch, and
/// replace it whole. A record carrying fields this CLI does not know is
/// refused, because replacing it would lose them.
pub fn update_measurement(
    env: &Environment,
    parsed: &Parsed,
    spec: &Spec,
    date: &str,
    options: &OutputOptions,
) -> Result<String> {
    let submission = Submission::read(env, parsed)?;
    let patch = measurement_body(submission.input.clone(), true)?;
    let patch = patch.as_object().expect("a measurement body is an object");
    let path = format!("{}/{date}", spec.path);
    let patch_fields: Vec<&str> = patch.keys().map(String::as_str).collect();

    let mut out = submission.outcome("PUT", &path, true);
    out.extend([
        ("strategy", "merge_with_current".into()),
        (
            "semantics",
            "partial_patch_merged_into_complete_replacement".into(),
        ),
        ("patchFields", json!(patch_fields)),
    ]);
    if submission.dry_run {
        out.push(("readBeforeWrite", false.into()));
        if options.full {
            out.push(("patch", Value::Object(patch.clone())));
        }
        out.push((
            "help",
            submission.confirm_command(spec, &format!("update {date}")),
        ));
        return Ok(options.render(to_object(out)));
    }

    let client = Client::configured(env)?;
    let response = client.get(&path, &[])?;
    let current = object(&response, "body measurement")?;
    let unknown: Vec<&str> = current
        .keys()
        .map(String::as_str)
        .filter(|field| *field != "date" && !MEASUREMENT_FIELDS.contains(field))
        .collect();
    if !unknown.is_empty() {
        let plural = if unknown.len() == 1 { "" } else { "s" };
        return Err(Error::protocol(format!(
            "Hevy returned unknown body measurement field{plural}: {}. Refusing a replacement that could lose data.",
            unknown.join(", ")
        )));
    }
    if current.get("date").and_then(Value::as_str) != Some(date) {
        return Err(Error::protocol(
            "Hevy returned a body measurement for an unexpected date.",
        ));
    }

    let mut merged = Map::new();
    for field in MEASUREMENT_FIELDS {
        let existing = current.get(field).filter(|value| !value.is_null());
        if existing.is_some_and(|value| !value.is_number()) {
            return Err(Error::protocol(format!(
                "Hevy returned an invalid {field} body measurement."
            )));
        }
        merged.insert(
            field.to_owned(),
            patch
                .get(field)
                .or(existing)
                .filter(|value| value.is_number())
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    let changed: Vec<&str> = patch_fields
        .iter()
        .copied()
        .filter(|field| !same_value(current.get(*field), patch.get(*field)))
        .collect();
    let response = client.put(&path, &Value::Object(merged))?;

    out.extend([
        ("changedFields", json!(changed)),
        ("readBeforeWrite", true.into()),
        ("result", response),
    ]);
    options.render_projected(to_object(out))
}

#[cfg(test)]
mod tests;

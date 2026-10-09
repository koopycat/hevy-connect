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

#[derive(Clone, Copy)]
enum Ty {
    NonEmptyString,
    String,
    Array,
    Boolean,
    Number,
    StringArray,
}

#[derive(Clone, Copy, PartialEq)]
enum Presence {
    Required,
    /// Required for a full replacement, optional when creating.
    RequiredOnUpdate,
    Optional,
}

/// One field of a request body: its type, and whether `null` is accepted.
struct Rule {
    field: &'static str,
    ty: Ty,
    nullable: bool,
    presence: Presence,
}

const fn rule(field: &'static str, ty: Ty, nullable: bool, presence: Presence) -> Rule {
    Rule {
        field,
        ty,
        nullable,
        presence,
    }
}

use Presence::{Optional, Required, RequiredOnUpdate};
use Ty::{Array, Boolean, NonEmptyString, Number, String as Text, StringArray};

/// How each resource's request body is wrapped and which fields it must have.
/// Unlisted fields pass through for Hevy to judge.
fn body_shape(spec: &Spec) -> Option<(&'static str, &'static str, &'static [Rule])> {
    const WORKOUT: [Rule; 6] = [
        rule("title", NonEmptyString, false, Required),
        rule("start_time", Text, false, Required),
        rule("end_time", Text, false, Required),
        rule("description", Text, true, RequiredOnUpdate),
        rule("exercises", Array, false, Required),
        rule("is_private", Boolean, false, Optional),
    ];
    const ROUTINE: [Rule; 4] = [
        rule("title", NonEmptyString, false, Required),
        rule("folder_id", Number, true, RequiredOnUpdate),
        rule("notes", Text, true, RequiredOnUpdate),
        rule("exercises", Array, false, Required),
    ];
    const EXERCISE: [Rule; 5] = [
        rule("title", NonEmptyString, false, Required),
        rule("exercise_type", Text, false, Required),
        rule("equipment_category", Text, false, Required),
        rule("muscle_group", Text, false, Required),
        rule("other_muscles", StringArray, false, Required),
    ];
    const FOLDER: [Rule; 1] = [rule("title", NonEmptyString, false, Required)];
    match spec.name {
        "workout" => Some(("workout", "Workout", &WORKOUT)),
        "routine" => Some(("routine", "Routine", &ROUTINE)),
        "exercise" => Some(("exercise", "Exercise", &EXERCISE)),
        "folder" => Some(("routine_folder", "Routine folder", &FOLDER)),
        _ => None,
    }
}

fn check_type(rule: &Rule, value: &Value, description: &str) -> Result<()> {
    if rule.nullable && value.is_null() {
        return Ok(());
    }
    let wrong = |expected: &str| {
        let or_null = if rule.nullable { " or null" } else { "" };
        Error::validation(format!(
            "{description} {} must be {expected}{or_null}.",
            rule.field
        ))
    };
    match rule.ty {
        NonEmptyString if value.as_str().is_none_or(|s| s.trim().is_empty()) => {
            Err(wrong("a non-empty string"))
        }
        Text if !value.is_string() => Err(wrong("a string")),
        Array if !value.is_array() => Err(wrong("an array")),
        Boolean if !value.is_boolean() => Err(wrong("a boolean")),
        Number if !value.is_number() => Err(wrong("a number")),
        StringArray => match value.as_array() {
            None => Err(wrong("an array")),
            Some(items) if items.iter().any(|item| !item.is_string()) => Err(Error::validation(
                format!("{description} {} entries must be strings.", rule.field),
            )),
            Some(_) => Ok(()),
        },
        _ => Ok(()),
    }
}

/// The request body for a wrapped resource: the input wrapped in its envelope
/// (unless already wrapped), checked against the resource's rules.
fn wrapped_body(
    input: Map<String, Value>,
    key: &str,
    description: &str,
    rules: &[Rule],
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

    let required =
        |rule: &Rule| rule.presence == Required || (rule.presence == RequiredOnUpdate && update);
    let missing: Vec<&str> = rules
        .iter()
        .filter(|r| required(r) && !inner.contains_key(r.field))
        .map(|r| r.field)
        .collect();
    if !missing.is_empty() {
        return Err(Error::validation(format!(
            "{description} is missing required field(s): {}.",
            missing.join(", ")
        )));
    }
    for rule in rules {
        if let Some(value) = inner.get(rule.field) {
            check_type(rule, value, description)?;
        }
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
        Some((key, description, rules)) => wrapped_body(input, key, description, rules, update),
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
    fn confirm_command(&self, spec: &Spec, update: bool) -> String {
        let action = if update { "update <id>" } else { "create" };
        format!(
            "hevy-axi {} {action} --file {} --confirm",
            spec.name,
            shell_argument(&self.file)
        )
    }
}

/// Semantics worth stating explicitly, shared by previews and results.
fn semantics(spec: &Spec, update: bool) -> Vec<(&'static str, Value)> {
    let mut extra = Vec::new();
    if update && !spec.is_measurement() {
        extra.push(("semantics", json!("full_replacement")));
    }
    if spec.name == "folder" && !update {
        extra.push(("insertionIndex", json!(0)));
        extra.push(("shiftsExistingFolders", json!(true)));
    }
    extra
}

fn method_name(update: bool) -> &'static str {
    if update { "PUT" } else { "POST" }
}

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
    let checks_title = spec.name == "exercise" && !update;

    if submission.dry_run {
        let mut preview = Map::new();
        preview.insert("dryRun".into(), true.into());
        preview.insert("method".into(), method_name(update).into());
        preview.insert("path".into(), path.into());
        preview.insert("idempotent".into(), update.into());
        preview.extend(
            semantics(spec, update)
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v)),
        );
        if checks_title {
            // Dry-run stays offline, so the title check runs only on --confirm.
            preview.insert(
                "duplicateCheck".into(),
                "runs on --confirm; refuses an existing title unless --allow-duplicate".into(),
            );
        }
        let envelope = body_shape(spec).map_or("none", |(key, ..)| key);
        preview.insert(
            "bodySummary".into(),
            json!({ "envelope": envelope, "fields": body_fields(spec, &body) }),
        );
        if options.full {
            preview.insert("body".into(), body);
        }
        preview.insert(
            "help".into(),
            json!([submission.confirm_command(spec, update)]),
        );
        return Ok(options.render(Value::Object(preview)));
    }

    let client = Client::configured(env)?;
    let duplicate_check = if checks_title {
        Some(check_exercise_title_unique(
            &client,
            &body,
            parsed.switch("allow-duplicate"),
        )?)
    } else {
        None
    };
    let response = if update {
        client.put(path, &body)?
    } else {
        client.post(path, &body)?
    };

    let mut result = Map::new();
    result.insert("status".into(), "success".into());
    result.insert("method".into(), method_name(update).into());
    result.insert("path".into(), path.into());
    result.insert("idempotent".into(), update.into());
    result.insert("retried".into(), false.into());
    result.extend(
        semantics(spec, update)
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v)),
    );
    if let Some(check) = duplicate_check {
        result.insert("duplicateCheck".into(), check);
    }
    result.insert("result".into(), response);
    options.render_projected(Value::Object(result))
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

    let mut result = Map::new();
    let mut put = |key: &str, value: Value| {
        result.insert(key.to_owned(), value);
    };
    if submission.dry_run {
        put("dryRun", true.into());
        put("method", "PUT".into());
        put("path", path.into());
        put("idempotent", true.into());
        put("strategy", "merge_with_current".into());
        put(
            "semantics",
            "partial_patch_merged_into_complete_replacement".into(),
        );
        put("patchFields", json!(patch_fields));
        put("readBeforeWrite", false.into());
        if options.full {
            put("patch", Value::Object(patch.clone()));
        }
        put(
            "help",
            json!([format!(
                "hevy-axi measurement update {date} --file {} --confirm",
                shell_argument(&submission.file)
            )]),
        );
        return Ok(options.render(Value::Object(result)));
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

    put("status", "success".into());
    put("method", "PUT".into());
    put("path", path.into());
    put("idempotent", true.into());
    put("retried", false.into());
    put("strategy", "merge_with_current".into());
    put(
        "semantics",
        "partial_patch_merged_into_complete_replacement".into(),
    );
    put("patchFields", json!(patch_fields));
    put("changedFields", json!(changed));
    put("readBeforeWrite", true.into());
    put("result", response);
    options.render_projected(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::find;

    fn body(resource: &str, input: Value, update: bool) -> Result<Value> {
        prepare_body(
            find(resource).unwrap(),
            input.as_object().unwrap().clone(),
            update,
        )
    }

    fn message(result: Result<Value>) -> String {
        result.unwrap_err().message.clone()
    }

    #[test]
    fn measurement_fields_match_the_committed_openapi_capture() {
        let capture: Value =
            serde_json::from_str(include_str!("../docs/hevy-openapi.json")).unwrap();
        let documented: Vec<&str> =
            capture["components"]["schemas"]["PutBodyMeasurement"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
        assert_eq!(
            documented, MEASUREMENT_FIELDS,
            "docs/hevy-openapi.json changed: update MEASUREMENT_FIELDS and review docs/hevy-api-analysis.md"
        );
    }

    #[test]
    fn bodies_are_wrapped_in_their_envelope_once() {
        let workout = json!({"title": "T", "start_time": "a", "end_time": "b", "exercises": []});
        let wrapped = body("workout", workout.clone(), false).unwrap();
        assert_eq!(wrapped, json!({"workout": workout}));
        assert_eq!(body("workout", wrapped.clone(), false).unwrap(), wrapped);
        assert_eq!(
            body("folder", json!({"title": "F"}), false).unwrap(),
            json!({"routine_folder": {"title": "F"}})
        );
        assert_eq!(
            message(body("workout", json!({"workout": {}, "other": 1}), false)),
            "The workout envelope must not contain sibling fields."
        );
        assert_eq!(
            message(body("workout", json!({"workout": []}), false)),
            "The workout envelope must contain an object."
        );
    }

    #[test]
    fn replacements_require_every_field_creations_fewer() {
        let create = json!({"title": "T", "start_time": "a", "end_time": "b", "exercises": []});
        assert!(body("workout", create.clone(), false).is_ok());
        assert_eq!(
            message(body("workout", create, true)),
            "Workout is missing required field(s): description."
        );
        assert_eq!(
            message(body(
                "routine",
                json!({"title": "R", "exercises": []}),
                true
            )),
            "Routine is missing required field(s): folder_id, notes."
        );
        assert!(
            body(
                "routine",
                json!({"title": "R", "folder_id": null, "notes": null, "exercises": []}),
                true
            )
            .is_ok()
        );
    }

    #[test]
    fn field_types_are_checked() {
        let workout = |patch: Value| {
            let mut base =
                json!({"title": "T", "start_time": "a", "end_time": "b", "exercises": []});
            base.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            body("workout", base, false)
        };
        assert_eq!(
            message(workout(json!({"title": "  "}))),
            "Workout title must be a non-empty string."
        );
        assert_eq!(
            message(workout(json!({"start_time": 1}))),
            "Workout start_time must be a string."
        );
        assert_eq!(
            message(workout(json!({"exercises": {}}))),
            "Workout exercises must be an array."
        );
        assert_eq!(
            message(workout(json!({"description": 5}))),
            "Workout description must be a string or null."
        );
        assert_eq!(
            message(workout(json!({"is_private": "yes"}))),
            "Workout is_private must be a boolean."
        );
        assert!(workout(json!({"description": null, "is_private": true})).is_ok());
        assert_eq!(
            message(body(
                "routine",
                json!({"title": "R", "exercises": [], "folder_id": "x"}),
                false
            )),
            "Routine folder_id must be a number or null."
        );
        let exercise = json!({"title": "E", "exercise_type": "t", "equipment_category": "e", "muscle_group": "m", "other_muscles": ["a", 1]});
        assert_eq!(
            message(body("exercise", exercise, false)),
            "Exercise other_muscles entries must be strings."
        );
    }

    #[test]
    fn measurement_bodies_allow_documented_fields_only() {
        assert!(
            body(
                "measurement",
                json!({"date": "2024-08-14", "weight_kg": 80.5, "waist": null}),
                false
            )
            .is_ok()
        );
        assert_eq!(
            message(body("measurement", json!({"weight_kg": 1}), false)),
            "Measurement is missing required field(s): date."
        );
        assert_eq!(
            message(body("measurement", json!({"date": 5}), false)),
            "Measurement date must be a YYYY-MM-DD string."
        );
        assert_eq!(
            message(body("measurement", json!({"date": "2024-02-30"}), false)),
            "Date must be a valid calendar date in YYYY-MM-DD format."
        );
        assert_eq!(
            message(body("measurement", json!({"weight_kg": "80"}), true)),
            "Measurement weight_kg must be a finite number or null."
        );
        assert_eq!(
            message(body("measurement", json!({}), true)),
            "Measurement update patch must not be empty."
        );
        let error = body(
            "measurement",
            json!({"date": "2024-08-14", "bogus": 1}),
            true,
        )
        .unwrap_err();
        assert_eq!(error.message, "Unknown measurement fields: date, bogus.");
        assert!(error.suggestions[0].starts_with("Allowed fields: weight_kg, "));
    }

    #[test]
    fn titles_compare_ignoring_case_and_spacing() {
        assert_eq!(
            normalize_title("  Bench   PRESS\t(Barbell) "),
            "bench press (barbell)"
        );
    }

    #[test]
    fn numbers_compare_by_value() {
        assert!(same_value(Some(&json!(80)), Some(&json!(80.0))));
        assert!(!same_value(Some(&json!(80)), Some(&json!(81))));
        assert!(!same_value(None, Some(&Value::Null)));
        assert!(same_value(None, None));
    }

    #[test]
    fn body_rules_agree_with_the_committed_openapi_capture() {
        use crate::spec::{properties, property, schema};

        // Resource, then the request schema that decides its replacement body.
        let bodies = [
            ("workout", "PostWorkoutsRequestBody"),
            ("routine", "PutRoutinesRequestBody"),
            ("exercise", "CreateCustomExerciseRequestBody"),
            ("folder", "PostRoutineFolderRequestBody"),
        ];
        for (resource, request) in bodies {
            let (envelope, _, rules) = body_shape(find(resource).unwrap()).unwrap();
            let record = property(schema(request), envelope)
                .unwrap_or_else(|| panic!("{request} has no {envelope}"));
            let documented = properties(record);
            for rule in rules {
                let field = documented
                    .get(rule.field)
                    .unwrap_or_else(|| panic!("{resource}.{} is not in {request}", rule.field));
                let types: &[&str] = match rule.ty {
                    NonEmptyString | Text => &["string", "enum"],
                    Array | StringArray => &["array"],
                    Boolean => &["boolean"],
                    Number => &["number", "integer"],
                };
                let kind = crate::spec::resolve(field)["type"].as_str().unwrap_or("");
                assert!(
                    types.contains(&kind),
                    "{resource}.{} is {kind} in {request}",
                    rule.field
                );
                assert_eq!(
                    field["nullable"].as_bool().unwrap_or(false),
                    rule.nullable,
                    "{resource}.{} nullability in {request}",
                    rule.field
                );
            }
            let required = record["required"].as_array().into_iter().flatten();
            for name in required.filter_map(Value::as_str) {
                assert!(
                    rules
                        .iter()
                        .any(|rule| rule.field == name && rule.presence != Optional),
                    "{request} requires {resource}.{name}, which the rules do not"
                );
            }
        }
    }
}

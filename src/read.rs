//! Single-record reads: `view`, `count`, `user info`, and exercise `history`.

use serde_json::{Map, Value, json};

use crate::client::Client;
use crate::compact::Kind;
use crate::error::{Error, Result};
use crate::output::{OutputOptions, project_fields};
use crate::resource::Spec;
use crate::wire::{array, object, safe_integer};

const USER_FIELDS: [&str; 6] = [
    "id",
    "username",
    "name",
    "url",
    "weight_unit",
    "distance_unit",
];
const HISTORY_LIMIT: usize = 50;

/// Apply `--fields` to a record whose selectable fields are `available`.
fn select(value: Value, options: &OutputOptions, available: &[&str]) -> Result<Value> {
    match &options.fields {
        Some(fields) => project_fields(&value, fields, Some(available)),
        None => Ok(value),
    }
}

/// The record inside a response envelope such as `{"routine": {...}}`.
fn unwrap<'a>(wire: &'a Value, key: &str) -> Result<&'a Value> {
    object(wire, &format!("{key} response"))?
        .get(key)
        .ok_or_else(|| Error::protocol(format!("Hevy omitted {key} from its response.")))
}

pub fn parse_workout_count(wire: &Value) -> Result<u64> {
    safe_integer(object(wire, "workout count response")?.get("workout_count"))
        .ok_or_else(|| Error::protocol("Hevy returned an invalid workout count."))
}

pub fn workout_count(client: &Client) -> Result<u64> {
    parse_workout_count(&client.get("/v1/workouts/count", &[])?)
}

pub fn count(client: &Client, options: &OutputOptions) -> Result<String> {
    let wire = client.get("/v1/workouts/count", &[])?;
    if options.full {
        return Ok(options.render(wire));
    }
    let compact = json!({ "workoutCount": parse_workout_count(&wire)? });
    let mut result = select(compact, options, &["workoutCount"])?;
    result["help"] = json!(["hevy-axi workout list"]);
    Ok(options.render(result))
}

/// One record: the account for `user`, otherwise a compact record of `spec`.
pub fn view(client: &Client, spec: &Spec, path: &str, options: &OutputOptions) -> Result<String> {
    let wire = client.get(path, &[])?;
    if options.full {
        return Ok(options.render(wire));
    }
    let record = match spec.view_key {
        Some(key) => unwrap(&wire, key)?,
        None => &wire,
    };
    let result = match spec.kind {
        None => json!({
            "account": select(record.clone(), options, &USER_FIELDS)?,
            "help": ["hevy-axi workout list", "hevy-axi routine list"],
        }),
        Some(kind) => {
            let compact = kind.compact(record, true)?;
            let available: Vec<&str> = object(&compact, "record")?
                .keys()
                .map(String::as_str)
                .collect();
            let selected = select(compact.clone(), options, &available)?;
            json!({ "result": selected, "help": detail_help(spec, kind, &compact) })
        }
    };
    Ok(options.render(result))
}

fn detail_help(spec: &Spec, kind: Kind, compact: &Value) -> Vec<String> {
    let key = if kind == Kind::Measurement {
        "date"
    } else {
        "id"
    };
    let mut help = vec![format!("hevy-axi {} list", spec.name)];
    match compact.get(key) {
        Some(Value::String(id)) => help.push(format!("hevy-axi {} view {id} --full", spec.name)),
        Some(Value::Number(id)) => help.push(format!("hevy-axi {} view {id} --full", spec.name)),
        _ => {}
    }
    help
}

/// An exercise's set history. It is not paginated, so the default view caps it.
pub fn history(
    client: &Client,
    spec: &Spec,
    id: &str,
    range: (Option<&str>, Option<&str>),
    options: &OutputOptions,
) -> Result<String> {
    let path = format!("/v1/exercise_history/{}", spec.id_segment(id)?);
    let query: Vec<(&str, String)> = [("start_date", range.0), ("end_date", range.1)]
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value.to_owned())))
        .collect();
    let wire = client.get(&path, &query)?;
    let entries = array(
        object(&wire, "exercise history response")?.get("exercise_history"),
        "exercise history array",
    )?;
    if options.full {
        return Ok(options.render(wire));
    }

    let compact = entries
        .iter()
        .map(|entry| Kind::History.compact(entry, false))
        .collect::<Result<Vec<_>>>()?;
    let default_fields: Vec<String> = Kind::History
        .default_fields()
        .iter()
        .map(|f| (*f).to_owned())
        .collect();
    let fields = options.fields.as_deref().unwrap_or(&default_fields);
    let selected = project_fields(
        &Value::Array(compact),
        fields,
        Some(&Kind::History.available_fields()),
    )?;
    let results: Vec<Value> = selected
        .as_array()
        .into_iter()
        .flatten()
        .take(HISTORY_LIMIT)
        .cloned()
        .collect();
    let omitted = entries.len() - results.len();

    let mut help = vec![format!("hevy-axi exercise view {id}")];
    if omitted > 0 {
        help.push(format!("hevy-axi exercise history {id} --full"));
    }
    let mut result = Map::new();
    result.insert("totalCount".into(), entries.len().into());
    result.insert("resultCount".into(), results.len().into());
    result.insert("omittedCount".into(), omitted.into());
    result.insert("truncated".into(), (omitted > 0).into());
    result.insert("results".into(), Value::Array(results));
    result.insert(
        "filters".into(),
        json!({ "start": range.0, "end": range.1 }),
    );
    result.insert("help".into(), json!(help));
    Ok(options.render(Value::Object(result)))
}

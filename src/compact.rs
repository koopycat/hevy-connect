//! Compact views: Hevy's verbose records reduced to a fixed set of camelCase
//! fields. Each view is a table of `(output name, where it comes from)`, and
//! the same table names the fields `--fields` may select.

use serde_json::{Map, Value};

use crate::error::Result;
use crate::output::project_fields;
use crate::time::parse_timestamp;
use crate::wire::{array_len, number, object, scalar, string};

type Compute = fn(&Map<String, Value>) -> Result<Value>;

enum Source {
    /// A primitive copied from this key of the wire object.
    Key(&'static str),
    Computed(Compute),
}

struct Field {
    name: &'static str,
    source: Source,
}

const fn key(name: &'static str, wire_key: &'static str) -> Field {
    Field {
        name,
        source: Source::Key(wire_key),
    }
}

const fn computed(name: &'static str, compute: Compute) -> Field {
    Field {
        name,
        source: Source::Computed(compute),
    }
}

fn build(fields: &[Field], wire: &Map<String, Value>) -> Result<Map<String, Value>> {
    fields
        .iter()
        .map(|field| {
            let value = match &field.source {
                Source::Key(wire_key) => scalar(wire.get(*wire_key)),
                Source::Computed(compute) => compute(wire)?,
            };
            Ok((field.name.to_owned(), value))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Workout,
    Routine,
    Exercise,
    Folder,
    Measurement,
    Event,
    History,
}

impl Kind {
    fn fields(self) -> &'static [Field] {
        match self {
            Self::Workout => &WORKOUT,
            Self::Routine => &ROUTINE,
            Self::Exercise => &EXERCISE,
            Self::Folder => &FOLDER,
            Self::Measurement => &MEASUREMENT,
            Self::Event => &EVENT,
            Self::History => &HISTORY,
        }
    }

    /// How a wire record of this kind is named in protocol errors.
    fn description(self) -> &'static str {
        match self {
            Self::Workout => "workout",
            Self::Routine => "routine",
            Self::Exercise => "exercise template",
            Self::Folder => "routine folder",
            Self::Measurement => "body measurement",
            Self::Event => "workout event",
            Self::History => "exercise history entry",
        }
    }

    /// Every field a list of this kind can select with `--fields`.
    pub fn available_fields(self) -> Vec<&'static str> {
        self.fields().iter().map(|field| field.name).collect()
    }

    /// The fields a list of this kind shows without `--fields`.
    pub fn default_fields(self) -> &'static [&'static str] {
        match self {
            Self::Workout => &["id", "title", "startTime", "exerciseCount"],
            Self::Exercise => &["id", "title", "primaryMuscle", "equipment"],
            Self::History => &["workoutId", "workoutStartTime", "weightKg", "reps"],
            Self::Routine => &["id", "title", "folderId", "exerciseCount"],
            Self::Folder => &["id", "index", "title"],
            Self::Measurement => &["date", "weightKg", "fatPercent", "waist"],
            Self::Event => &["type", "id", "time", "title"],
        }
    }

    /// Compact every wire record and keep the fields asked for with `--fields`,
    /// or this kind's default fields. The result is an array of rows.
    pub fn rows(self, wire: &[Value], fields: Option<&[String]>) -> Result<Value> {
        let compact = wire
            .iter()
            .map(|record| self.compact(record, false))
            .collect::<Result<Vec<_>>>()?;
        let defaults: Vec<String> = self
            .default_fields()
            .iter()
            .map(|field| (*field).to_owned())
            .collect();
        let fields = fields.unwrap_or(&defaults);
        project_fields(
            &Value::Array(compact),
            fields,
            Some(&self.available_fields()),
        )
    }

    /// Compact one wire record. `detail` adds the nested sets of a workout or routine.
    pub fn compact(self, wire: &Value, detail: bool) -> Result<Value> {
        let wire = object(wire, self.description())?;
        let mut compact = build(self.fields(), wire)?;
        if detail {
            let extra: &[Field] = match self {
                Self::Workout => &WORKOUT_DETAIL,
                Self::Routine => &ROUTINE_DETAIL,
                _ => &[],
            };
            compact.extend(build(extra, wire)?);
        }
        Ok(Value::Object(compact))
    }
}

const WORKOUT: [Field; 5] = [
    key("id", "id"),
    key("title", "title"),
    key("startTime", "start_time"),
    computed("durationMinutes", workout_minutes),
    computed(
        "exerciseCount",
        |w| Ok(array_len(w.get("exercises")).into()),
    ),
];

const WORKOUT_DETAIL: [Field; 4] = [
    key("endTime", "end_time"),
    key("description", "description"),
    computed("setCount", workout_set_count),
    computed("exercises", |w| {
        exercises(w, ("workout exercise", "workout set"), &WORKOUT_SET)
    }),
];

const WORKOUT_SET: [Field; 7] = [
    key("type", "type"),
    key("weightKg", "weight_kg"),
    key("reps", "reps"),
    key("distanceMeters", "distance_meters"),
    key("durationSeconds", "duration_seconds"),
    key("rpe", "rpe"),
    key("customMetric", "custom_metric"),
];

const ROUTINE: [Field; 4] = [
    key("id", "id"),
    key("title", "title"),
    key("folderId", "folder_id"),
    computed(
        "exerciseCount",
        |r| Ok(array_len(r.get("exercises")).into()),
    ),
];

const ROUTINE_DETAIL: [Field; 1] = [computed("exercises", |r| {
    exercises(r, ("routine exercise", "routine set"), &ROUTINE_SET)
})];

/// Planned sets flatten the rep range so each set stays one TOON table row.
const ROUTINE_SET: [Field; 8] = [
    key("type", "type"),
    key("weightKg", "weight_kg"),
    key("reps", "reps"),
    computed("repRangeStart", |s| rep_range(s, "start")),
    computed("repRangeEnd", |s| rep_range(s, "end")),
    key("distanceMeters", "distance_meters"),
    key("durationSeconds", "duration_seconds"),
    key("customMetric", "custom_metric"),
];

const EXERCISE: [Field; 6] = [
    key("id", "id"),
    key("title", "title"),
    key("type", "type"),
    key("primaryMuscle", "primary_muscle_group"),
    key("equipment", "equipment"),
    key("isCustom", "is_custom"),
];

const FOLDER: [Field; 3] = [
    key("id", "id"),
    key("index", "index"),
    key("title", "title"),
];

const MEASUREMENT: [Field; 4] = [
    key("date", "date"),
    key("weightKg", "weight_kg"),
    key("fatPercent", "fat_percent"),
    key("waist", "waist"),
];

const EVENT: [Field; 4] = [
    key("type", "type"),
    computed("id", |e| event_field(e, "id", "id")),
    computed("time", |e| event_field(e, "updated_at", "deleted_at")),
    computed("title", |e| {
        Ok(scalar(event_workout(e)?.and_then(|w| w.get("title"))))
    }),
];

const HISTORY: [Field; 10] = [
    key("workoutId", "workout_id"),
    key("workoutTitle", "workout_title"),
    key("workoutStartTime", "workout_start_time"),
    key("setType", "set_type"),
    key("weightKg", "weight_kg"),
    key("reps", "reps"),
    key("distanceMeters", "distance_meters"),
    key("durationSeconds", "duration_seconds"),
    key("rpe", "rpe"),
    key("customMetric", "custom_metric"),
];

fn workout_minutes(workout: &Map<String, Value>) -> Result<Value> {
    let instant = |name| string(workout.get(name)).and_then(parse_timestamp);
    Ok(match (instant("start_time"), instant("end_time")) {
        (Some(start), Some(end)) => {
            let minutes = (end - start).num_milliseconds() as f64 / 60_000.0;
            number(((minutes * 10.0).round() / 10.0).max(0.0))
        }
        _ => Value::Null,
    })
}

fn workout_set_count(workout: &Map<String, Value>) -> Result<Value> {
    let exercises = workout
        .get("exercises")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    Ok(exercises
        .filter_map(Value::as_object)
        .map(|e| array_len(e.get("sets")))
        .sum::<usize>()
        .into())
}

/// The exercises of a workout or routine, each with its compacted sets.
/// `what` names an exercise and a set in protocol errors.
fn exercises(
    parent: &Map<String, Value>,
    what: (&str, &str),
    set_fields: &[Field],
) -> Result<Value> {
    let Some(Value::Array(entries)) = parent.get("exercises") else {
        return Ok(Value::Array(Vec::new()));
    };
    entries
        .iter()
        .map(|entry| {
            let exercise = object(entry, what.0)?;
            let sets = match exercise.get("sets") {
                Some(Value::Array(sets)) => sets
                    .iter()
                    .map(|set| Ok(Value::Object(build(set_fields, object(set, what.1)?)?)))
                    .collect::<Result<_>>()?,
                _ => Vec::new(),
            };
            Ok(Value::Object(Map::from_iter([
                ("title".to_owned(), scalar(exercise.get("title"))),
                (
                    "exerciseTemplateId".to_owned(),
                    scalar(exercise.get("exercise_template_id")),
                ),
                (
                    "setCount".to_owned(),
                    array_len(exercise.get("sets")).into(),
                ),
                ("sets".to_owned(), Value::Array(sets)),
            ])))
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn rep_range(set: &Map<String, Value>, bound: &str) -> Result<Value> {
    match set.get("rep_range") {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(range) => Ok(scalar(object(range, "routine set rep range")?.get(bound))),
    }
}

/// An event nests its workout for updates; deletions carry only an id and time.
fn event_workout(event: &Map<String, Value>) -> Result<Option<&Map<String, Value>>> {
    event
        .get("workout")
        .map(|workout| object(workout, "event workout"))
        .transpose()
}

fn event_field(event: &Map<String, Value>, nested: &str, fallback: &str) -> Result<Value> {
    let from_workout = event_workout(event)?
        .and_then(|w| w.get(nested))
        .filter(|v| !v.is_null());
    Ok(scalar(from_workout.or_else(|| event.get(fallback))))
}

#[cfg(test)]
mod tests;

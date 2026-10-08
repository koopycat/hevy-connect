//! Compact views: Hevy's verbose records reduced to a fixed set of camelCase
//! fields. Each view is a table of `(output name, where it comes from)`, and
//! the same table names the fields `--fields` may select.

use serde_json::{Map, Value};

use crate::error::Result;
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
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn workout_list_view_counts_and_times() {
        let wire = json!({
            "id": "w1", "title": "Push", "start_time": "2024-01-01T10:00:00Z", "end_time": "2024-01-01T11:02:30Z",
            "exercises": [{"sets": [{}, {}]}, {"sets": [{}]}]
        });
        let compact = Kind::Workout.compact(&wire, false).unwrap();
        assert_eq!(
            compact,
            json!({"id": "w1", "title": "Push", "startTime": "2024-01-01T10:00:00Z", "durationMinutes": 62.5, "exerciseCount": 2})
        );
        let detail = Kind::Workout.compact(&wire, true).unwrap();
        assert_eq!(detail["setCount"], 3);
        assert_eq!(detail["exercises"][0]["setCount"], 2);
    }

    #[test]
    fn whole_minutes_print_as_integers_and_bad_times_as_null() {
        let wire =
            json!({"start_time": "2024-01-01T10:00:00Z", "end_time": "2024-01-01T11:00:00Z"});
        assert_eq!(
            serde_json::to_string(&Kind::Workout.compact(&wire, false).unwrap()["durationMinutes"])
                .unwrap(),
            "60"
        );
        let unparsable = json!({"start_time": "yesterday", "end_time": "2024-01-01T11:00:00Z"});
        assert_eq!(
            Kind::Workout.compact(&unparsable, false).unwrap()["durationMinutes"],
            Value::Null
        );
        let backwards =
            json!({"start_time": "2024-01-01T11:00:00Z", "end_time": "2024-01-01T10:00:00Z"});
        assert_eq!(
            Kind::Workout.compact(&backwards, false).unwrap()["durationMinutes"],
            0
        );
    }

    #[test]
    fn routine_sets_flatten_the_rep_range() {
        let wire = json!({"exercises": [{"title": "Squat", "sets": [{"type": "normal", "rep_range": {"start": 5, "end": 8}}, {"type": "warmup"}]}]});
        let compact = Kind::Routine.compact(&wire, true).unwrap();
        let sets = &compact["exercises"][0]["sets"];
        assert_eq!(
            (
                sets[0]["repRangeStart"].clone(),
                sets[0]["repRangeEnd"].clone()
            ),
            (json!(5), json!(8))
        );
        assert_eq!(sets[1]["repRangeStart"], Value::Null);
        let bad = json!({"exercises": [{"sets": [{"rep_range": 3}]}]});
        assert_eq!(
            Kind::Routine.compact(&bad, true).unwrap_err().message,
            "Hevy returned an invalid routine set rep range."
        );
    }

    #[test]
    fn events_read_the_workout_or_the_deletion_fields() {
        let update = json!({"type": "updated", "workout": {"id": "w", "title": "T", "updated_at": "2024-01-02T00:00:00Z"}});
        assert_eq!(
            Kind::Event.compact(&update, false).unwrap(),
            json!({"type": "updated", "id": "w", "time": "2024-01-02T00:00:00Z", "title": "T"})
        );
        let deleted = json!({"type": "deleted", "id": "w", "deleted_at": "2024-01-03T00:00:00Z"});
        assert_eq!(
            Kind::Event.compact(&deleted, false).unwrap(),
            json!({"type": "deleted", "id": "w", "time": "2024-01-03T00:00:00Z", "title": null})
        );
    }

    #[test]
    fn non_objects_are_protocol_errors() {
        let error = Kind::Exercise.compact(&json!([]), false).unwrap_err();
        assert_eq!(error.message, "Hevy returned an invalid exercise template.");
    }

    #[test]
    fn every_default_field_is_selectable() {
        for kind in [
            Kind::Workout,
            Kind::Routine,
            Kind::Exercise,
            Kind::Folder,
            Kind::Measurement,
            Kind::Event,
            Kind::History,
        ] {
            let available = kind.available_fields();
            assert!(
                kind.default_fields().iter().all(|f| available.contains(f)),
                "{kind:?}"
            );
        }
    }
}

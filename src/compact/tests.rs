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
    let wire = json!({"start_time": "2024-01-01T10:00:00Z", "end_time": "2024-01-01T11:00:00Z"});
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

#[test]
fn wire_keys_exist_in_the_committed_openapi_capture() {
    use crate::spec::{items, properties, property, schema};

    let sets = |parent: &'static Value| {
        items(property(items(property(parent, "exercises").unwrap()), "sets").unwrap())
    };
    let workout = schema("Workout");
    let routine = schema("Routine");
    let tables: [(&str, &[Field], &Value); 9] = [
        ("workout", &WORKOUT, workout),
        ("workout detail", &WORKOUT_DETAIL, workout),
        ("workout set", &WORKOUT_SET, sets(workout)),
        ("routine", &ROUTINE, routine),
        ("routine set", &ROUTINE_SET, sets(routine)),
        ("exercise", &EXERCISE, schema("ExerciseTemplate")),
        ("folder", &FOLDER, schema("RoutineFolder")),
        ("measurement", &MEASUREMENT, schema("BodyMeasurement")),
        ("history", &HISTORY, schema("ExerciseHistoryEntry")),
    ];
    for (name, fields, record) in tables {
        for field in fields {
            if let Source::Key(wire_key) = field.source {
                assert!(
                    properties(record).contains_key(wire_key),
                    "{name} reads {wire_key}, which docs/hevy-openapi.json no longer documents: review docs/hevy-api-analysis.md"
                );
            }
        }
    }
    for event in ["UpdatedWorkout", "DeletedWorkout"] {
        assert!(properties(schema(event)).contains_key("type"), "{event}");
    }
}

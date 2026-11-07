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
        serde_json::from_str(include_str!("../../docs/hevy-openapi.json")).unwrap();
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
fn updates_require_complete_intended_state_creations_fewer() {
    let create = json!({"title": "T", "start_time": "a", "end_time": "b", "exercises": []});
    assert_eq!(
        message(body("routine", json!({}), false)),
        "Routine is missing required field(s): title, exercises."
    );
    assert!(body("workout", create.clone(), false).is_ok());
    assert_eq!(
        message(body("workout", create, true)),
        "Workout is missing required field(s): description, is_private."
    );
    assert_eq!(
        message(body(
            "workout",
            json!({"title": "T", "start_time": "a", "end_time": "b", "description": null, "exercises": []}),
            true
        )),
        "Workout is missing required field(s): is_private."
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

    // Resource and request schema; only the workout has documented required fields.
    let bodies = [
        ("workout", "PostWorkoutsRequestBody"),
        ("routine", "PutRoutinesRequestBody"),
        ("exercise", "CreateCustomExerciseRequestBody"),
        ("folder", "PostRoutineFolderRequestBody"),
    ];
    for (resource, request) in bodies {
        let (envelope, _, create_fields, update_fields) =
            body_shape(find(resource).unwrap()).unwrap();
        let record = property(schema(request), envelope)
            .unwrap_or_else(|| panic!("{request} has no {envelope}"));
        let documented = properties(record);
        for field in create_fields.iter().chain(update_fields) {
            assert!(
                documented.contains_key(*field),
                "{resource}.{field} is not in {request}"
            );
        }
        let required: Vec<&str> = record["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if resource == "workout" {
            assert_eq!(required, create_fields, "workout requirements changed");
            assert_eq!(
                record["properties"]["is_private"]["default"], false,
                "review workout update privacy policy if the API default changes"
            );
        } else {
            assert!(
                required.iter().all(|field| create_fields.contains(field)),
                "{request} requires a field the CLI does not check"
            );
        }
    }
}

#[test]
fn a_created_exercise_id_is_read_from_every_observed_response_shape() {
    let uuid = "123e4567-e89b-12d3-a456-426614174000";
    let id = |response: Value| created_exercise_id(&response);
    assert_eq!(id(json!({ "id": 123 })).as_deref(), Some("123"));
    assert_eq!(id(json!({ "id": uuid })).as_deref(), Some(uuid));
    assert_eq!(id(json!(uuid)).as_deref(), Some(uuid));
    assert_eq!(id(json!(format!(" {uuid}\n"))).as_deref(), Some(uuid));
    assert_eq!(id(json!("1e10")).as_deref(), Some("1e10"));
    assert_eq!(id(json!(42)).as_deref(), Some("42"));
    assert_eq!(id(json!(r#"{"id":7}"#)).as_deref(), Some("7"));
    assert_eq!(id(json!(format!("\"{uuid}\""))).as_deref(), Some(uuid));
    for unusable in [
        json!({}),
        json!({ "id": null }),
        json!({ "id": 1.5 }),
        json!({ "id": -1 }),
        json!({ "id": "" }),
        json!({ "id": r#"{"id":7}"# }),
        json!("not an id"),
        json!("{broken"),
        json!(null),
        json!([uuid]),
    ] {
        assert_eq!(id(unusable.clone()), None, "{unusable}");
    }
}

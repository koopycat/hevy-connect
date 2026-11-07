use super::*;

#[test]
fn identifiers_are_one_safe_path_segment() {
    let workout = find("workout").unwrap();
    assert_eq!(
        workout.record_path("abc-123").unwrap(),
        "/v1/workouts/abc-123"
    );
    assert_eq!(
        workout.record_path("a/b c?d#e").unwrap(),
        "/v1/workouts/a%2Fb%20c%3Fd%23e"
    );
    assert_eq!(workout.record_path("ü").unwrap(), "/v1/workouts/%C3%BC");
    for bad in ["", "  ", ".", "..", "a\nb", "a\0b"] {
        assert_eq!(
            workout.record_path(bad).unwrap_err().message,
            "Workout ID is invalid.",
            "{bad:?}"
        );
    }
}

#[test]
fn measurements_are_addressed_by_calendar_date() {
    let measurement = find("measurement").unwrap();
    assert_eq!(
        measurement.record_path("2024-08-14").unwrap(),
        "/v1/body_measurements/2024-08-14"
    );
    assert!(measurement.record_path("2024-02-30").is_err());
    assert!(measurement.record_path("today").is_err());
}

#[test]
fn every_resource_has_actions_and_a_unique_name() {
    let mut names: Vec<_> = RESOURCES.iter().map(|spec| spec.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), RESOURCES.len());
    assert!(RESOURCES.iter().all(|spec| !spec.actions.is_empty()));
}

#[test]
fn resources_match_the_committed_openapi_capture() {
    use crate::spec::{doc, items, properties};
    use std::collections::BTreeSet;

    // Every action, as the (method, path template) it performs.
    let mut ours = BTreeSet::new();
    for spec in &RESOURCES {
        for action in spec.actions {
            let (method, path) = match action {
                Action::List | Action::Info => ("get", spec.path.to_owned()),
                Action::Count => ("get", format!("{}/count", spec.path)),
                Action::Events => ("get", format!("{}/events", spec.path)),
                Action::View => ("get", format!("{}/{{}}", spec.path)),
                Action::History => ("get", "/v1/exercise_history/{}".to_owned()),
                Action::Create => ("post", spec.path.to_owned()),
                Action::Update => ("put", format!("{}/{{}}", spec.path)),
            };
            ours.insert((method.to_owned(), path));
        }
    }
    // Path parameters are named differently per endpoint; compare their positions.
    let template = |path: &str| {
        path.split('/')
            .map(|part| if part.starts_with('{') { "{}" } else { part })
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut documented = BTreeSet::new();
    for (path, item) in doc()["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            documented.insert((method.clone(), template(path)));
        }
    }
    assert_eq!(
        ours, documented,
        "docs/hevy-openapi.json changed: update RESOURCES and review docs/hevy-api-analysis.md"
    );

    for spec in RESOURCES
        .iter()
        .filter(|s| s.actions.contains(&Action::List))
    {
        let get = &doc()["paths"][spec.path]["get"];
        let page_size = get["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parameter| parameter["name"] == "pageSize")
            .unwrap_or_else(|| panic!("{} has no pageSize", spec.name));
        assert_eq!(
            page_size["schema"]["maximum"].as_u64(),
            Some(spec.max_page_size),
            "{} max_page_size",
            spec.name
        );
        let body = &get["responses"]["200"]["content"]["application/json"]["schema"];
        assert!(
            properties(body).contains_key(spec.array_key),
            "{} array_key {:?}",
            spec.name,
            spec.array_key
        );
        // The records themselves are what `compact` reads.
        items(&properties(body)[spec.array_key]);
    }
}

//! Black-box tests of the `hevy-axi` binary: dispatch, routes, output, and
//! errors. Safety invariants live in `tests/safety.rs`.

use crate::common::{Cli, KEY, Mock, Recorded, Reply};
use serde_json::{Value, json};

/// A mock that answers every endpoint with a minimal, valid shape.
fn generic(request: &Recorded) -> Reply {
    let path = request.target.split('?').next().unwrap();
    let body = match path {
        "/v1/user/info" => json!({ "data": { "id": "u" } }),
        "/v1/workouts/count" => json!({ "workout_count": 7 }),
        "/v1/workouts"
        | "/v1/workouts/events"
        | "/v1/routines"
        | "/v1/exercise_templates"
        | "/v1/routine_folders"
        | "/v1/body_measurements"
            if request.method == "GET" =>
        {
            let key = match path {
                "/v1/workouts/events" => "events",
                other => other.trim_start_matches("/v1/"),
            };
            json!({ "page": 1, "page_count": 1, key: [] })
        }
        _ if path.starts_with("/v1/exercise_history/") => json!({ "exercise_history": [] }),
        _ if path.starts_with("/v1/routines/") => json!({ "routine": {} }),
        _ => json!({}),
    };
    Reply::json(200, &body)
}

#[test]
fn help_version_and_the_unconfigured_home_view_need_no_credentials() {
    let cli = Cli::new().without_key();
    let help = cli.run(&["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("Usage:\n  hevy-axi <command>"));
    assert!(help.stdout.contains("workout      list, count, events"));

    for flag in ["--version", "-v", "-V"] {
        let version = cli.run(&[flag]);
        assert_eq!(
            (version.stdout.as_str(), version.code),
            (concat!(env!("CARGO_PKG_VERSION"), "\n"), 0)
        );
    }

    let page = cli.run(&["workout", "--help"]);
    assert!(page.stdout.starts_with("Usage: hevy-axi workout <action>"));
    assert!(page.stdout.contains("--page-size <n>"));
    assert_eq!(
        cli.run(&["workout", "list", "--help"]).stdout,
        page.stdout,
        "--help anywhere shows the command's page"
    );

    let home = cli.run(&[]);
    assert_eq!(home.code, 0);
    assert!(
        home.stdout
            .contains("description: Agent-ergonomic access to the Hevy Public API.")
    );
    assert!(home.stdout.contains("status: not_configured"));
    assert!(
        home.stdout.find("status: not_configured") < home.stdout.find("help[2]:"),
        "content comes before guidance"
    );
    assert!(home.stdout.contains("hevy-axi setup key --confirm"));
}

#[test]
fn the_home_view_when_configured_reports_only_local_state() {
    let mock = Mock::start(generic);
    let home = Cli::against(&mock).run(&[]);
    assert!(home.stdout.contains("status: configured"));
    assert!(home.stdout.contains("credentialSource: environment"));
    assert!(!home.stdout.contains(KEY));
    assert!(
        mock.requests().is_empty(),
        "the home view makes no API call"
    );
}

#[test]
fn usage_errors_are_structured_and_exit_2() {
    let cli = Cli::new();
    let unknown = cli.run(&["does-not-exist", "--json"]);
    assert_eq!(unknown.code, 2);
    let error = &unknown.json()["error"];
    assert_eq!(error["code"], "VALIDATION_ERROR");
    assert_eq!(error["message"], "Unknown command: does-not-exist.");
    assert!(error["suggestions"][0].as_str().unwrap().contains("--help"));

    let toon = cli.run(&["does-not-exist"]);
    assert!(
        toon.stdout.starts_with(
            "error:\n  code: VALIDATION_ERROR\n  message: \"Unknown command: does-not-exist.\""
        ),
        "{}",
        toon.stdout
    );

    for (args, message) in [
        (
            vec!["--json", "user", "info"],
            "Flags must come after the command.",
        ),
        (
            vec!["workout"],
            "An action is required. Run \"hevy-axi workout --help\".",
        ),
        (vec!["workout", "bogus"], "Unknown workout action: bogus."),
        (
            vec!["workout", "view"],
            "Usage: hevy-axi workout view <id> [flags]",
        ),
        (vec!["workout", "list", "--nope"], "Unknown flag: --nope."),
        (
            vec!["workout", "list", "--format", "xml"],
            "--format must be either \"toon\" or \"json\".",
        ),
        (
            vec!["user", "info", "--full", "--fields", "id"],
            "--full cannot be combined with --fields.",
        ),
        (
            vec!["exercise", "update", "x"],
            "Unknown exercise action: update.",
        ),
    ] {
        let output = cli.run(&args);
        assert_eq!(output.code, 2, "{args:?}");
        // TOON escapes the quotes inside a message.
        assert!(
            output.stdout.contains(&message.replace('"', "\\\"")),
            "{args:?}: {}",
            output.stdout
        );
    }
}

#[test]
fn a_missing_key_is_a_validation_error_and_bad_flags_win_over_it() {
    let cli = Cli::new().without_key();
    let missing = cli.run(&["user", "info"]);
    assert_eq!(missing.code, 2);
    assert!(missing.stdout.contains("A Hevy API key is required."));
    let flags = cli.run(&["workout", "list", "--page-size", "0"]);
    assert!(
        flags
            .stdout
            .contains("--page-size must be a positive integer."),
        "{}",
        flags.stdout
    );
}

#[test]
fn every_read_operation_maps_to_its_endpoint() {
    let cases: &[(&[&str], &[&str])] = &[
        (&["user", "info"], &["GET /v1/user/info"]),
        (
            &["workout", "list"],
            &[
                "GET /v1/workouts?page=1&pageSize=10",
                "GET /v1/workouts/count",
            ],
        ),
        (&["workout", "count"], &["GET /v1/workouts/count"]),
        (
            &["workout", "events"],
            &["GET /v1/workouts/events?page=1&pageSize=10&since=1970-01-01T00%3A00%3A00Z"],
        ),
        (&["workout", "view", "a b"], &["GET /v1/workouts/a%20b"]),
        (
            &["routine", "list"],
            &["GET /v1/routines?page=1&pageSize=10"],
        ),
        (&["routine", "view", "r1"], &["GET /v1/routines/r1"]),
        (
            &["exercise", "list"],
            &["GET /v1/exercise_templates?page=1&pageSize=10"],
        ),
        (
            &["exercise", "view", "t1"],
            &["GET /v1/exercise_templates/t1"],
        ),
        (
            &["exercise", "history", "t1"],
            &["GET /v1/exercise_history/t1"],
        ),
        (
            &[
                "exercise",
                "history",
                "t1",
                "--start",
                "2024-01-01",
                "--end",
                "2024-02-01T00:00:00Z",
            ],
            &[
                "GET /v1/exercise_history/t1?start_date=2024-01-01&end_date=2024-02-01T00%3A00%3A00Z",
            ],
        ),
        (
            &["folder", "list"],
            &["GET /v1/routine_folders?page=1&pageSize=10"],
        ),
        (&["folder", "view", "42"], &["GET /v1/routine_folders/42"]),
        (
            &["measurement", "list"],
            &["GET /v1/body_measurements?page=1&pageSize=10"],
        ),
        (
            &["measurement", "view", "2024-08-14"],
            &["GET /v1/body_measurements/2024-08-14"],
        ),
    ];
    for (args, expected) in cases {
        let mock = Mock::start(generic);
        let output = Cli::against(&mock).run(args);
        assert_eq!(output.code, 0, "{args:?}: {}", output.stdout);
        assert_eq!(mock.targets(), *expected, "{args:?}");
        assert!(
            mock.requests()
                .iter()
                .all(|r| r.headers["api-key"] == KEY && r.headers["accept"] == "application/json")
        );
    }
}

#[test]
fn identifiers_that_would_change_the_endpoint_are_refused_before_any_request() {
    let mock = Mock::start(generic);
    let cli = Cli::against(&mock);
    for id in ["..", ".", "", "  "] {
        let output = cli.run(&["workout", "view", id]);
        assert_eq!(output.code, 2, "{id:?}");
        assert!(output.stdout.contains("Workout ID is invalid."));
    }
    for date in ["2024-02-30", "today"] {
        assert_eq!(cli.run(&["measurement", "view", date]).code, 2, "{date}");
    }
    for bad in ["yesterday", "2024-02-30", "2024-01-01T00:00:00"] {
        assert_eq!(
            cli.run(&["workout", "events", "--since", bad]).code,
            2,
            "{bad}"
        );
    }
    let reversed = cli.run(&[
        "exercise",
        "history",
        "t",
        "--start",
        "2024-02-01",
        "--end",
        "2024-01-01",
    ]);
    assert!(
        reversed
            .stdout
            .contains("--start must not be later than --end.")
    );
    assert!(mock.requests().is_empty());
}

#[test]
fn output_is_toon_by_default_and_json_on_request() {
    let mock = Mock::start(|_| {
        Reply::json(
            200,
            &json!({ "data": { "id": "u-1", "name": "Ada", "url": "https://x.example", "weight_unit": "kg" } }),
        )
    });
    let cli = Cli::against(&mock);
    let toon = cli.run(&["user", "info"]);
    assert_eq!(
        toon.stdout,
        "account:\n  id: u-1\n  name: Ada\n  url: \"https://x.example\"\n  weight_unit: kg\nhelp[2]: hevy-axi workout list,hevy-axi routine list\n"
    );

    let projected = cli
        .run(&["user", "info", "--fields", "name,weight_unit", "--json"])
        .json();
    assert_eq!(
        projected["account"],
        json!({ "name": "Ada", "weight_unit": "kg" })
    );
    let full = cli.run(&["user", "info", "--full", "--format=json"]).json();
    assert_eq!(full["data"]["id"], "u-1");

    let unknown = cli.run(&["user", "info", "--fields", "nope"]);
    assert_eq!(unknown.code, 2);
    assert!(
        unknown
            .stdout
            .contains("Available fields: id, username, name, url, weight_unit, distance_unit")
    );
}

#[test]
fn long_values_are_truncated_explicitly_and_full_bypasses_it() {
    let long = "x".repeat(300);
    let mock = Mock::start(move |_| Reply::json(200, &json!({ "data": { "name": long.clone() } })));
    let cli = Cli::against(&mock);
    let cut = cli.run(&["user", "info", "--json"]).json();
    assert_eq!(
        cut["account"]["name"].as_str().unwrap().chars().count(),
        241
    );
    assert_eq!(cut["truncated"], true);
    assert_eq!(
        cut["truncationHelp"],
        "Use --full to return untruncated wire data."
    );
    let full = cli.run(&["user", "info", "--full", "--json"]).json();
    assert_eq!(full["data"]["name"].as_str().unwrap().len(), 300);
    assert!(full.get("truncated").is_none());
}

fn workouts_page(page: u64, count: u64, ids: std::ops::Range<u64>) -> Value {
    let workouts: Vec<Value> = ids.map(|i| json!({ "id": format!("w{i}"), "title": format!("Workout {i}"), "start_time": "2024-01-01T10:00:00Z", "exercises": [] })).collect();
    json!({ "page": page, "page_count": count, "workouts": workouts })
}

/// 25 workouts in pages of ten, with an exact count endpoint.
fn paged_workouts(request: &Recorded) -> Reply {
    if request.target.starts_with("/v1/workouts/count") {
        return Reply::json(200, &json!({ "workout_count": 25 }));
    }
    let query: std::collections::HashMap<_, _> = request
        .target
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter_map(|p| p.split_once('='))
        .collect();
    let page: u64 = query["page"].parse().unwrap();
    let size: u64 = query["pageSize"].parse().unwrap();
    let count = 25u64.div_ceil(size);
    Reply::json(
        200,
        &workouts_page(page, count, ((page - 1) * size)..(page * size).min(25)),
    )
}

#[test]
fn a_single_page_reports_where_to_go_next_and_an_exact_total() {
    let mock = Mock::start(paged_workouts);
    let output = Cli::against(&mock).run(&["workout", "list", "--json"]);
    let list = output.json();
    assert_eq!(
        (
            list["page"].clone(),
            list["pageCount"].clone(),
            list["resultCount"].clone()
        ),
        (json!(1), json!(3), json!(10))
    );
    assert_eq!(
        (list["hasMore"].clone(), list["totalCount"].clone()),
        (json!(true), json!(25))
    );
    assert_eq!(
        list["help"],
        json!(["hevy-axi workout list --page 2 --page-size 10"])
    );
    assert_eq!(
        list["results"][0],
        json!({ "id": "w0", "title": "Workout 0", "startTime": "2024-01-01T10:00:00Z", "exerciseCount": 0 })
    );
}

#[test]
fn all_reads_every_page_and_derives_the_total_without_another_request() {
    let mock = Mock::start(paged_workouts);
    let list = Cli::against(&mock)
        .run(&["workout", "list", "--all", "--json"])
        .json();
    assert_eq!(
        (
            list["resultCount"].clone(),
            list["totalCount"].clone(),
            list["hasMore"].clone()
        ),
        (json!(25), json!(25), json!(false))
    );
    assert_eq!(
        mock.targets(),
        [
            "GET /v1/workouts?page=1&pageSize=10",
            "GET /v1/workouts?page=2&pageSize=10",
            "GET /v1/workouts?page=3&pageSize=10"
        ]
    );
    assert_eq!(
        list["help"],
        json!(["hevy-axi workout list --all", "hevy-axi workout view <id>"])
    );
}

#[test]
fn a_limit_inside_a_page_says_where_to_resume() {
    let mock = Mock::start(paged_workouts);
    let list = Cli::against(&mock)
        .run(&["workout", "list", "--all", "--limit", "12", "--json"])
        .json();
    assert_eq!(list["resultCount"], 12);
    assert_eq!(list["hasMore"], true);
    assert_eq!(list["resume"], json!({ "page": 2, "skip": 2 }));
    assert!(
        !mock.targets().iter().any(|t| t.contains("page=3")),
        "stops before a third page"
    );
    assert!(
        list.get("totalCount").is_some_and(|total| total == 25),
        "the total is still exact"
    );
    assert_eq!(
        list["help"],
        json!(["hevy-axi workout list --page 2 --page-size 10"])
    );

    let boundary = Cli::against(&Mock::start(paged_workouts))
        .run(&["workout", "list", "--all", "--limit", "10", "--json"])
        .json();
    assert!(boundary.get("resume").is_none());
    assert_eq!(
        boundary["help"],
        json!(["hevy-axi workout list --page 2 --page-size 10"])
    );
}

#[test]
fn pagination_flags_are_validated_before_any_request() {
    let mock = Mock::start(paged_workouts);
    let cli = Cli::against(&mock);
    for (args, message) in [
        (vec!["--page", "0"], "--page must be a positive integer."),
        (vec!["--page-size", "11"], "--page-size must not exceed 10."),
        (
            vec!["--all", "--page", "2"],
            "--all cannot be combined with --page.",
        ),
        (vec!["--limit", "5001"], "--limit must not exceed 5000."),
        (
            vec!["--limit", "99999999999999999999"],
            "--limit must be a safe integer.",
        ),
    ] {
        let mut full = vec!["workout", "list"];
        full.extend(args);
        let output = cli.run(&full);
        assert_eq!(output.code, 2, "{full:?}");
        assert!(
            output.stdout.contains(message),
            "{full:?}: {}",
            output.stdout
        );
    }
    assert!(mock.requests().is_empty());
}

#[test]
fn inconsistent_pagination_metadata_is_a_protocol_error() {
    for wire in [
        json!({ "page": 2, "page_count": 3, "workouts": [] }),
        json!({ "page": 1, "page_count": -1, "workouts": [] }),
        json!({ "page": 1, "workouts": [] }),
        json!({ "page": 1, "page_count": 0, "workouts": [{ "id": "x" }] }),
        json!({ "page": "1", "page_count": 1, "workouts": [] }),
    ] {
        let mock = Mock::start({
            let wire = wire.clone();
            move |_| Reply::json(200, &wire)
        });
        let output = Cli::against(&mock).run(&["workout", "list", "--json"]);
        assert_eq!(output.code, 1, "{wire}");
        assert_eq!(output.json()["error"]["code"], "PROTOCOL_ERROR", "{wire}");
    }
    let empty =
        Mock::start(|_| Reply::json(200, &json!({ "page": 1, "page_count": 0, "routines": [] })));
    let list = Cli::against(&empty)
        .run(&["routine", "list", "--json"])
        .json();
    assert_eq!(
        (
            list["empty"].clone(),
            list["resultCount"].clone(),
            list["pageCount"].clone()
        ),
        (json!(true), json!(0), json!(0))
    );
}

#[test]
fn a_list_of_pages_that_never_ends_is_capped() {
    // The server always claims there is another page.
    let mock = Mock::start(|request| {
        let page: u64 = request
            .target
            .split("page=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        Reply::json(
            200,
            &json!({ "page": page, "page_count": 100000, "routines": [] }),
        )
    });
    let output = Cli::against(&mock).run(&["routine", "list", "--all", "--json"]);
    assert_eq!(output.code, 2);
    assert!(
        output.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("safety cap of 500 pages")
    );
    assert_eq!(mock.requests().len(), 500);
}

#[test]
fn known_envelope_variations_are_tolerated() {
    // Events without `since` have arrived as plain workouts under `workouts`.
    let events = Mock::start(|_| {
        Reply::json(
            200,
            &json!({ "page": 1, "page_count": 1, "workouts": [{ "id": "w1", "title": "T", "updated_at": "2024-01-02T00:00:00Z" }, { "type": "deleted", "id": "w2", "deleted_at": "2024-01-03T00:00:00Z" }] }),
        )
    });
    let list = Cli::against(&events)
        .run(&["workout", "events", "--json"])
        .json();
    assert_eq!(
        list["results"][0],
        json!({ "type": "updated", "id": "w1", "time": "2024-01-02T00:00:00Z", "title": "T" })
    );
    assert_eq!(
        list["results"][1],
        json!({ "type": "deleted", "id": "w2", "time": "2024-01-03T00:00:00Z", "title": null })
    );

    // Routine folders have arrived under the routine key.
    let folders = Mock::start(|_| {
        Reply::json(
            200,
            &json!({ "page": 1, "page_count": 1, "routines": [{ "id": 5, "index": 0, "title": "Push" }] }),
        )
    });
    let list = Cli::against(&folders)
        .run(&["folder", "list", "--json"])
        .json();
    assert_eq!(
        list["results"],
        json!([{ "id": 5, "index": 0, "title": "Push" }])
    );
    assert_eq!(list["help"][1], "hevy-axi folder view <id>");
}

#[test]
fn workout_events_keep_a_supplied_since_in_the_continuation() {
    let mock =
        Mock::start(|_| Reply::json(200, &json!({ "page": 1, "page_count": 2, "events": [] })));
    let list = Cli::against(&mock)
        .run(&[
            "workout",
            "events",
            "--since",
            "2024-01-01T00:00:00Z",
            "--json",
        ])
        .json();
    assert_eq!(
        list["help"],
        json!(["hevy-axi workout events --since 2024-01-01T00:00:00Z --page 2 --page-size 10"])
    );
}

#[test]
fn field_selection_applies_to_lists_and_rejects_unknown_fields() {
    let mock = Mock::start(paged_workouts);
    let cli = Cli::against(&mock);
    let list = cli
        .run(&[
            "workout",
            "list",
            "--fields",
            "id,exerciseCount",
            "--page-size",
            "2",
            "--json",
        ])
        .json();
    assert_eq!(
        list["results"],
        json!([{ "id": "w0", "exerciseCount": 0 }, { "id": "w1", "exerciseCount": 0 }])
    );
    let bad = cli.run(&["workout", "list", "--fields", "weight"]);
    assert_eq!(bad.code, 2);
    assert!(
        bad.stdout
            .contains("Available fields: id, title, startTime, durationMinutes, exerciseCount")
    );
}

#[test]
fn history_is_capped_and_explains_how_to_see_all_of_it() {
    let entries: Vec<Value> = (0..120)
        .map(|i| json!({ "workout_id": format!("w{i}"), "weight_kg": 60 + i, "reps": 5 }))
        .collect();
    let mock =
        Mock::start(move |_| Reply::json(200, &json!({ "exercise_history": entries.clone() })));
    let cli = Cli::against(&mock);
    let history = cli.run(&["exercise", "history", "t1", "--json"]).json();
    assert_eq!(
        (
            history["totalCount"].clone(),
            history["resultCount"].clone(),
            history["omittedCount"].clone(),
            history["truncated"].clone()
        ),
        (json!(120), json!(50), json!(70), json!(true))
    );
    assert_eq!(history["help"][1], "hevy-axi exercise history t1 --full");
    assert_eq!(history["filters"], json!({ "start": null, "end": null }));
    let full = cli
        .run(&["exercise", "history", "t1", "--full", "--json"])
        .json();
    assert_eq!(full["exercise_history"].as_array().unwrap().len(), 120);
}

#[test]
fn views_flatten_nested_sets_into_table_rows() {
    let workout = json!({
        "id": "w1", "title": "Push", "start_time": "2024-01-01T10:00:00Z", "end_time": "2024-01-01T11:30:00Z",
        "exercises": [{ "title": "Bench", "exercise_template_id": "t", "sets": [{ "type": "normal", "weight_kg": 80, "reps": 5, "rpe": 8.5 }] }]
    });
    let mock = Mock::start(move |_| Reply::json(200, &workout));
    let output = Cli::against(&mock).run(&["workout", "view", "w1"]);
    assert!(
        output.stdout.contains("durationMinutes: 90\n"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("setCount: 1\n"));
    assert!(
        output.stdout.contains(
            "sets[1]{type,weightKg,reps,distanceMeters,durationSeconds,rpe,customMetric}:"
        ),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("normal,80,5,null,null,8.5,null"));
    assert!(output.stdout.contains("hevy-axi workout view w1 --full"));
}

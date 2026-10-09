//! The safety contract, end to end: credentials never leak or get redirected,
//! only reads are retried, mutations are gated, and local files are handled
//! defensively.

use std::ffi::OsString;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;

use crate::common::{Cli, KEY, Mock, Reply, path_exists};
use serde_json::{Value, json};

fn error_of(output: &crate::common::Output) -> Value {
    output.json()["error"].clone()
}

// ---- HTTP failures and credential handling

#[test]
fn the_key_is_sent_to_hevy_and_redacted_from_anything_echoed_back() {
    let mock = Mock::start(|request| {
        let key = request.headers["api-key"].clone();
        Reply::json(
            401,
            &json!({ "message": format!("rejected {key}"), key.clone(): key }),
        )
    });
    let output = Cli::against(&mock).run(&["user", "info", "--json"]);
    assert_eq!(output.code, 1);
    assert_eq!(mock.requests()[0].headers["api-key"], KEY);
    let error = error_of(&output);
    assert_eq!(
        (error["code"].clone(), error["details"]["status"].clone()),
        (json!("AUTH_ERROR"), json!(401))
    );
    assert!(error["details"]["body"].to_string().contains("[REDACTED]"));
    assert!(!output.stdout.contains(KEY), "{}", output.stdout);
}

#[test]
fn status_codes_map_to_stable_error_codes() {
    for (status, code) in [
        (400, "BAD_REQUEST"),
        (403, "FORBIDDEN"),
        (404, "NOT_FOUND"),
        (409, "CONFLICT"),
        (418, "API_ERROR"),
        (500, "API_ERROR"),
    ] {
        let mock = Mock::start(move |_| Reply::json(status, &json!({ "error": "  the reason " })));
        let output = Cli::against(&mock).run(&["workout", "view", "x", "--json"]);
        let error = error_of(&output);
        assert_eq!(
            (output.code, error["code"].clone()),
            (1, json!(code)),
            "{status}"
        );
        assert!(
            error["message"].as_str().unwrap().ends_with(": the reason"),
            "{status}: {error}"
        );
        assert_eq!(mock.requests().len(), 1, "{status} is not retried");
    }
}

#[test]
fn a_very_large_error_body_is_bounded_and_marked() {
    let mock = Mock::start(|_| Reply::json(500, &json!({ "error": "e".repeat(20_000) })));
    let output = Cli::against(&mock).run(&["workout", "view", "x", "--json"]);
    let details = &error_of(&output)["details"];
    assert_eq!(details["bodyTruncated"], true);
    assert!(
        output.stdout.len() < 12_000,
        "{} bytes",
        output.stdout.len()
    );
}

#[test]
fn a_redirect_is_blocked_and_never_followed_with_the_key() {
    let elsewhere = Mock::start(|_| Reply::json(200, &json!({ "data": {} })));
    let target = format!("{}/v1/user/info", elsewhere.url);
    let mock = Mock::start(move |_| Reply::text(302, "moved").with_header("Location", &target));
    let output = Cli::against(&mock).run(&["user", "info", "--json"]);
    let error = error_of(&output);
    assert_eq!(
        (output.code, error["code"].clone()),
        (1, json!("UNSAFE_REDIRECT"))
    );
    assert_eq!(error["details"], json!({ "status": 302 }));
    assert!(
        elsewhere.requests().is_empty(),
        "the redirect target received nothing"
    );
}

#[test]
fn reads_are_retried_and_a_recovered_read_succeeds() {
    let attempts = std::sync::atomic::AtomicUsize::new(0);
    let mock =
        Mock::start(
            move |_| match attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => Reply::text(429, "slow down").with_header("Retry-After", "0"),
                1 => Reply::text(503, "unavailable"),
                _ => Reply::json(200, &json!({ "data": { "id": "u" } })),
            },
        );
    let output = Cli::against(&mock).run(&["user", "info", "--json"]);
    assert_eq!(output.code, 0, "{}", output.stdout);
    assert_eq!(mock.requests().len(), 3);
}

#[test]
fn reads_give_up_after_two_retries() {
    let mock = Mock::start(|_| {
        Reply::json(429, &json!({ "error": "slow" })).with_header("Retry-After", "0")
    });
    let output = Cli::against(&mock).run(&["user", "info", "--json"]);
    assert_eq!(
        (output.code, error_of(&output)["code"].clone()),
        (1, json!("RATE_LIMITED"))
    );
    assert_eq!(mock.requests().len(), 3);
}

#[test]
fn writes_are_never_retried() {
    for status in [503, 429] {
        let mock = Mock::start(move |_| Reply::text(status, "no").with_header("Retry-After", "0"));
        let cli = Cli::against(&mock);
        cli.file(
            "w.json",
            &json!({ "title": "T", "start_time": "a", "end_time": "b", "exercises": [] })
                .to_string(),
            0o600,
        );
        let output = cli.run(&[
            "workout",
            "create",
            "--file",
            "w.json",
            "--confirm",
            "--json",
        ]);
        assert_eq!(output.code, 1);
        assert_eq!(
            mock.requests().len(),
            1,
            "POST answered {status} is sent exactly once"
        );
    }

    let mock = Mock::start(|_| Reply::text(503, "no"));
    let cli = Cli::against(&mock);
    cli.file("w.json", &json!({ "title": "T", "start_time": "a", "end_time": "b", "description": null, "exercises": [] }).to_string(), 0o600);
    let output = cli.run(&[
        "workout",
        "update",
        "w1",
        "--file",
        "w.json",
        "--confirm",
        "--json",
    ]);
    assert_eq!(output.code, 1);
    assert_eq!(
        mock.targets(),
        ["PUT /v1/workouts/w1"],
        "PUT is sent exactly once"
    );
}

#[test]
fn a_write_that_fails_in_flight_warns_that_the_outcome_may_be_unknown() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let cli = Cli::new().with_env("HEVY_API_BASE_URL", &format!("http://127.0.0.1:{port}"));
    let started = std::time::Instant::now();
    let output = cli.run_with_stdin(
        &["workout", "create", "--file", "-", "--confirm", "--json"],
        r#"{"title":"T","start_time":"a","end_time":"b","exercises":[]}"#,
    );
    let error = error_of(&output);
    assert_eq!(error["code"], "NETWORK_ERROR");
    assert!(
        error["suggestions"][0]
            .as_str()
            .unwrap()
            .contains("outcome may be unknown"),
        "{error}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(240),
        "no retry back-off was spent"
    );
}

#[test]
fn an_unreachable_server_is_a_network_error_naming_only_the_cause() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let output = Cli::new()
        .with_env("HEVY_API_BASE_URL", &format!("http://127.0.0.1:{port}"))
        .run(&["user", "info", "--json"]);
    let error = error_of(&output);
    assert_eq!(
        (output.code, error["code"].clone()),
        (1, json!("NETWORK_ERROR"))
    );
    assert_eq!(error["details"], json!({ "cause": "ECONNREFUSED" }));
    assert!(!output.stdout.contains(KEY));
}

#[test]
fn an_oversized_success_response_is_refused() {
    let mock = Mock::start(|_| Reply::json(200, &json!({ "blob": "x".repeat(11 * 1024 * 1024) })));
    let output = Cli::against(&mock).run(&["workout", "view", "x", "--json"]);
    let error = error_of(&output);
    assert_eq!(
        (output.code, error["code"].clone()),
        (1, json!("API_ERROR"))
    );
    assert_eq!(error["details"]["maximumBytes"], 10 * 1024 * 1024);
}

#[test]
fn responses_that_are_not_the_expected_json_are_protocol_errors() {
    for (body, content_type) in [
        ("<html>", "text/html"),
        ("{oops", "application/json"),
        ("[]", "application/json"),
        ("", "application/json"),
    ] {
        let mock = Mock::start(move |_| Reply {
            status: 200,
            headers: vec![("Content-Type", content_type.into())],
            body: body.to_owned(),
        });
        let output = Cli::against(&mock).run(&["workout", "view", "x", "--full", "--json"]);
        if body == "[]" {
            assert_eq!(
                output.json(),
                json!([]),
                "--full passes the wire payload through"
            );
            continue;
        }
        let output = Cli::against(&mock).run(&["workout", "view", "x", "--json"]);
        assert_eq!(error_of(&output)["code"], "PROTOCOL_ERROR", "{body:?}");
    }
}

// ---- configuration and credentials

#[test]
fn the_base_url_must_be_https_unless_it_is_local() {
    for bad in [
        "http://example.com",
        "https://user:pw@example.com",
        "https://example.com/?x=1",
    ] {
        let output = Cli::new()
            .with_env("HEVY_API_BASE_URL", bad)
            .run(&["user", "info", "--json"]);
        assert_eq!(
            (output.code, error_of(&output)["code"].clone()),
            (1, json!("CONFIG_ERROR")),
            "{bad}"
        );
    }
}

#[test]
fn a_key_from_the_environment_cannot_be_redirected_by_a_project_env_file() {
    let mock = Mock::start(|_| Reply::json(200, &json!({ "data": {} })));
    let cli = Cli::new().with_env("HEVY_API_BASE_URL", &mock.url);
    cli.file(
        ".env",
        "HEVY_API_KEY=file-key\nHEVY_API_BASE_URL=https://evil.example\n",
        0o600,
    );
    let output = cli.run(&["user", "info", "--json"]);
    assert_eq!(output.code, 0, "{}", output.stdout);
    assert_eq!(mock.requests()[0].headers["api-key"], KEY);
}

#[test]
fn credential_files_must_be_private_regular_files() {
    let cli = Cli::new().without_key();
    cli.file(".env", "HEVY_API_KEY=k\n", 0o644);
    let output = cli.run(&["setup", "status", "--json"]);
    assert_eq!(
        (output.code, error_of(&output)["code"].clone()),
        (2, json!("CONFIG_INSECURE"))
    );
    assert!(!output.stdout.contains("HEVY_API_KEY=k"));

    // An unrelated .env is ignored whatever its mode.
    cli.file(".env", "OTHER=1\n", 0o644);
    assert_eq!(
        cli.run(&["setup", "status", "--json"]).json()["configured"],
        false
    );

    // A symbolic link is refused even when it points at a private file.
    let target = cli.file("real.env", "HEVY_API_KEY=k\n", 0o600);
    std::fs::remove_file(cli.project().join(".env")).unwrap();
    std::os::unix::fs::symlink(&target, cli.project().join(".env")).unwrap();
    let output = cli.run(&["setup", "status", "--json"]);
    assert_eq!(
        (output.code, error_of(&output)["message"].clone()),
        (2, json!("The credential path must not be a symbolic link."))
    );
}

#[test]
fn credential_sources_are_reported_by_category_never_by_path_or_value() {
    let cli = Cli::new().without_key();
    assert_eq!(
        cli.run(&["setup", "status", "--json"]).json()["credentialSource"],
        Value::Null
    );
    cli.file(".env", "HEVY_API_KEY=project-secret\n", 0o600);
    let status = cli.run(&["setup", "status", "--json"]);
    assert_eq!(status.json()["credentialSource"], "project");
    assert!(
        !status.stdout.contains("project-secret")
            && !status.stdout.contains(&cli.project().display().to_string())
    );
    cli.file("named.env", "export HEVY_API_KEY='named'\n", 0o600);
    let named = cli
        .with_env("HEVY_AXI_ENV_FILE", "named.env")
        .run(&["setup", "status", "--json"]);
    assert_eq!(named.json()["credentialSource"], "explicit");
}

#[test]
fn a_stored_key_is_written_privately_read_from_stdin_only_and_removable() {
    let cli = Cli::new().without_key();
    let refused = cli.run_with_stdin(&["setup", "key"], "secret-value\n");
    assert_eq!(
        (
            refused.code,
            refused.stdout.contains("setup key requires --confirm.")
        ),
        (2, true)
    );

    let stored = cli.run_with_stdin(
        &["setup", "key", "--confirm", "--json"],
        "HEVY_API_KEY = \"secret-value\"\n",
    );
    assert_eq!(stored.code, 0, "{}", stored.stdout);
    assert_eq!(stored.json()["status"], "created");
    assert!(!stored.stdout.contains("secret-value"));
    let path = cli.home().join(".config/hevy-axi/credentials.env");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "HEVY_API_KEY=\"secret-value\"\n"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let status = cli.run(&["setup", "status", "--json"]).json();
    assert_eq!(
        (
            status["configured"].clone(),
            status["credentialSource"].clone()
        ),
        (json!(true), json!("global"))
    );
    assert_eq!(
        cli.run_with_stdin(&["setup", "key", "--confirm", "--json"], "other\n")
            .json()["status"],
        "updated"
    );
    assert_eq!(
        cli.run_with_stdin(&["setup", "key", "--confirm"], "  \n")
            .code,
        2
    );

    assert_eq!(
        cli.run(&["setup", "remove-key", "--confirm", "--json"])
            .json()["status"],
        "removed"
    );
    assert_eq!(
        cli.run(&["setup", "remove-key", "--confirm", "--json"])
            .json()["status"],
        "not_found"
    );
    assert!(!path_exists(&path));
}

#[test]
fn update_only_reports_and_never_touches_the_network() {
    let output = Cli::new()
        .without_key()
        .run(&["update", "--check", "--json"]);
    let report = output.json();
    assert_eq!(report["status"], "manual_update_required");
    assert_eq!(report["currentVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(Cli::new().run(&["update", "x"]).code, 2);
}

// ---- mutations

const WORKOUT: &str = r#"{"title":"Push","start_time":"2024-03-01T10:00:00Z","end_time":"2024-03-01T11:00:00Z","exercises":[]}"#;

#[test]
fn nothing_is_sent_without_confirm_and_a_dry_run_is_fully_offline() {
    let mock = Mock::start(|_| Reply::json(201, &json!({ "workout": [] })));
    let cli = Cli::against(&mock);
    cli.file("w.json", WORKOUT, 0o644);

    for (args, message) in [
        (
            vec!["workout", "create", "--file", "w.json"],
            "This mutation requires --confirm, or use --dry-run.",
        ),
        (
            vec![
                "workout",
                "create",
                "--file",
                "w.json",
                "--confirm",
                "--dry-run",
            ],
            "--dry-run cannot be combined with --confirm.",
        ),
        (
            vec!["workout", "create", "--confirm"],
            "--file <path|-> is required. Inline JSON is not accepted.",
        ),
    ] {
        let output = cli.run(&args);
        assert_eq!(output.code, 2, "{args:?}");
        assert!(
            output.stdout.contains(message),
            "{args:?}: {}",
            output.stdout
        );
    }

    // A dry run needs neither a key nor a reachable server.
    let offline = Cli::new()
        .without_key()
        .with_env("HEVY_API_BASE_URL", "http://127.0.0.1:1");
    offline.file("w.json", WORKOUT, 0o644);
    let preview = offline
        .run(&[
            "workout",
            "create",
            "--file",
            "w.json",
            "--dry-run",
            "--json",
        ])
        .json();
    assert_eq!(preview["dryRun"], true);
    assert_eq!(
        (
            preview["method"].clone(),
            preview["path"].clone(),
            preview["idempotent"].clone()
        ),
        (json!("POST"), json!("/v1/workouts"), json!(false))
    );
    assert_eq!(
        preview["bodySummary"],
        json!({ "envelope": "workout", "fields": ["title", "start_time", "end_time", "exercises"] })
    );
    assert!(preview.get("body").is_none());
    assert_eq!(
        preview["help"],
        json!(["hevy-axi workout create --file w.json --confirm"])
    );
    let full = offline
        .run(&[
            "workout",
            "create",
            "--file",
            "w.json",
            "--dry-run",
            "--full",
            "--json",
        ])
        .json();
    assert_eq!(full["body"]["workout"]["title"], "Push");
    assert!(mock.requests().is_empty());
}

#[test]
fn a_confirmed_create_posts_the_enveloped_body_once() {
    let mock = Mock::start(|_| Reply::json(201, &json!({ "workout": [{ "id": "new" }] })));
    let cli = Cli::against(&mock);
    let output = cli.run_with_stdin(
        &["workout", "create", "--file", "-", "--confirm", "--json"],
        WORKOUT,
    );
    assert_eq!(output.code, 0, "{}", output.stdout);
    let result = output.json();
    assert_eq!(
        (
            result["status"].clone(),
            result["method"].clone(),
            result["retried"].clone()
        ),
        (json!("success"), json!("POST"), json!(false))
    );
    assert_eq!(result["result"], json!({ "workout": [{ "id": "new" }] }));
    let request = &mock.requests()[0];
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/v1/workouts")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&request.body).unwrap(),
        json!({ "workout": serde_json::from_str::<Value>(WORKOUT).unwrap() })
    );
    assert_eq!(request.headers["content-type"], "application/json");
}

#[test]
fn an_update_is_a_full_replacement_and_says_so() {
    let mock = Mock::start(|_| Reply::json(200, &json!({ "id": "w1" })));
    let cli = Cli::against(&mock);
    let partial = cli.run_with_stdin(
        &[
            "workout",
            "update",
            "w1",
            "--file",
            "-",
            "--dry-run",
            "--json",
        ],
        WORKOUT,
    );
    assert_eq!(partial.code, 2);
    assert!(
        partial
            .stdout
            .contains("Workout is missing required field(s): description.")
    );

    let body = WORKOUT.replace("\"exercises\"", "\"description\":null,\"exercises\"");
    let preview = cli
        .run_with_stdin(
            &[
                "workout",
                "update",
                "w1",
                "--file",
                "-",
                "--dry-run",
                "--json",
            ],
            &body,
        )
        .json();
    assert_eq!(
        (
            preview["semantics"].clone(),
            preview["idempotent"].clone(),
            preview["method"].clone()
        ),
        (json!("full_replacement"), json!(true), json!("PUT"))
    );
    assert_eq!(
        preview["help"],
        json!(["hevy-axi workout update <id> --file - --confirm"])
    );
    let sent = cli
        .run_with_stdin(
            &[
                "workout",
                "update",
                "w1",
                "--file",
                "-",
                "--confirm",
                "--json",
            ],
            &body,
        )
        .json();
    assert_eq!(sent["semantics"], "full_replacement");
    assert_eq!(mock.targets(), ["PUT /v1/workouts/w1"]);
}

#[test]
fn mutation_bodies_are_checked_before_anything_is_sent() {
    let mock = Mock::start(|_| Reply::json(201, &json!({})));
    let cli = Cli::against(&mock);
    for (body, message) in [
        (
            json!({ "title": "x" }),
            "Workout is missing required field(s): start_time, end_time, exercises.",
        ),
        (
            json!({ "workout": {}, "extra": 1 }),
            "The workout envelope must not contain sibling fields.",
        ),
        (
            json!({ "title": "  ", "start_time": "a", "end_time": "b", "exercises": [] }),
            "Workout title must be a non-empty string.",
        ),
        (
            json!({ "title": "x", "start_time": "a", "end_time": "b", "exercises": {} }),
            "Workout exercises must be an array.",
        ),
        (json!([1]), "The mutation input must be a JSON object."),
    ] {
        let output = cli.run_with_stdin(
            &["workout", "create", "--file", "-", "--confirm"],
            &body.to_string(),
        );
        assert_eq!(output.code, 2, "{body}");
        assert!(output.stdout.contains(message), "{body}: {}", output.stdout);
    }
    assert_eq!(
        cli.run_with_stdin(&["workout", "create", "--file", "-", "--confirm"], "{oops")
            .code,
        2
    );
    assert!(mock.requests().is_empty());
}

#[test]
fn mutation_input_must_be_a_small_regular_file() {
    let mock = Mock::start(|_| Reply::json(201, &json!({})));
    let cli = Cli::against(&mock);
    let real = cli.file("real.json", WORKOUT, 0o644);
    std::os::unix::fs::symlink(&real, cli.project().join("link.json")).unwrap();
    std::fs::create_dir(cli.project().join("folder")).unwrap();
    cli.file(
        "big.json",
        &format!("{{\"title\":\"{}\"}}", "x".repeat(1024 * 1024)),
        0o644,
    );
    for (file, message) in [
        ("link.json", "must be a regular, non-symbolic-link file"),
        ("folder", "must be a regular, non-symbolic-link file"),
        ("big.json", "must not exceed 1048576 bytes"),
        ("missing.json", "could not be inspected"),
    ] {
        let output = cli.run(&["workout", "create", "--file", file, "--dry-run"]);
        assert_eq!(output.code, 2, "{file}");
        assert!(output.stdout.contains(message), "{file}: {}", output.stdout);
    }
    let oversized_stdin = cli.run_with_stdin(
        &["workout", "create", "--file", "-", "--dry-run"],
        &"x".repeat(1024 * 1024 + 1),
    );
    assert!(
        oversized_stdin
            .stdout
            .contains("Input must not exceed 1048576 bytes.")
    );
    assert!(mock.requests().is_empty());
}

fn templates(titles: &[(&str, &str)], page: u64, pages: u64) -> Value {
    let items: Vec<Value> = titles
        .iter()
        .map(|(id, title)| json!({ "id": id, "title": title }))
        .collect();
    json!({ "page": page, "page_count": pages, "exercise_templates": items })
}

const EXERCISE: &str = r#"{"title":"  Cable  FLY ","exercise_type":"weight_reps","equipment_category":"cable","muscle_group":"chest","other_muscles":[]}"#;

#[test]
fn creating_an_exercise_refuses_an_existing_title_on_any_page_and_sends_no_post() {
    let mock = Mock::start(|request| {
        if request.method == "POST" {
            return Reply::json(200, &json!({ "id": 1 }));
        }
        let page = if request.target.contains("page=2") {
            2
        } else {
            1
        };
        match page {
            1 => Reply::json(200, &templates(&[("a", "Squat")], 1, 2)),
            _ => Reply::json(200, &templates(&[("b", "cable fly")], 2, 2)),
        }
    });
    let cli = Cli::against(&mock);
    let output = cli.run_with_stdin(
        &["exercise", "create", "--file", "-", "--confirm"],
        EXERCISE,
    );
    assert_eq!(output.code, 2);
    assert!(
        output
            .stdout
            .contains("already exists (ID b). Nothing was created."),
        "{}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("Pass --allow-duplicate to create it anyway.")
    );
    assert_eq!(
        mock.targets(),
        [
            "GET /v1/exercise_templates?page=1&pageSize=100",
            "GET /v1/exercise_templates?page=2&pageSize=100"
        ]
    );

    let forced = cli
        .run_with_stdin(
            &[
                "exercise",
                "create",
                "--file",
                "-",
                "--confirm",
                "--allow-duplicate",
                "--json",
            ],
            EXERCISE,
        )
        .json();
    assert_eq!(
        forced["duplicateCheck"],
        json!({ "performed": true, "matchesFound": 1, "overridden": true })
    );
    assert!(
        mock.targets()
            .contains(&"POST /v1/exercise_templates".to_owned())
    );
}

#[test]
fn the_duplicate_check_fails_closed_when_it_cannot_finish() {
    let mock = Mock::start(|request| {
        if request.method == "POST" {
            return Reply::json(200, &json!({ "id": 1 }));
        }
        Reply::json(200, &json!({ "page": 1, "exercise_templates": [] }))
    });
    let output = Cli::against(&mock).run_with_stdin(
        &["exercise", "create", "--file", "-", "--confirm", "--json"],
        EXERCISE,
    );
    assert_eq!(
        (output.code, error_of(&output)["code"].clone()),
        (1, json!("PROTOCOL_ERROR"))
    );
    assert!(!mock.targets().iter().any(|t| t.starts_with("POST")));

    let clean = Mock::start(|request| {
        if request.method == "POST" {
            Reply::json(200, &json!({ "id": 9 }))
        } else {
            Reply::json(200, &templates(&[("a", "Squat")], 1, 1))
        }
    });
    let created = Cli::against(&clean)
        .run_with_stdin(
            &["exercise", "create", "--file", "-", "--confirm", "--json"],
            EXERCISE,
        )
        .json();
    assert_eq!(
        created["duplicateCheck"],
        json!({ "performed": true, "matchesFound": 0, "overridden": false })
    );
    assert_eq!(created["result"], json!({ "id": 9 }));
}

#[test]
fn a_dry_run_exercise_create_never_reads_the_template_list() {
    let mock = Mock::start(|_| Reply::json(200, &json!({})));
    let preview = Cli::against(&mock)
        .run_with_stdin(
            &["exercise", "create", "--file", "-", "--dry-run", "--json"],
            EXERCISE,
        )
        .json();
    assert!(
        preview["duplicateCheck"]
            .as_str()
            .unwrap()
            .starts_with("runs on --confirm")
    );
    assert!(mock.requests().is_empty());
}

#[test]
fn folder_creation_states_that_it_reorders_existing_folders() {
    let mock = Mock::start(|_| Reply::json(201, &json!({ "id": 1 })));
    let cli = Cli::against(&mock);
    let preview = cli
        .run_with_stdin(
            &["folder", "create", "--file", "-", "--dry-run", "--json"],
            r#"{"title":"Push"}"#,
        )
        .json();
    assert_eq!(
        (
            preview["insertionIndex"].clone(),
            preview["shiftsExistingFolders"].clone()
        ),
        (json!(0), json!(true))
    );
    assert_eq!(preview["bodySummary"]["envelope"], "routine_folder");
    cli.run_with_stdin(
        &["folder", "create", "--file", "-", "--confirm"],
        r#"{"routine_folder":{"title":"Push"}}"#,
    );
    assert_eq!(
        serde_json::from_str::<Value>(&mock.requests()[0].body).unwrap(),
        json!({ "routine_folder": { "title": "Push" } })
    );
}

fn measurement_server(current: Value) -> Mock {
    Mock::start(move |request| {
        if request.method == "GET" {
            Reply::json(200, &current)
        } else {
            Reply::json(200, &json!({}))
        }
    })
}

#[test]
fn a_measurement_patch_is_merged_into_the_current_record_before_one_full_put() {
    let mock = measurement_server(
        json!({ "date": "2024-08-14", "weight_kg": 80.5, "waist": 82, "hips": null }),
    );
    let cli = Cli::against(&mock);
    let patch = r#"{"weight_kg":81,"waist":null,"neck_cm":38}"#;
    let preview = cli
        .run_with_stdin(
            &[
                "measurement",
                "update",
                "2024-08-14",
                "--file",
                "-",
                "--dry-run",
                "--json",
            ],
            patch,
        )
        .json();
    assert_eq!(
        (
            preview["strategy"].clone(),
            preview["readBeforeWrite"].clone()
        ),
        (json!("merge_with_current"), json!(false))
    );
    assert!(mock.requests().is_empty(), "a dry run is offline");

    let result = cli
        .run_with_stdin(
            &[
                "measurement",
                "update",
                "2024-08-14",
                "--file",
                "-",
                "--confirm",
                "--json",
            ],
            patch,
        )
        .json();
    assert_eq!(
        result["changedFields"],
        json!(["weight_kg", "waist", "neck_cm"])
    );
    assert_eq!(result["readBeforeWrite"], true);
    assert_eq!(
        mock.targets(),
        [
            "GET /v1/body_measurements/2024-08-14",
            "PUT /v1/body_measurements/2024-08-14"
        ]
    );
    let put: Value = serde_json::from_str(&mock.requests()[1].body).unwrap();
    assert_eq!(
        put.as_object().unwrap().len(),
        17,
        "every documented field is sent"
    );
    assert!(put.get("date").is_none());
    assert_eq!(
        (
            put["weight_kg"].clone(),
            put["waist"].clone(),
            put["neck_cm"].clone(),
            put["hips"].clone()
        ),
        (json!(81), Value::Null, json!(38), Value::Null)
    );

    let unchanged = cli
        .run_with_stdin(
            &[
                "measurement",
                "update",
                "2024-08-14",
                "--file",
                "-",
                "--confirm",
                "--json",
            ],
            r#"{"waist":82}"#,
        )
        .json();
    assert_eq!(unchanged["changedFields"], json!([]));
    let put: Value = serde_json::from_str(&mock.requests().last().unwrap().body).unwrap();
    assert_eq!(
        (put["weight_kg"].clone(), put["waist"].clone()),
        (json!(80.5), json!(82)),
        "unmentioned fields keep their values"
    );
}

#[test]
fn a_measurement_update_refuses_to_replace_a_record_it_cannot_represent() {
    for (current, message) in [
        (
            json!({ "date": "2024-08-14", "surprise": 1 }),
            "unknown body measurement field: surprise. Refusing a replacement that could lose data.",
        ),
        (
            json!({ "date": "2024-08-15" }),
            "Hevy returned a body measurement for an unexpected date.",
        ),
        (
            json!({ "date": "2024-08-14", "waist": "wide" }),
            "Hevy returned an invalid waist body measurement.",
        ),
    ] {
        let mock = measurement_server(current);
        let output = Cli::against(&mock).run_with_stdin(
            &[
                "measurement",
                "update",
                "2024-08-14",
                "--file",
                "-",
                "--confirm",
                "--json",
            ],
            r#"{"weight_kg":1}"#,
        );
        assert_eq!(
            (output.code, error_of(&output)["code"].clone()),
            (1, json!("PROTOCOL_ERROR"))
        );
        assert!(output.stdout.contains(message), "{}", output.stdout);
        assert_eq!(mock.targets().len(), 1, "no PUT after a refused read");
    }
}

#[test]
fn measurement_bodies_reject_unknown_and_non_numeric_fields() {
    let mock = Mock::start(|_| Reply::json(200, &json!({})));
    let cli = Cli::against(&mock);
    for (args, body, message) in [
        (
            vec!["measurement", "create"],
            r#"{"date":"2024-08-14","bogus":1}"#,
            "Unknown measurement field: bogus.",
        ),
        (
            vec!["measurement", "create"],
            r#"{"weight_kg":80}"#,
            "Measurement is missing required field(s): date.",
        ),
        (
            vec!["measurement", "create"],
            r#"{"date":"2024-02-30"}"#,
            "Date must be a valid calendar date in YYYY-MM-DD format.",
        ),
        (
            vec!["measurement", "update", "2024-08-14"],
            r#"{"weight_kg":"80"}"#,
            "Measurement weight_kg must be a finite number or null.",
        ),
        (
            vec!["measurement", "update", "2024-08-14"],
            "{}",
            "Measurement update patch must not be empty.",
        ),
        (
            vec!["measurement", "update", "2024-08-14"],
            r#"{"date":"2024-08-14"}"#,
            "Unknown measurement field: date.",
        ),
    ] {
        let mut full = args.clone();
        full.extend(["--file", "-", "--dry-run"]);
        let output = cli.run_with_stdin(&full, body);
        assert_eq!(output.code, 2, "{args:?} {body}");
        assert!(
            output.stdout.contains(message),
            "{args:?} {body}: {}",
            output.stdout
        );
    }
}

#[test]
fn non_utf8_environment_and_arguments_never_panic() {
    use std::os::unix::ffi::OsStringExt;
    let cli = Cli::new().without_key();
    let bad_env = [(OsString::from("BAD"), OsString::from_vec(vec![0xff, 0xfe]))];
    for flag in ["--version", "--help"] {
        let output = cli.execute(&[OsString::from(flag)], &bad_env, "");
        assert_eq!(output.code, 0, "{flag}: {}", output.stdout);
    }
    let bad_arg = cli.execute(&[OsString::from_vec(b"workout\xff".to_vec())], &[], "");
    assert_eq!(bad_arg.code, 2);
    assert!(
        bad_arg.stdout.contains("VALIDATION_ERROR"),
        "{}",
        bad_arg.stdout
    );
}

#[test]
fn proxy_variables_in_the_environment_are_ignored() {
    let proxy = Mock::start(|_| Reply::json(200, &json!({ "data": {} })));
    let mock = Mock::start(|_| Reply::json(200, &json!({ "data": {} })));
    let cli = Cli::against(&mock)
        .with_env("HTTP_PROXY", &proxy.url)
        .with_env("ALL_PROXY", &proxy.url);
    let output = cli.run(&["user", "info", "--json"]);
    assert_eq!(output.code, 0, "{}", output.stdout);
    assert_eq!(mock.requests().len(), 1);
    assert!(
        proxy.requests().is_empty(),
        "the API key never goes through an ambient proxy"
    );
}

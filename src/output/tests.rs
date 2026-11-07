use super::*;

fn fields(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn requested_format_reads_the_raw_arguments() {
    let argv = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(requested_format(&argv(&["x", "--json"])), Format::Json);
    assert_eq!(
        requested_format(&argv(&["x", "--format=json"])),
        Format::Json
    );
    assert_eq!(
        requested_format(&argv(&["x", "--format", "json"])),
        Format::Json
    );
    assert_eq!(
        requested_format(&argv(&["x", "--format", "toon"])),
        Format::Toon
    );
    assert_eq!(requested_format(&argv(&["x"])), Format::Toon);
}

#[test]
fn fields_are_trimmed_deduplicated_and_checked() {
    assert_eq!(parse_fields(" a , b.c,a").unwrap(), ["a", "b.c"]);
    for bad in ["", "a,,b", "a..b", "a b", "a/b"] {
        assert!(parse_fields(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn projects_nested_paths_and_each_array_element() {
    let value = json!({"a": {"b": 1, "c": 2}, "d": [{"e": 1, "f": 2}, {"e": 3, "f": 4}], "g": 5});
    let picked = project_fields(&value, &fields(&["a.b", "d.e"]), None).unwrap();
    assert_eq!(picked, json!({"a": {"b": 1}, "d": [{"e": 1}, {"e": 3}]}));
    let rows = json!([{"x": 1, "y": 2}, {"x": 3, "y": 4}]);
    assert_eq!(
        project_fields(&rows, &fields(&["y"]), None).unwrap(),
        json!([{"y": 2}, {"y": 4}])
    );
}

#[test]
fn unknown_fields_list_the_choices() {
    let error = project_fields(
        &json!({"b": 1, "a": {"c": 2}}),
        &fields(&["zz", "a.q"]),
        None,
    )
    .unwrap_err();
    assert_eq!(error.message, "Unknown fields: zz, a.q.");
    assert_eq!(error.suggestions, ["Available fields: a, a.c, b"]);
    let error = project_fields(&json!({}), &fields(&["x"]), None).unwrap_err();
    assert_eq!(
        error.suggestions,
        ["No fields are available for this output."]
    );
    let error = project_fields(&json!([]), &fields(&["x"]), Some(&["id", "title"])).unwrap_err();
    assert_eq!(error.suggestions, ["Available fields: id, title"]);
}

#[test]
fn truncation_marks_what_it_cut() {
    let long = "x".repeat(300);
    let cut = truncate_output(json!({"note": long, "items": (0..60).collect::<Vec<_>>()}));
    assert_eq!(
        cut["note"].as_str().unwrap().chars().count(),
        STRING_LIMIT + 1
    );
    assert_eq!(cut["items"].as_array().unwrap().len(), ARRAY_LIMIT);
    assert_eq!(cut["truncated"], true);
    assert_eq!(cut["omittedItems"], 10);
    assert_eq!(
        cut["truncationHelp"],
        "Use --full to return untruncated wire data."
    );
    assert_eq!(truncate_output(json!({"a": 1})), json!({"a": 1}));
    assert_eq!(truncate_output(json!("y".repeat(241)))["truncated"], true);
}

#[test]
fn list_results_keep_every_item_but_their_contents_are_still_cut() {
    let value =
        json!({"resultCount": 60, "results": (0..60).map(|_| "z".repeat(300)).collect::<Vec<_>>()});
    let cut = truncate_output(value);
    assert_eq!(cut["results"].as_array().unwrap().len(), 60);
    assert_eq!(cut["truncated"], true);
    assert!(cut.get("omittedItems").is_none());
}

#[test]
fn shell_arguments_are_quoted_only_when_needed() {
    assert_eq!(
        shell_argument("2024-01-01T00:00:00Z"),
        "2024-01-01T00:00:00Z"
    );
    assert_eq!(shell_argument("a b"), "'a b'");
    assert_eq!(shell_argument("it's"), "'it'\\''s'");
    assert_eq!(shell_argument(""), "''");
}

/// The official TOON encoder fixtures that use default options, which are
/// the only options the CLI uses. Each entry is `(file, source)`.
const TOON_FIXTURES: [(&str, &str); 9] = [
    (
        "arrays-nested",
        include_str!("../../tests/fixtures/toon/arrays-nested.json"),
    ),
    (
        "arrays-objects",
        include_str!("../../tests/fixtures/toon/arrays-objects.json"),
    ),
    (
        "arrays-primitive",
        include_str!("../../tests/fixtures/toon/arrays-primitive.json"),
    ),
    (
        "arrays-tabular",
        include_str!("../../tests/fixtures/toon/arrays-tabular.json"),
    ),
    (
        "delimiters",
        include_str!("../../tests/fixtures/toon/delimiters.json"),
    ),
    (
        "objects-keyed",
        include_str!("../../tests/fixtures/toon/objects-keyed.json"),
    ),
    (
        "objects",
        include_str!("../../tests/fixtures/toon/objects.json"),
    ),
    (
        "primitives",
        include_str!("../../tests/fixtures/toon/primitives.json"),
    ),
    (
        "whitespace",
        include_str!("../../tests/fixtures/toon/whitespace.json"),
    ),
];

#[test]
fn toon_output_conforms_to_the_official_encoder_fixtures() {
    // `serde_toon` quotes a string that starts with U+FEFF, where the
    // specification leaves it bare. Quoting more than required still decodes
    // to the same value. If a `serde_toon` upgrade fixes this, the test
    // below fails and this exception should be deleted.
    const OVER_QUOTED: &str = "keeps strings that only resemble numbers or literals, a leading byte-order mark, or DEL unquoted";
    let (mut checked, mut over_quoted) = (0, 0);
    for (file, source) in TOON_FIXTURES {
        let fixture: Value = serde_json::from_str(source).unwrap();
        for case in fixture["tests"].as_array().unwrap() {
            let non_default = case
                .get("options")
                .and_then(Value::as_object)
                .is_some_and(|o| !o.is_empty());
            if non_default || case.get("shouldError").is_some() {
                continue;
            }
            let name = case["name"].as_str().unwrap();
            let got = render(&case["input"], Format::Toon);
            if name == OVER_QUOTED {
                over_quoted += 1;
                assert_ne!(
                    got,
                    case["expected"].as_str().unwrap(),
                    "the known deviation is gone: remove OVER_QUOTED"
                );
                assert_eq!(got, "a: .5\nb: NaN\nc: True\nd: \"\u{feff}x\"\ne: a\u{7f}b");
            } else {
                assert_eq!(got, case["expected"].as_str().unwrap(), "{file}: {name}");
            }
            checked += 1;
        }
    }
    assert_eq!(
        (checked, over_quoted),
        (138, 1),
        "fixture set changed: re-check conformance"
    );
}

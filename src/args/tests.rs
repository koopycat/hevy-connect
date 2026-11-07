use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn parses_switches_values_inline_values_and_positionals() {
    let parsed = parse(
        &args(&["one", "--json", "--page", "2", "--limit=7", "--", "--two"]),
        &[OUTPUT, PAGINATION],
    )
    .unwrap();
    assert_eq!(parsed.positionals, ["one", "--two"]);
    assert!(parsed.switch("json"));
    assert_eq!(parsed.value("page"), Some("2"));
    assert_eq!(parsed.value("limit"), Some("7"));
}

#[test]
fn accepts_a_bare_dash_as_a_value() {
    let parsed = parse(&args(&["--file", "-"]), &[MUTATION]).unwrap();
    assert_eq!(parsed.value("file"), Some("-"));
}

#[test]
fn rejects_misuse() {
    let cases: &[(&[&str], &str)] = &[
        (&["--nope"], "Unknown flag: --nope."),
        (&["-x"], "Unknown flag: -x."),
        (&["--page", "1"], "Unknown flag: --page."),
        (&["--json=1"], "--json does not take a value."),
        (&["--format"], "--format requires a value."),
        (&["--format", "--json"], "--format requires a value."),
        (&["--format="], "--format requires a non-empty value."),
        (&["--json", "--json"], "--json may only be specified once."),
    ];
    for (input, message) in cases {
        let error = parse(&args(input), &[OUTPUT]).unwrap_err();
        assert_eq!(error.message, *message, "{input:?}");
    }
}

#[test]
fn validates_positive_integers() {
    for (text, ok) in [
        ("1", true),
        ("42", true),
        ("0", false),
        ("01", false),
        ("-1", false),
        ("1.5", false),
    ] {
        let parsed = parse(&args(&[&format!("--page={text}")]), &[PAGINATION]).unwrap();
        assert_eq!(parsed.positive_integer("page").is_ok(), ok, "{text}");
    }
    let parsed = parse(&args(&["--page", "9007199254740992"]), &[PAGINATION]).unwrap();
    assert_eq!(
        parsed.positive_integer("page").unwrap_err().message,
        "--page must be a safe integer."
    );
    assert_eq!(
        parse(&[], &[PAGINATION])
            .unwrap()
            .positive_integer("page")
            .unwrap(),
        None
    );
}

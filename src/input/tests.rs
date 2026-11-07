use std::fs;

use super::*;

fn message(result: Result<Map<String, Value>>) -> String {
    result.unwrap_err().message.clone()
}

#[test]
fn reads_a_json_object_and_rejects_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        fs::write(dir.path().join(name), text).unwrap();
    };
    write("ok.json", r#"{"a": 1}"#);
    write("array.json", "[1]");
    write("bad.json", "{oops");
    write(
        "big.json",
        &format!("{{\"a\":\"{}\"}}", "x".repeat(MAX_INPUT_BYTES)),
    );

    assert_eq!(read_json_object("ok.json", dir.path()).unwrap()["a"], 1);
    assert_eq!(
        message(read_json_object("array.json", dir.path())),
        "The mutation input must be a JSON object."
    );
    assert_eq!(
        message(read_json_object("bad.json", dir.path())),
        "The mutation input must contain valid JSON."
    );
    assert_eq!(
        message(read_json_object("big.json", dir.path())),
        format!("The mutation input must not exceed {MAX_INPUT_BYTES} bytes.")
    );
    assert_eq!(
        message(read_json_object("missing.json", dir.path())),
        "The mutation input file could not be inspected."
    );
}

#[test]
fn symlinks_and_non_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("real.json"), "{}").unwrap();
    std::os::unix::fs::symlink(dir.path().join("real.json"), dir.path().join("link.json")).unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();
    let regular = "The mutation input must be a regular, non-symbolic-link file.";
    assert_eq!(message(read_json_object("link.json", dir.path())), regular);
    assert_eq!(message(read_json_object("folder", dir.path())), regular);
}

#[test]
fn invalid_utf8_is_refused_rather_than_replaced() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("latin1.json"), b"{\"title\":\"caf\xe9\"}").unwrap();
    assert_eq!(
        message(read_json_object("latin1.json", dir.path())),
        "The mutation input must be valid UTF-8."
    );
}

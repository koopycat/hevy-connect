//! Reading JSON mutation input from a file or standard input, safely and bounded.

use std::io;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::fsutil::{is_symlink_error, open_no_follow, read_capped};

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

fn too_large() -> Error {
    Error::validation(format!("Input must not exceed {MAX_INPUT_BYTES} bytes."))
}

pub fn read_stdin() -> Result<String> {
    let (bytes, more) = read_capped(io::stdin().lock(), MAX_INPUT_BYTES)
        .map_err(|_| Error::validation("Standard input could not be read."))?;
    if more {
        return Err(too_large());
    }
    utf8(bytes)
}

/// Mutation input is sent to Hevy, so invalid bytes must fail instead of
/// being silently replaced.
fn utf8(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes)
        .map_err(|_| Error::validation("The mutation input must be valid UTF-8."))
}

fn read_file(path: &Path) -> Result<String> {
    let not_regular =
        || Error::validation("The mutation input must be a regular, non-symbolic-link file.");
    let file = open_no_follow(path).map_err(|error| match error {
        _ if is_symlink_error(&error) => not_regular(),
        _ if error.kind() == io::ErrorKind::NotFound => {
            Error::validation("The mutation input file could not be inspected.")
        }
        _ => Error::validation("The mutation input file could not be opened safely."),
    })?;
    let metadata = file
        .metadata()
        .map_err(|_| Error::validation("The mutation input file could not be inspected."))?;
    if !metadata.is_file() {
        return Err(not_regular());
    }
    if metadata.len() > MAX_INPUT_BYTES as u64 {
        return Err(Error::validation(format!(
            "The mutation input must not exceed {MAX_INPUT_BYTES} bytes."
        )));
    }
    let (bytes, more) = read_capped(file, MAX_INPUT_BYTES)
        .map_err(|_| Error::validation("The mutation input file could not be read."))?;
    if more {
        return Err(too_large());
    }
    utf8(bytes)
}

/// The JSON object in `source`: a path (relative to `cwd`) or `-` for stdin.
pub fn read_json_object(source: &str, cwd: &Path) -> Result<Map<String, Value>> {
    let text = if source == "-" {
        read_stdin()?
    } else {
        read_file(&cwd.join(source))?
    };
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Error::validation(
            "The mutation input must be a JSON object.",
        )),
        Err(_) => Err(Error::validation(
            "The mutation input must contain valid JSON.",
        )),
    }
}

#[cfg(test)]
mod tests {
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
        std::os::unix::fs::symlink(dir.path().join("real.json"), dir.path().join("link.json"))
            .unwrap();
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
}

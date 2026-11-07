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
mod tests;

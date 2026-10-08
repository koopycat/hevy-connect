//! Refreshes `docs/hevy-openapi.json` from Hevy's public Swagger UI.
//!
//! The spec is embedded in `swagger-ui-init.js` as the `swaggerDoc` object.
//! This tool copies that object byte-for-byte, so the committed file stays a
//! verbatim capture. Only the public script is requested; no API key is sent.
//!
//! ```text
//! cargo run --example api-sync            write the capture if it changed
//! cargo run --example api-sync -- --check report drift; exit 1 if changed, write nothing
//! ```
//!
//! This is deliberately not part of `just check`: it needs the network, and a
//! change in Hevy's contract should be reviewed, not absorbed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

const SOURCE_URL: &str = "https://api.hevyapp.com/docs/swagger-ui-init.js";
const MARKER: &str = "\"swaggerDoc\": ";
// The live script is about 90 KB today. The cap stops a runaway response from
// exhausting memory before the job timeout would.
const MAX_SCRIPT_BYTES: u64 = 4 * 1024 * 1024;

fn capture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/hevy-openapi.json")
}

/// Index of the brace closing the object that opens at `open`, skipping strings.
fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (index, byte) in source.bytes().enumerate().skip(open) {
        match (in_string, byte) {
            (true, _) if escaped => escaped = false,
            (true, b'\\') => escaped = true,
            (true, b'"') => in_string = false,
            (true, _) => {}
            (false, b'"') => in_string = true,
            (false, b'{') => depth += 1,
            (false, b'}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            (false, _) => {}
        }
    }
    None
}

/// The `swaggerDoc` object text exactly as the script embeds it.
fn extract_swagger_doc(script: &str) -> Result<&str, String> {
    let first = script
        .find(MARKER)
        .ok_or("swaggerDoc not found in the Swagger UI init script.")?;
    if script[first + 1..].contains(MARKER) {
        return Err("swaggerDoc appears more than once; refusing to guess.".into());
    }
    let start = first + MARKER.len();
    if !script[start..].starts_with('{') {
        return Err("swaggerDoc is not an object literal.".into());
    }
    let end = matching_brace(script, start).ok_or("Unbalanced braces in swaggerDoc object.")?;
    let text = &script[start..=end];
    let spec: Value =
        serde_json::from_str(text).map_err(|error| format!("swaggerDoc is not JSON: {error}"))?;
    if !spec["openapi"].is_string() || !spec["paths"].is_object() {
        return Err("swaggerDoc is not an OpenAPI document.".into());
    }
    Ok(text)
}

/// Human-readable lines for how the contract changed: `+` added, `-` removed,
/// `~` changed, for both paths and schemas.
fn describe_changes(committed: &str, live: &str) -> Result<Vec<String>, serde_json::Error> {
    let (before, after): (Value, Value) = (
        serde_json::from_str(committed)?,
        serde_json::from_str(live)?,
    );
    let mut lines = Vec::new();
    for (kind, pointer) in [("path", "/paths"), ("schema", "/components/schemas")] {
        let empty = serde_json::Map::new();
        let old = before
            .pointer(pointer)
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let new = after
            .pointer(pointer)
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        lines.extend(
            new.keys()
                .filter(|key| !old.contains_key(*key))
                .map(|key| format!("+ {kind} {key}")),
        );
        for (key, value) in old {
            match new.get(key) {
                None => lines.push(format!("- {kind} {key}")),
                Some(other) if other != value => lines.push(format!("~ {kind} {key}")),
                Some(_) => {}
            }
        }
    }
    if lines.is_empty() {
        lines.push("Only formatting or ordering changed.".into());
    }
    Ok(lines)
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fetch_live_script() -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .max_redirects(0)
        .max_redirects_will_error(true)
        .build()
        .into();
    let mut response = agent
        .get(SOURCE_URL)
        .call()
        .map_err(|error| format!("Fetching {SOURCE_URL} failed: {error}."))?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_SCRIPT_BYTES)
        .read_to_string()
        .map_err(|error| format!("Reading {SOURCE_URL} failed: {error}."))
}

fn run(check_only: bool) -> Result<bool, String> {
    let script = fetch_live_script()?;
    let live = extract_swagger_doc(&script)?;
    let path = capture_path();
    let committed = std::fs::read_to_string(&path)
        .map_err(|error| format!("Reading {}: {error}.", path.display()))?;

    if live == committed {
        println!(
            "docs/hevy-openapi.json matches the live docs (sha256 {}).",
            sha256(live)
        );
        return Ok(true);
    }
    println!(
        "Live contract differs from the committed capture (sha256 {}):",
        sha256(live)
    );
    for line in describe_changes(&committed, live).map_err(|error| error.to_string())? {
        println!("  {line}");
    }
    if check_only {
        eprintln!(
            "Run `just api-sync` to adopt the change, then review docs/hevy-api-analysis.md."
        );
        return Ok(false);
    }
    std::fs::write(&path, live).map_err(|error| format!("Writing {}: {error}.", path.display()))?;
    println!("Wrote docs/hevy-openapi.json.");
    println!(
        "Next: update the SHA and capture date in docs/hevy-api-analysis.md and AGENTS.md, then run `just check`."
    );
    Ok(true)
}

fn main() -> ExitCode {
    let check_only = std::env::args().any(|arg| arg == "--check");
    match run(check_only) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn committed() -> String {
        std::fs::read_to_string(capture_path()).unwrap()
    }

    fn embed(spec: &str) -> String {
        format!(
            "window.onload = function() {{\n  var options = {{\n  \"swaggerDoc\": {spec},\n  \"dom_id\": \"#swagger-ui\" }};\n}};"
        )
    }

    #[test]
    fn extracts_the_committed_capture_byte_for_byte() {
        let spec = committed();
        let script = embed(&spec);
        assert_eq!(extract_swagger_doc(&script).unwrap(), spec);
    }

    #[test]
    fn does_not_stop_at_braces_or_quotes_inside_string_values() {
        let spec = serde_json::json!({ "openapi": "3.0.0", "info": { "description": "a } brace { and \"quotes\" \\ backslash" }, "paths": {} }).to_string();
        assert_eq!(extract_swagger_doc(&embed(&spec)).unwrap(), spec);
    }

    #[test]
    fn refuses_anything_it_cannot_be_sure_of() {
        let spec = committed();
        assert!(
            extract_swagger_doc("var nothing = 1;")
                .unwrap_err()
                .contains("not found")
        );
        assert!(
            extract_swagger_doc(&(embed(&spec) + &embed(&spec)))
                .unwrap_err()
                .contains("more than once")
        );
        assert!(
            extract_swagger_doc(&embed("{\"paths\": {}}"))
                .unwrap_err()
                .contains("not an OpenAPI document")
        );
        assert!(
            extract_swagger_doc("\"swaggerDoc\": [1]")
                .unwrap_err()
                .contains("not an object literal")
        );
        assert!(
            extract_swagger_doc("\"swaggerDoc\": {\"a\": 1")
                .unwrap_err()
                .contains("Unbalanced")
        );
    }

    #[test]
    fn describes_added_removed_and_changed_paths_and_schemas() {
        let before = serde_json::json!({ "paths": { "/a": { "get": 1 }, "/b": {} }, "components": { "schemas": { "S": { "x": 1 }, "T": {} } } }).to_string();
        let after = serde_json::json!({ "paths": { "/a": { "get": 2 }, "/c": {} }, "components": { "schemas": { "S": { "x": 1 }, "U": {} } } }).to_string();
        assert_eq!(
            describe_changes(&before, &after).unwrap(),
            [
                "+ path /c",
                "~ path /a",
                "- path /b",
                "+ schema U",
                "- schema T"
            ]
        );
        assert_eq!(
            describe_changes(&before, &before).unwrap(),
            ["Only formatting or ordering changed."]
        );
    }
}

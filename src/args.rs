//! Strict long-option parsing: no abbreviations, no short flags, no flags the
//! command does not declare, so a misspelling can never be silently ignored.

use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Switch,
    Value,
}

/// Every flag the CLI knows. Commands choose which of them they accept.
const KNOWN: &[(&str, Kind)] = &[
    ("format", Kind::Value),
    ("json", Kind::Switch),
    ("full", Kind::Switch),
    ("fields", Kind::Value),
    ("page", Kind::Value),
    ("page-size", Kind::Value),
    ("limit", Kind::Value),
    ("all", Kind::Switch),
    ("since", Kind::Value),
    ("start", Kind::Value),
    ("end", Kind::Value),
    ("file", Kind::Value),
    ("confirm", Kind::Switch),
    ("dry-run", Kind::Switch),
    ("allow-duplicate", Kind::Switch),
    ("check", Kind::Switch),
];

pub const OUTPUT: &[&str] = &["format", "json", "full", "fields"];
pub const PAGINATION: &[&str] = &["page", "page-size", "limit", "all"];
pub const MUTATION: &[&str] = &["file", "confirm", "dry-run"];

#[derive(Debug, Default)]
pub struct Parsed {
    pub positionals: Vec<String>,
    switches: HashSet<String>,
    values: HashMap<String, String>,
}

impl Parsed {
    pub fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Fail with `Usage: ...` unless exactly `count` positionals were given.
    pub fn expect_positionals(&self, count: usize, usage: &str) -> Result<()> {
        if self.positionals.len() == count {
            Ok(())
        } else {
            Err(Error::validation(format!("Usage: {usage}")))
        }
    }

    /// A positive integer flag such as `--page`, bounded to what JSON clients
    /// can represent exactly.
    pub fn positive_integer(&self, name: &str) -> Result<Option<u64>> {
        const MAX_SAFE: u64 = (1 << 53) - 1;
        let Some(text) = self.value(name) else {
            return Ok(None);
        };
        let shaped =
            !text.starts_with('0') && !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
        if !shaped {
            return Err(Error::validation(format!(
                "--{name} must be a positive integer."
            )));
        }
        match text.parse::<u64>() {
            Ok(n) if n <= MAX_SAFE => Ok(Some(n)),
            _ => Err(Error::validation(format!(
                "--{name} must be a safe integer."
            ))),
        }
    }
}

/// Parse `args` against the flags named in `allowed`.
pub fn parse(args: &[String], allowed: &[&[&str]]) -> Result<Parsed> {
    let accepts = |name: &str| allowed.iter().any(|group| group.contains(&name));
    let mut parsed = Parsed::default();
    let mut positional_only = false;
    let mut iter = args.iter().peekable();

    while let Some(argument) = iter.next() {
        if positional_only || !argument.starts_with('-') {
            parsed.positionals.push(argument.clone());
            continue;
        }
        if argument == "--" {
            positional_only = true;
            continue;
        }
        let Some(flag) = argument.strip_prefix("--") else {
            return Err(Error::validation(format!("Unknown flag: {argument}.")));
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        let kind = KNOWN
            .iter()
            .find(|(known, _)| *known == name && accepts(name))
            .map(|(_, kind)| *kind)
            .ok_or_else(|| Error::validation(format!("Unknown flag: --{name}.")))?;
        if parsed.switches.contains(name) || parsed.values.contains_key(name) {
            return Err(Error::validation(format!(
                "--{name} may only be specified once."
            )));
        }

        match (kind, inline) {
            (Kind::Switch, Some(_)) => {
                return Err(Error::validation(format!(
                    "--{name} does not take a value."
                )));
            }
            (Kind::Switch, None) => {
                parsed.switches.insert(name.to_owned());
            }
            (Kind::Value, inline) => {
                // A bare "-" is the conventional stdin operand, not a flag.
                let value = match inline {
                    Some(value) => value.to_owned(),
                    None => iter
                        .next_if(|next| *next == "-" || !next.starts_with('-'))
                        .cloned()
                        .ok_or_else(|| Error::validation(format!("--{name} requires a value.")))?,
                };
                if value.is_empty() {
                    return Err(Error::validation(format!(
                        "--{name} requires a non-empty value."
                    )));
                }
                parsed.values.insert(name.to_owned(), value);
            }
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests;

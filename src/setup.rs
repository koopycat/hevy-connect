//! `setup`: local credential and ambient-context management. No Hevy API call.

use serde_json::{Value, json};

use crate::args::Parsed;
use crate::config::{self, Environment};
use crate::error::{Code, Error, Result};
use crate::hooks;
use crate::input::read_stdin;
use crate::output::OutputOptions;

pub const SETUP_KEY_COMMAND: &str =
    "printf '%s\\n' \"$HEVY_API_KEY\" | hevy-axi setup key --confirm";

pub fn hooks_status(env: &Environment) -> Value {
    let status = hooks::status(env);
    json!({ "scope": "user", "claude": status.claude, "codex": status.codex, "opencode": status.opencode })
}

/// A key piped in as `KEY`, `HEVY_API_KEY=KEY`, or either one quoted.
fn clean_key(input: &str) -> &str {
    let key = input.trim();
    let key = key
        .strip_prefix("HEVY_API_KEY")
        .and_then(|rest| rest.trim_start().strip_prefix('='))
        .map_or(key, str::trim);
    for quote in ['"', '\''] {
        if let Some(inner) = key
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    key
}

pub fn run(
    env: &Environment,
    action: &str,
    parsed: &Parsed,
    options: &OutputOptions,
) -> Result<String> {
    let require_confirm = || {
        if parsed.switch("confirm") {
            Ok(())
        } else {
            Err(Error::validation(format!(
                "setup {action} requires --confirm."
            )))
        }
    };
    let result = match action {
        "status" => {
            if parsed.switch("confirm") {
                return Err(Error::validation("setup status does not accept --confirm."));
            }
            let config = config::resolve(env)?;
            let help = if config.api_key.is_some() {
                json!(["hevy-axi user info"])
            } else {
                json!([SETUP_KEY_COMMAND])
            };
            json!({
                "configured": config.api_key.is_some(),
                "credentialSource": config.credential_category(env),
                "baseUrl": config.base_url.as_str(),
                "hooks": hooks_status(env),
                "help": help,
            })
        }
        "key" => {
            require_confirm()?;
            let input = read_stdin()?;
            let key = clean_key(&input);
            if key.is_empty() {
                return Err(Error::validation("No API key was provided on stdin."));
            }
            let status = config::store_api_key(env, key)?;
            json!({ "status": status, "credentialSource": "global", "help": ["hevy-axi setup status", "hevy-axi user info"] })
        }
        "remove-key" => {
            require_confirm()?;
            json!({ "status": config::remove_stored_api_key(env)?, "credentialSource": "global" })
        }
        "hooks" => {
            require_confirm()?;
            let exe = std::env::current_exe().map_err(|_| {
                Error::new(
                    Code::Config,
                    "The path of this executable could not be determined.",
                )
            })?;
            hooks::install(env, &exe)?;
            json!({ "status": "installed", "hooks": hooks_status(env) })
        }
        "remove-hooks" => {
            require_confirm()?;
            hooks::uninstall(env)?;
            json!({ "status": "removed", "hooks": hooks_status(env) })
        }
        other => return Err(Error::validation(format!("Unknown setup action: {other}."))),
    };
    options.render_projected(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_cleaned_of_assignments_and_quotes() {
        for (input, want) in [
            ("abc\n", "abc"),
            ("  abc  ", "abc"),
            ("HEVY_API_KEY=abc", "abc"),
            ("HEVY_API_KEY = \"abc\"\n", "abc"),
            ("'abc'", "abc"),
            ("\"abc\"", "abc"),
            ("\"abc'", "\"abc'"),
            ("", ""),
        ] {
            assert_eq!(clean_key(input), want, "{input:?}");
        }
    }
}

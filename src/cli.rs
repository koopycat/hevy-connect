//! Command dispatch: from raw arguments to rendered output.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::args::{self, MUTATION, OUTPUT, PAGINATION, Parsed};
use crate::client::Client;
use crate::compact::Kind;
use crate::config::{self, Environment};
use crate::error::{Error, Result};
use crate::help;
use crate::list::{self, ListSpec};
use crate::mutate;
use crate::output::{Format, OutputOptions, render_error, requested_format};
use crate::read;
use crate::resource::{self, Action, Spec};
use crate::setup;
use crate::time::{validate_date, validate_iso};

const DESCRIPTION: &str = "Agent-ergonomic access to the Hevy Public API.";
const DEFAULT_EVENTS_SINCE: &str = "1970-01-01T00:00:00Z";

/// What a run prints and how it exits.
pub struct Outcome {
    pub text: String,
    pub code: u8,
}

impl Outcome {
    /// A failure that happened before any command could run.
    pub fn error(error: &Error) -> Self {
        Self {
            text: render_error(error, Format::Toon),
            code: error.exit_code(),
        }
    }
}

/// Run one invocation. Results and errors alike are printed on stdout,
/// rendered in the format the arguments asked for.
pub fn run(argv: &[String], env: &Environment, exe: &Path) -> Outcome {
    match dispatch(argv, env, exe) {
        Ok(text) => Outcome { text, code: 0 },
        Err(error) => Outcome {
            text: render_error(&error, requested_format(argv)),
            code: error.exit_code(),
        },
    }
}

fn dispatch(argv: &[String], env: &Environment, exe: &Path) -> Result<String> {
    let Some((command, args)) = argv.split_first() else {
        return home(env, exe);
    };
    if args.is_empty() {
        match command.as_str() {
            "--help" => return Ok(help::top_level()),
            "-v" | "-V" | "--version" => return Ok(env!("CARGO_PKG_VERSION").to_owned()),
            _ => {}
        }
    }
    if command.starts_with('-') {
        return Err(Error::validation("Flags must come after the command.")
            .with_suggestion("Run `hevy-axi <command> [args] [flags]`")
            .with_suggestion(format!(
                "Move `{command}` after the command instead of before it"
            )));
    }
    if args.iter().any(|arg| arg == "--help")
        && let Some(page) = help::command(command)
    {
        return Ok(page);
    }

    match command.as_str() {
        "setup" => setup_command(env, args),
        "update" => update_command(exe, args),
        name => match resource::find(name) {
            Some(spec) => resource_command(env, spec, args),
            None => Err(Error::validation(format!("Unknown command: {name}."))
                .with_suggestion("Run `hevy-axi --help` to see available commands.")),
        },
    }
}

fn require_action<'a>(args: &'a [String], command: &str) -> Result<(&'a str, &'a [String])> {
    args.split_first()
        .map(|(action, rest)| (action.as_str(), rest))
        .ok_or_else(|| {
            Error::validation(format!(
                "An action is required. Run \"hevy-axi {command} --help\"."
            ))
        })
}

fn usage(command: &str, action: &str, takes_id: bool, mutation: bool) -> String {
    let id = if takes_id { " <id>" } else { "" };
    let tail = if mutation {
        "--file <path|-> (--confirm|--dry-run)"
    } else {
        "[flags]"
    };
    format!("hevy-axi {command} {action}{id} {tail}")
}

fn resource_command(env: &Environment, spec: &Spec, args: &[String]) -> Result<String> {
    let (action_name, rest) = require_action(args, spec.name)?;
    let action = spec.action(action_name).ok_or_else(|| {
        Error::validation(format!("Unknown {} action: {action_name}.", spec.name))
    })?;

    let mutating = matches!(action, Action::Create | Action::Update);
    let duplicate_flag: &[&str] = if spec.name == "exercise" && action == Action::Create {
        &["allow-duplicate"]
    } else {
        &[]
    };
    let allowed: Vec<&[&str]> = match action {
        Action::List => vec![OUTPUT, PAGINATION],
        Action::Events => vec![OUTPUT, PAGINATION, &["since"]],
        Action::History => vec![OUTPUT, &["start", "end"]],
        Action::Count | Action::View | Action::Info => vec![OUTPUT],
        Action::Create | Action::Update => vec![OUTPUT, MUTATION, duplicate_flag],
    };
    let parsed = args::parse(rest, &allowed)?;
    parsed.expect_positionals(
        usize::from(action.takes_id()),
        &usage(spec.name, action_name, action.takes_id(), mutating),
    )?;
    let options = OutputOptions::from_args(&parsed)?;
    let id = parsed
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_default();

    match action {
        Action::List => {
            Ok(options.render(list::list(env, &parsed, &ListSpec::of(spec), &options)?))
        }
        Action::Events => events(env, spec, &parsed, &options),
        Action::Count => read::count(&Client::configured(env)?, &options),
        Action::Info => read::view(&Client::configured(env)?, spec, spec.path, &options),
        Action::View => {
            let path = spec.record_path(id)?;
            read::view(&Client::configured(env)?, spec, &path, &options)
        }
        Action::History => {
            let (start, end) = (parsed.value("start"), parsed.value("end"));
            let instant =
                |value: Option<&str>, name| value.map(|v| validate_iso(v, name)).transpose();
            if let (Some(start), Some(end)) = (instant(start, "--start")?, instant(end, "--end")?)
                && start > end
            {
                return Err(Error::validation("--start must not be later than --end."));
            }
            spec.id_segment(id)?;
            read::history(&Client::configured(env)?, spec, id, (start, end), &options)
        }
        Action::Create => mutate::run(env, &parsed, spec, false, spec.path, &options),
        Action::Update if spec.is_measurement() => {
            mutate::update_measurement(env, &parsed, spec, validate_date(id)?, &options)
        }
        Action::Update => mutate::run(env, &parsed, spec, true, &spec.record_path(id)?, &options),
    }
}

fn events(
    env: &Environment,
    spec: &Spec,
    parsed: &Parsed,
    options: &OutputOptions,
) -> Result<String> {
    let requested = parsed.value("since");
    if let Some(since) = requested {
        validate_iso(since, "--since")?;
    }
    let list_spec = ListSpec {
        action: "events",
        path: "/v1/workouts/events",
        array_key: "events",
        kind: Kind::Event,
        extra_query: vec![(
            "since",
            requested.unwrap_or(DEFAULT_EVENTS_SINCE).to_owned(),
        )],
        continuation: requested
            .map(|since| vec!["--since".to_owned(), since.to_owned()])
            .unwrap_or_default(),
        counts_total: false,
        has_view: false,
        ..ListSpec::of(spec)
    };
    Ok(options.render(list::list(env, parsed, &list_spec, options)?))
}

fn setup_command(env: &Environment, args: &[String]) -> Result<String> {
    let (action, rest) = require_action(args, "setup")?;
    let parsed = args::parse(rest, &[OUTPUT, &["confirm"]])?;
    parsed.expect_positionals(0, &usage("setup", action, false, false))?;
    let options = OutputOptions::from_args(&parsed)?;
    setup::run(env, action, &parsed, &options)
}

fn update_command(exe: &Path, args: &[String]) -> Result<String> {
    let parsed = args::parse(args, &[OUTPUT, &["check"]])?;
    parsed.expect_positionals(0, "hevy-axi update [--check] [flags]")?;
    let options = OutputOptions::from_args(&parsed)?;
    // --check is accepted for AXI compatibility; both forms only report.
    options.render_projected(json!({
        "status": "manual_update_required",
        "currentVersion": env!("CARGO_PKG_VERSION"),
        "executable": exe.display().to_string(),
        "help": ["In your checkout of this repository: git pull --ff-only && just install", "hevy-axi --version"],
    }))
}

/// The home view: local configuration only, with no API call and no account or
/// workout data, because SessionStart hooks inject it into every agent session.
fn home(env: &Environment, exe: &Path) -> Result<String> {
    let config = config::resolve(env)?;
    let mut view = Map::new();
    view.insert("bin".into(), collapse_home(exe, &env.home).into());
    view.insert("description".into(), DESCRIPTION.into());
    let status = if config.api_key.is_none() {
        json!({
            "status": "not_configured",
            "configured": false,
            "help": [setup::SETUP_KEY_COMMAND, "hevy-axi setup status"],
        })
    } else {
        json!({
            "status": "configured",
            "configured": true,
            "credentialSource": config.credential_category(env),
            "help": ["hevy-axi workout list", "hevy-axi routine list", "hevy-axi exercise list", "hevy-axi user info"],
        })
    };
    if let Value::Object(status) = status {
        view.extend(status);
    }
    Ok(crate::output::render(&Value::Object(view), Format::Toon))
}

/// `~/...` for paths under the home directory.
fn collapse_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => Path::new("~").join(rest).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_under_home_are_collapsed() {
        let home = Path::new("/Users/vk");
        assert_eq!(
            collapse_home(Path::new("/Users/vk/.local/bin/hevy-axi"), home),
            "~/.local/bin/hevy-axi"
        );
        assert_eq!(
            collapse_home(Path::new("/Users/vk2/bin/hevy-axi"), home),
            "/Users/vk2/bin/hevy-axi"
        );
        assert_eq!(
            collapse_home(Path::new("/usr/bin/hevy-axi"), home),
            "/usr/bin/hevy-axi"
        );
    }

    #[test]
    fn usage_names_the_action_and_its_shape() {
        assert_eq!(
            usage("workout", "view", true, false),
            "hevy-axi workout view <id> [flags]"
        );
        assert_eq!(
            usage("folder", "create", false, true),
            "hevy-axi folder create --file <path|-> (--confirm|--dry-run)"
        );
    }
}

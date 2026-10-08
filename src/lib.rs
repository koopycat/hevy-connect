//! `hevy-axi`: an agent-ergonomic CLI over the Hevy Public API.

mod args;
mod cli;
mod client;
mod compact;
mod config;
mod error;
mod fsutil;
mod help;
mod hooks;
mod input;
mod list;
mod mutate;
mod output;
mod read;
mod resource;
mod setup;
mod time;
mod wire;

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

/// Run the CLI for `args` (without the program name) against the real process
/// environment, printing to stdout.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let outcome = match (
        args.into_iter()
            .map(OsString::into_string)
            .collect::<Result<Vec<_>, _>>(),
        config::Environment::from_process(),
        std::env::current_exe(),
    ) {
        (Ok(argv), Ok(env), Ok(exe)) => cli::run(&argv, &env, &exe),
        (Err(_), ..) => {
            cli::Outcome::error(&error::Error::validation("Arguments must be valid UTF-8."))
        }
        _ => cli::Outcome::error(&error::Error::new(
            error::Code::Config,
            "The working directory, home directory, or executable path could not be determined.",
        )),
    };
    // A closed pipe (`hevy-axi ... | head`) is a normal way for a reader to stop.
    match writeln!(io::stdout().lock(), "{}", outcome.text) {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => ExitCode::from(1),
        _ => ExitCode::from(outcome.code),
    }
}

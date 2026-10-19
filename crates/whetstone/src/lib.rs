//! The Whetstone command-line app.
//!
//! [`run`] is the whole program: it parses arguments and reports the outcome
//! as an exit code. `main.rs` only passes in the process's inputs.

mod app;
mod cli;
mod environment;
mod home;
mod logging;
mod output;
mod terminal;
mod theme;
mod tui;

pub use crate::app::{App, Flow};
pub use crate::environment::{Dirs, Environment, NoHome};
pub use crate::terminal::{ModeSwitch, TerminalGuard, TerminalMode};
pub use crate::theme::Theme;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser as _;

use crate::cli::Cli;
use crate::tui::Ending;

/// Runs Whetstone with the given command-line arguments, the first of which
/// is the program name, in the given environment.
pub fn run<I, T>(args: I, environment: &Environment) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    if let Err(outcome) = Cli::try_parse_from(args) {
        return output::clap_outcome(&outcome);
    }
    launch(environment).unwrap_or_else(|error| output::failure(&error))
}

/// Opens the TUI. A refusal happens before anything is drawn or written.
fn launch(environment: &Environment) -> anyhow::Result<ExitCode> {
    terminal::require_interactive()?;
    let dirs = Dirs::resolve(environment)?;
    logging::init(&dirs.state)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %dirs.config.display(),
        "whetstone started"
    );
    match tui::run(App::new(Theme::plain()))? {
        Ending::Quit => {
            tracing::info!("whetstone quit");
            Ok(ExitCode::SUCCESS)
        }
        Ending::Stopped { signal } => {
            tracing::info!(signal, "whetstone stopped by a signal");
            // The shell convention for a process ended by a signal.
            Ok(ExitCode::from(
                u8::try_from(128 + signal).unwrap_or(output::FAILURE),
            ))
        }
    }
}

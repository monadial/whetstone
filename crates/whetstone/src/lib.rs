//! The Whetstone command-line app.
//!
//! [`run`] is the whole program: it parses arguments and reports the outcome
//! as an exit code. `main.rs` only passes in the process's inputs.

mod app;
mod areas;
mod cli;
mod columns;
mod config;
mod entry;
mod environment;
mod focus;
mod help;
mod home;
mod keymap;
mod logging;
mod mark;
mod outbound_review;
mod output;
mod overlay;
mod palette;
mod picker;
mod reader;
mod repository;
mod search;
mod settings;
mod showcase;
mod terminal;
mod theme;
mod tui;
mod view;
mod viewer;

pub use crate::app::{App, Flow};
pub use crate::config::{ConfigError, Settings};
pub use crate::environment::{Dirs, Environment, NoHome};
pub use crate::repository::{Repository, Unavailable};
pub use crate::showcase::embedded_showcase;
pub use crate::terminal::{ModeSwitch, TerminalGuard, TerminalMode};
pub use crate::theme::{ColorSupport, ControlKind, ControlState, Theme};

use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser as _;

use crate::cli::{AreasCommand, Cli, Command};
use crate::tui::Ending;

/// Runs Whetstone with the given command-line arguments, the first of which
/// is the program name, in the given environment.
pub fn run<I, T>(args: I, environment: &Environment) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(outcome) => return output::clap_outcome(&outcome),
    };
    match cli.command {
        None => launch(environment),
        Some(Command::Areas(AreasCommand::Init)) => init_areas(environment),
    }
    .unwrap_or_else(|error| output::failure(&error))
}

/// Writes the default area files into the repository Whetstone runs in.
fn init_areas(environment: &Environment) -> anyhow::Result<ExitCode> {
    let written = areas::init(environment.current_dir())?;
    output::written(&written);
    Ok(ExitCode::SUCCESS)
}

/// Opens the TUI. A refusal happens before anything is drawn or written.
fn launch(environment: &Environment) -> anyhow::Result<ExitCode> {
    terminal::require_interactive(environment)?;
    let dirs = Dirs::resolve(environment)?;
    logging::init(&dirs.state)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %dirs.config.display(),
        "whetstone started"
    );
    let settings = Settings::load(&dirs.config, environment)?;
    let theme = Theme::new(settings.color_support());
    let repository = Repository::open(environment.current_dir());
    repository::log(&repository);
    match tui::run(
        App::new(theme, settings, embedded_showcase(), repository),
        environment,
    )? {
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

//! Diagnostics go to a log file in the state directory, never to the screen:
//! the TUI owns the terminal.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::Mutex;

use tracing::level_filters::LevelFilter;

use crate::environment::Environment;

/// Sets the level, such as `debug` or `off`. Defaults to `info`.
const LEVEL_VARIABLE: &str = "WHETSTONE_LOG";
const DEFAULT_LEVEL: LevelFilter = LevelFilter::INFO;

/// Logging could not be started.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoggingError {
    #[error("cannot open the log file {}", path.display())]
    Open { path: PathBuf, source: io::Error },
    #[error("logging was already started in this process")]
    AlreadyStarted,
}

/// Sends this process's log events to `whetstone.log` in `state_dir`,
/// appending to it.
pub(crate) fn init(state_dir: &Path, environment: &Environment) -> Result<(), LoggingError> {
    let path = state_dir.join("whetstone.log");
    let file = fs::create_dir_all(state_dir)
        .and_then(|()| OpenOptions::new().create(true).append(true).open(&path))
        .map_err(|source| LoggingError::Open { path, source })?;

    let requested = environment.get(LEVEL_VARIABLE);
    let level = requested
        .and_then(|value| value.to_str())
        .and_then(|value| LevelFilter::from_str(value).ok());
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_max_level(level.unwrap_or(DEFAULT_LEVEL))
        .try_init()
        .map_err(|_already_set| LoggingError::AlreadyStarted)?;
    if requested.is_some() && level.is_none() {
        tracing::warn!("{LEVEL_VARIABLE} is not a log level; logging at {DEFAULT_LEVEL}");
    }
    Ok(())
}

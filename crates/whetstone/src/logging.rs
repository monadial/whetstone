//! Diagnostics go to a log file in the state directory, never to the screen:
//! the TUI owns the terminal.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::level_filters::LevelFilter;

const LEVEL: LevelFilter = LevelFilter::INFO;

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
pub(crate) fn init(state_dir: &Path) -> Result<(), LoggingError> {
    let path = state_dir.join("whetstone.log");
    let file = fs::create_dir_all(state_dir)
        .and_then(|()| OpenOptions::new().create(true).append(true).open(&path))
        .map_err(|source| LoggingError::Open { path, source })?;
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_max_level(LEVEL)
        .try_init()
        .map_err(|_already_set| LoggingError::AlreadyStarted)
}

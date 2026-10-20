//! Diagnostics go to a log file in the state directory, never to the screen:
//! the TUI owns the terminal.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

const LEVEL: LevelFilter = LevelFilter::INFO;

/// The log targets of the connected-code engine and of the directory walker
/// it reads with. Their events can name the learner's files and callables,
/// so only their warnings and errors are kept. A target matches every target
/// it begins, so `cgg` also covers `cgg_core`, `cgg_lang` and the rest.
const ENGINE_TARGETS: [&str; 3] = ["cgg", "globset", "ignore"];

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
    subscriber(Mutex::new(file))
        .try_init()
        .map_err(|_already_set| LoggingError::AlreadyStarted)
}

/// The subscriber that writes Whetstone's log events to `writer`: plain
/// text, from `LEVEL` up, and only warnings and errors from the engine.
fn subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let targets = ENGINE_TARGETS
        .into_iter()
        .fold(Targets::new().with_default(LEVEL), |targets, target| {
            targets.with_target(target, LevelFilter::WARN)
        });
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_max_level(LEVEL)
        // A dependency turns on tracing-subscriber's `ansi` feature, which
        // would otherwise colour the file whenever `NO_COLOR` is unset.
        .with_ansi(false)
        .finish()
        .with(targets)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn the_engine_logs_only_warnings_and_errors_in_plain_text() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&log);
        let subscriber = subscriber(move || Captured(Arc::clone(&writer)));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "cgg_core::graph", "callable charge in src/pay.rs");
            tracing::info!(target: "ignore::walk", "walking src/pay.rs");
            tracing::warn!(target: "cgg", "the engine skipped a file");
            tracing::info!("opened the outbound review");
        });

        let log = String::from_utf8(log.lock().expect("log lock").clone()).expect("UTF-8");
        assert!(!log.contains("pay.rs"), "{log}");
        assert!(log.contains("the engine skipped a file"), "{log}");
        assert!(log.contains("opened the outbound review"), "{log}");
        assert!(!log.contains('\x1b'), "{log:?}");
    }

    /// A writer that appends to a shared buffer.
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("log lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

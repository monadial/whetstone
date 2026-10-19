//! Helpers shared by the terminal guard tests.

use std::io;
use std::sync::{Arc, Mutex};

use whetstone::{ModeSwitch, TerminalMode};

/// A [`ModeSwitch`] that records what it was asked to do, and can be told to
/// fail one mode.
#[derive(Debug, Clone, Default)]
pub(crate) struct RecordingSwitch {
    pub(crate) log: Log,
    /// A mode that fails to enable.
    pub(crate) failing: Option<TerminalMode>,
}

/// An ordered record of what happened, shared with the test.
#[derive(Debug, Clone, Default)]
pub(crate) struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    pub(crate) fn push(&self, entry: impl Into<String>) {
        self.0.lock().expect("log lock").push(entry.into());
    }

    pub(crate) fn entries(&self) -> Vec<String> {
        self.0.lock().expect("log lock").clone()
    }
}

impl RecordingSwitch {
    pub(crate) fn new(log: &Log) -> Self {
        Self {
            log: log.clone(),
            failing: None,
        }
    }
}

impl ModeSwitch for RecordingSwitch {
    fn enable(&mut self, mode: TerminalMode) -> io::Result<()> {
        if self.failing == Some(mode) {
            return Err(io::Error::other(format!("{mode:?} unsupported")));
        }
        self.log.push(format!("enable {mode:?}"));
        Ok(())
    }

    fn disable(&mut self, mode: TerminalMode) -> io::Result<()> {
        self.log.push(format!("disable {mode:?}"));
        Ok(())
    }
}

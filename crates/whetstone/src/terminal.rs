//! Ownership of the user's terminal.
//!
//! [`TerminalGuard`] turns terminal modes on and guarantees they are turned
//! off again: when it is restored or dropped, and from a panic hook that runs
//! before the panic message prints. Ratatui's own restore only covers raw mode
//! and the alternate screen, so Whetstone owns this.

use std::io::{self, IsTerminal as _, Write};
use std::panic;
use std::sync::{Arc, Mutex, PoisonError, TryLockError};

use ratatui::crossterm::{cursor, execute, terminal};

/// Whetstone was started without an interactive terminal to draw in.
#[derive(Debug, thiserror::Error)]
pub(crate) enum NotInteractive {
    #[error("whetstone needs an interactive terminal, but stdin is not a terminal")]
    Stdin,
    #[error("whetstone needs an interactive terminal, but stdout is not a terminal")]
    Stdout,
    #[error("whetstone needs an interactive terminal, but stdin and stdout are not terminals")]
    StdinAndStdout,
}

/// Checks that both stdin and stdout are terminals, before anything is drawn.
pub(crate) fn require_interactive() -> Result<(), NotInteractive> {
    match (io::stdin().is_terminal(), io::stdout().is_terminal()) {
        (true, true) => Ok(()),
        (false, true) => Err(NotInteractive::Stdin),
        (true, false) => Err(NotInteractive::Stdout),
        (false, false) => Err(NotInteractive::StdinAndStdout),
    }
}

/// A terminal mode Whetstone turns on and must turn off again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalMode {
    /// Keys reach the app unprocessed, including Ctrl+C.
    Raw,
    /// A separate screen, so the user's scrollback is left untouched.
    AlternateScreen,
    /// The text cursor is hidden.
    HiddenCursor,
}

impl TerminalMode {
    /// Every mode the TUI uses, in the order it enables them.
    pub(crate) const TUI: [Self; 3] = [Self::Raw, Self::AlternateScreen, Self::HiddenCursor];
}

/// Turns terminal modes on and off for a [`TerminalGuard`].
pub trait ModeSwitch: Send {
    /// Turns `mode` on.
    fn enable(&mut self, mode: TerminalMode) -> io::Result<()>;
    /// Turns `mode` off again. Called only for modes `enable` turned on.
    fn disable(&mut self, mode: TerminalMode) -> io::Result<()>;
}

/// Restores every terminal mode it enabled, exactly once: on [`restore`],
/// on drop, or from its panic hook, whichever comes first.
///
/// The panic hook stays installed after the guard is gone and does nothing
/// from then on. Install other panic hooks before enabling a guard, so this
/// one runs first.
///
/// [`restore`]: TerminalGuard::restore
#[derive(Debug)]
pub struct TerminalGuard {
    session: Arc<Mutex<Session>>,
}

struct Session {
    switch: Box<dyn ModeSwitch>,
    /// Modes currently on, in the order they were enabled.
    enabled: Vec<TerminalMode>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl TerminalGuard {
    /// Installs the panic hook, then enables `modes` in order. If a mode
    /// fails to enable, the modes already enabled are restored and the error
    /// is returned.
    pub fn enable(switch: impl ModeSwitch + 'static, modes: &[TerminalMode]) -> io::Result<Self> {
        let session = Arc::new(Mutex::new(Session {
            switch: Box::new(switch),
            enabled: Vec::with_capacity(modes.len()),
        }));
        install_panic_hook(Arc::clone(&session));
        let guard = Self { session };
        for &mode in modes {
            let mut session = guard.lock();
            session.switch.enable(mode)?;
            session.enabled.push(mode);
        }
        Ok(guard)
    }

    /// Restores every mode this guard enabled, in reverse order, and reports
    /// the first failure. Every mode is attempted even if one fails.
    pub fn restore(self) -> io::Result<()> {
        self.lock().restore()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Nothing can report a failure from here; `restore` does.
        drop(self.lock().restore());
    }
}

impl Session {
    fn restore(&mut self) -> io::Result<()> {
        let mut outcome = Ok(());
        while let Some(mode) = self.enabled.pop() {
            let result = self.switch.disable(mode);
            if outcome.is_ok() {
                outcome = result;
            }
        }
        outcome
    }
}

fn install_panic_hook(session: Arc<Mutex<Session>>) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Never block or panic in here: skip the restore if the session is
        // busy on another thread, and keep the panic message either way.
        let session = match session.try_lock() {
            Ok(session) => Some(session),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        };
        if let Some(mut session) = session {
            drop(session.restore());
        }
        previous(info);
    }));
}

/// The real [`ModeSwitch`], driving the terminal on stdout via crossterm.
#[derive(Debug)]
pub(crate) struct CrosstermSwitch<W> {
    out: W,
}

impl<W: Write + Send> CrosstermSwitch<W> {
    pub(crate) fn new(out: W) -> Self {
        Self { out }
    }
}

impl<W: Write + Send> ModeSwitch for CrosstermSwitch<W> {
    fn enable(&mut self, mode: TerminalMode) -> io::Result<()> {
        match mode {
            TerminalMode::Raw => terminal::enable_raw_mode(),
            TerminalMode::AlternateScreen => execute!(self.out, terminal::EnterAlternateScreen),
            TerminalMode::HiddenCursor => execute!(self.out, cursor::Hide),
        }
    }

    fn disable(&mut self, mode: TerminalMode) -> io::Result<()> {
        match mode {
            TerminalMode::Raw => terminal::disable_raw_mode(),
            TerminalMode::AlternateScreen => execute!(self.out, terminal::LeaveAlternateScreen),
            TerminalMode::HiddenCursor => execute!(self.out, cursor::Show),
        }
    }
}

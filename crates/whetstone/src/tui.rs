//! The single UI loop: it owns the terminal while the app runs.

use std::ffi::c_int;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::style;
use signal_hook::SigId;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

use crate::app::{App, Flow};
use crate::environment::Environment;
use crate::terminal::{CrosstermSwitch, TerminalGuard, TerminalMode};

/// How long the loop waits for input before checking for a stop signal.
const SIGNAL_CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// Why the UI loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// The learner quit.
    Quit,
    /// A signal from outside the terminal stopped the app.
    Stopped { signal: c_int },
}

/// The terminal could not be used.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TuiError {
    #[error("cannot listen for stop signals")]
    Signals(#[source] io::Error),
    #[error("cannot take over the terminal")]
    Enable(#[source] io::Error),
    #[error("cannot draw to the terminal")]
    Draw(#[source] io::Error),
    #[error("cannot read terminal input")]
    Input(#[source] io::Error),
    #[error("cannot restore the terminal")]
    Restore(#[source] io::Error),
}

/// Takes over the terminal, runs `app` until it quits or a signal stops it,
/// then gives the terminal back exactly as it was. `environment` can set the
/// [`CrashTrigger`].
pub(crate) fn run(mut app: App, environment: &Environment) -> Result<Ending, TuiError> {
    let crash = CrashTrigger::read(environment);
    // The theme alone honours NO_COLOR. Under NO_COLOR crossterm 0.29 writes
    // each colour, even a reset, as a bare `CSI m`, which also clears the
    // bold and other attributes that mark focus without colour.
    style::force_color_output(true);
    // Listen before taking over the terminal, so no signal can leave it in
    // raw mode.
    let stop = StopSignals::listen().map_err(TuiError::Signals)?;
    let guard = TerminalGuard::enable(CrosstermSwitch::new(io::stdout()), &TerminalMode::TUI)
        .map_err(TuiError::Enable)?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(TuiError::Draw)?;
    // The terminal reports later sizes as resize events, but not the first.
    let size = terminal.size().map_err(TuiError::Draw)?;
    app.handle(&Event::Resize(size.width, size.height));
    let ending = loop {
        terminal
            .draw(|frame| app.render(frame))
            .map_err(TuiError::Draw)?;
        crash.after_draw();
        match next_input(&stop)? {
            Input::Event(event) => {
                if app.handle(&event) == Flow::Quit {
                    break Ending::Quit;
                }
            }
            Input::Stop { signal } => break Ending::Stopped { signal },
        }
    };
    drop(terminal);
    guard.restore().map_err(TuiError::Restore)?;
    Ok(ending)
}

enum Input {
    Event(Event),
    Stop { signal: c_int },
}

/// Waits for the next terminal event, the loop's only input source, or for a
/// stop signal.
fn next_input(stop: &StopSignals) -> Result<Input, TuiError> {
    loop {
        if let Some(signal) = stop.received() {
            return Ok(Input::Stop { signal });
        }
        match event::poll(SIGNAL_CHECK_INTERVAL) {
            Ok(true) => return event::read().map(Input::Event).map_err(TuiError::Input),
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(TuiError::Input(error)),
        }
    }
}

/// Signals from outside the terminal that stop the app the way Ctrl+C does.
/// In raw mode Ctrl+C is a key, so SIGINT can only come from outside.
#[derive(Debug)]
struct StopSignals {
    /// The last stop signal received, or 0.
    received: Arc<AtomicUsize>,
    registrations: Vec<SigId>,
}

impl StopSignals {
    const SIGNALS: [c_int; 3] = [SIGHUP, SIGINT, SIGTERM];

    fn listen() -> io::Result<Self> {
        let received = Arc::new(AtomicUsize::new(0));
        let mut stop = Self {
            received,
            registrations: Vec::with_capacity(Self::SIGNALS.len()),
        };
        for signal in Self::SIGNALS {
            let value = usize::try_from(signal).expect("signal numbers are positive");
            stop.registrations.push(signal_hook::flag::register_usize(
                signal,
                Arc::clone(&stop.received),
                value,
            )?);
        }
        Ok(stop)
    }

    fn received(&self) -> Option<c_int> {
        match self.received.load(Ordering::Relaxed) {
            0 => None,
            signal => c_int::try_from(signal).ok(),
        }
    }
}

impl Drop for StopSignals {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

/// Whether a debug build panics right after its first draw, so a test can
/// watch a real panic restore the terminal. [`CRASH_AFTER_FIRST_DRAW`] sets
/// it. Release builds compile it out and never read the variable.
#[derive(Debug, Clone, Copy)]
enum CrashTrigger {
    /// Draw and carry on, as release builds always do.
    Unset,
    /// Panic right after the first draw.
    #[cfg(debug_assertions)]
    Set,
}

/// The variable that sets the [`CrashTrigger`] when it is not empty.
#[cfg(debug_assertions)]
const CRASH_AFTER_FIRST_DRAW: &str = "WHETSTONE_CRASH_AFTER_FIRST_DRAW";

impl CrashTrigger {
    #[cfg(debug_assertions)]
    fn read(environment: &Environment) -> Self {
        match environment.get(CRASH_AFTER_FIRST_DRAW) {
            Some(value) if !value.is_empty() => Self::Set,
            _ => Self::Unset,
        }
    }

    #[cfg(not(debug_assertions))]
    fn read(_: &Environment) -> Self {
        Self::Unset
    }

    /// Panics if the trigger is set. It runs after every draw, so the first
    /// draw is the one it follows.
    fn after_draw(self) {
        match self {
            Self::Unset => {}
            #[cfg(debug_assertions)]
            Self::Set => crash(),
        }
    }
}

#[cfg(debug_assertions)]
#[expect(clippy::panic, reason = "the crash trigger exists to panic")]
fn crash() -> ! {
    panic!("crashing after the first draw, as {CRASH_AFTER_FIRST_DRAW} asks")
}

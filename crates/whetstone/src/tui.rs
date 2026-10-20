//! The single UI loop: it owns the terminal while the app runs, and gives it
//! back while the app is suspended.

use std::ffi::c_int;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::style;
use ratatui::layout::{Position, Rect, Size};
use signal_hook::SigId;
use signal_hook::consts::{SIGCONT, SIGHUP, SIGINT, SIGTERM, SIGTSTP};

use crate::app::{App, Flow};
use crate::environment::Environment;
use crate::terminal::{CrosstermSwitch, TerminalGuard, TerminalMode};

/// The terminal as ratatui draws to it.
type Screen = Terminal<CrosstermBackend<io::Stdout>>;

/// How long the loop waits for input before checking for a signal.
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
    #[error("cannot listen for signals")]
    Signals(#[source] io::Error),
    #[error("cannot take over the terminal")]
    Enable(#[source] io::Error),
    #[error("cannot draw to the terminal")]
    Draw(#[source] io::Error),
    #[error("cannot read terminal input")]
    Input(#[source] io::Error),
    #[error("cannot restore the terminal")]
    Restore(#[source] io::Error),
    #[error("cannot suspend")]
    Suspend(#[source] io::Error),
}

/// Takes over the terminal, runs `app` until it quits or a signal stops it,
/// then gives the terminal back exactly as it was. Suspending gives the
/// terminal back until the process is continued. `environment` can set the
/// [`CrashTrigger`].
pub(crate) fn run(mut app: App, environment: &Environment) -> Result<Ending, TuiError> {
    let crash = CrashTrigger::read(environment);
    // The theme alone honours NO_COLOR. Under NO_COLOR crossterm 0.29 writes
    // each colour, even a reset, as a bare `CSI m`, which also clears the
    // bold and other attributes that mark focus without colour.
    style::force_color_output(true);
    // Listen before taking over the terminal, so no signal can leave it in
    // raw mode.
    let signals = Signals::listen().map_err(TuiError::Signals)?;
    let mut guard = TerminalGuard::enable(CrosstermSwitch::new(io::stdout()), &TerminalMode::TUI)
        .map_err(TuiError::Enable)?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(TuiError::Draw)?;
    // The terminal reports later sizes as resize events, but not the first.
    report_size(&terminal, &mut app)?;
    let ending = loop {
        terminal
            .draw(|frame| app.render(frame))
            .map_err(TuiError::Draw)?;
        crash.after_draw();
        let flow = match next_input(&signals)? {
            Input::Event(event) => app.handle(&event),
            Input::Stop { signal } => break Ending::Stopped { signal },
            Input::Suspend => Flow::Suspend,
            // Continued without suspending first, perhaps after a stop the
            // app could not catch. Taking the terminal back is harmless.
            Input::Continue => {
                resume(&mut guard, &mut terminal, &mut app)?;
                Flow::Continue
            }
        };
        match flow {
            Flow::Continue => {}
            Flow::Quit => break Ending::Quit,
            Flow::Suspend => {
                guard.suspend().map_err(TuiError::Restore)?;
                stop_until_continued().map_err(TuiError::Suspend)?;
                // A stop signal that came while suspended ends the app
                // before it takes the terminal back.
                if let Some(signal) = signals.stop.last() {
                    break Ending::Stopped { signal };
                }
                resume(&mut guard, &mut terminal, &mut app)?;
                // The SIGCONT that continued the app is handled.
                signals.job_control.take();
            }
        }
    };
    drop(terminal);
    guard.restore().map_err(TuiError::Restore)?;
    Ok(ending)
}

/// Tells `app` the size `terminal` has now, and returns it.
fn report_size(terminal: &Screen, app: &mut App) -> Result<Size, TuiError> {
    let size = terminal.size().map_err(TuiError::Draw)?;
    app.handle(&Event::Resize(size.width, size.height));
    Ok(size)
}

/// Stops the process the way SIGTSTP does by default, and returns once it
/// is continued, usually by the learner's `fg`.
fn stop_until_continued() -> io::Result<()> {
    signal_hook::low_level::emulate_default_handler(SIGTSTP)
}

/// Takes the terminal back once the process is continued: turns its modes
/// back on, reads its size again, since the window may have been resized
/// while stopped, and clears it, so the next frame draws the same screen in
/// full.
fn resume(guard: &mut TerminalGuard, terminal: &mut Screen, app: &mut App) -> Result<(), TuiError> {
    guard.resume().map_err(TuiError::Enable)?;
    let size = report_size(terminal, app)?;
    // Unlike `Terminal::clear`, resizing clears without asking the terminal
    // where its cursor is, a reply that could be lost or take a while.
    terminal
        .resize(Rect::from((Position::ORIGIN, size)))
        .map_err(TuiError::Draw)
}

enum Input {
    /// A terminal event.
    Event(Event),
    /// SIGHUP, SIGINT or SIGTERM.
    Stop { signal: c_int },
    /// SIGTSTP.
    Suspend,
    /// SIGCONT, other than the one that ends a suspend the loop handled
    /// itself.
    Continue,
}

/// Waits for the next terminal event, the loop's only input source, or for a
/// signal.
fn next_input(signals: &Signals) -> Result<Input, TuiError> {
    loop {
        if let Some(signal) = signals.stop.last() {
            return Ok(Input::Stop { signal });
        }
        match signals.job_control.take() {
            Some(SIGTSTP) => return Ok(Input::Suspend),
            Some(_) => return Ok(Input::Continue),
            None => {}
        }
        match event::poll(SIGNAL_CHECK_INTERVAL) {
            Ok(true) => return event::read().map(Input::Event).map_err(TuiError::Input),
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(TuiError::Input(error)),
        }
    }
}

/// Signals from outside the terminal, which the loop acts on between
/// events.
#[derive(Debug)]
struct Signals {
    /// SIGHUP, SIGINT and SIGTERM stop the app the way Ctrl+C does. In raw
    /// mode Ctrl+C is a key, so SIGINT can only come from outside.
    stop: Received,
    /// SIGTSTP suspends the app the way Ctrl+Z does, and SIGCONT continues
    /// it. In raw mode Ctrl+Z is a key, so SIGTSTP can only come from
    /// outside. As with the kernel's own stop and continue, only the later
    /// of the two counts.
    job_control: Received,
}

impl Signals {
    fn listen() -> io::Result<Self> {
        Ok(Self {
            stop: Received::listen(&[SIGHUP, SIGINT, SIGTERM])?,
            job_control: Received::listen(&[SIGTSTP, SIGCONT])?,
        })
    }
}

/// The last of some signals received, recorded by their handlers.
#[derive(Debug)]
struct Received {
    /// The last signal received, or 0.
    last: Arc<AtomicUsize>,
    registrations: Vec<SigId>,
}

impl Received {
    fn listen(signals: &[c_int]) -> io::Result<Self> {
        let mut received = Self {
            last: Arc::new(AtomicUsize::new(0)),
            registrations: Vec::with_capacity(signals.len()),
        };
        for &signal in signals {
            let value = usize::try_from(signal).expect("signal numbers are positive");
            received
                .registrations
                .push(signal_hook::flag::register_usize(
                    signal,
                    Arc::clone(&received.last),
                    value,
                )?);
        }
        Ok(received)
    }

    /// The last signal received, if any.
    fn last(&self) -> Option<c_int> {
        Self::signal(self.last.load(Ordering::Relaxed))
    }

    /// The last signal received since the last take, if any.
    fn take(&self) -> Option<c_int> {
        Self::signal(self.last.swap(0, Ordering::Relaxed))
    }

    fn signal(value: usize) -> Option<c_int> {
        match value {
            0 => None,
            signal => c_int::try_from(signal).ok(),
        }
    }
}

impl Drop for Received {
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

//! The single UI loop: it owns the terminal while the app runs.

use std::io;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event;

use crate::app::{App, Flow};
use crate::terminal::{CrosstermSwitch, TerminalGuard, TerminalMode};

/// Takes over the terminal, runs `app` until it quits, then gives the
/// terminal back exactly as it was.
pub(crate) fn run(mut app: App) -> io::Result<()> {
    let guard = TerminalGuard::enable(CrosstermSwitch::new(io::stdout()), &TerminalMode::TUI)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        terminal.draw(|frame| app.render(frame))?;
        // The only input source. Each event is applied before the next draw.
        if app.handle(&event::read()?) == Flow::Quit {
            break;
        }
    }
    drop(terminal);
    guard.restore()
}

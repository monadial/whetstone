//! Bracketed paste in a real terminal: `whetstone` turns it on at start and
//! off again at every exit, a real panic included, and a real paste goes
//! into the search box as text, never as keys.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

use rexpect::session::PtySession;
use tempfile::TempDir;

use crate::pty::{LAUNCH, LAUNCH_IN_BACKGROUND, Transcript, quit_with, read_pid, start};
use crate::pty_terminal::{LAUNCH_AND_CRASH, without_pending_input};

/// A terminal mode the TUI turns on at start and must turn off at every
/// exit, with the escape sequences that turn it on and off.
#[derive(Debug, Clone, Copy)]
pub(super) struct Mode {
    name: &'static str,
    pub(super) on: &'static str,
    off: &'static str,
}

/// The modes these tests check, in the order the guard turns them on, so
/// the guard's order shows. Raw mode writes no escape sequence, so the
/// shell's records of the modes cover it instead.
pub(super) const MODES: &[Mode] = &[
    Mode {
        name: "the alternate screen",
        on: "\x1b[?1049h",
        off: "\x1b[?1049l",
    },
    Mode {
        name: "the hidden cursor",
        on: "\x1b[?25l",
        off: "\x1b[?25h",
    },
    Mode {
        name: "bracketed paste",
        on: "\x1b[?2004h",
        off: "\x1b[?2004l",
    },
];

/// A way `whetstone` ends. Each must turn every mode off.
#[derive(Debug, Clone, Copy)]
pub(super) enum Exit {
    /// `q` on home.
    Quit,
    /// The reserved Ctrl+C.
    CtrlC,
    /// SIGTERM from outside the terminal.
    Signal,
    /// A real panic right after the first draw, from the crash trigger that
    /// only debug builds have.
    Panic,
}

impl Exit {
    /// Every way `whetstone` ends, so the checks that loop over it cover a
    /// new one too. [`Exit::code`] does not compile until a new one has a
    /// code, as a reminder to add it here.
    ///
    /// The crash trigger panics right after the first draw, so a panic can
    /// never follow a suspend: `tests/it/pty_suspend.rs` leaves it out.
    pub(super) const ALL: [Self; 4] = [Self::Quit, Self::CtrlC, Self::Signal, Self::Panic];

    /// The exit code `whetstone` ends with this way.
    pub(super) fn code(self) -> i32 {
        match self {
            Self::Quit | Self::CtrlC => 0,
            // The shell convention for a process ended by SIGTERM: 128 + 15.
            Self::Signal => 143,
            // Rust's exit code for a process whose main thread panicked.
            Self::Panic => 101,
        }
    }

    /// Whether this build can end this way: release builds compile the
    /// crash trigger out.
    fn in_this_build(self) -> bool {
        match self {
            Self::Quit | Self::CtrlC | Self::Signal => true,
            Self::Panic => cfg!(debug_assertions),
        }
    }

    /// Runs `whetstone` in a pseudo-terminal until it draws home, lets it
    /// end this way, and returns everything the terminal received, with the
    /// exit code.
    fn run(self) -> Transcript {
        let home = TempDir::new().expect("create a temporary home");
        let launch = match self {
            Self::Quit | Self::CtrlC => LAUNCH,
            Self::Signal => LAUNCH_IN_BACKGROUND,
            Self::Panic => LAUNCH_AND_CRASH,
        };
        let mut session = start(launch, &[("HOME", home.path())]);
        let transcript = quit_with(&mut session, |session| self.end(session, home.path()));
        match self {
            Self::Quit | Self::CtrlC | Self::Signal => transcript,
            // The panic comes before whetstone first polls the terminal: see
            // `without_pending_input`.
            Self::Panic => without_pending_input(&transcript.0),
        }
    }

    /// Ends the `whetstone` running in `session` this way. A signal goes to
    /// the process ID its launch wrote to `pid` in `home`.
    pub(super) fn end(self, session: &mut PtySession, home: &Path) {
        match self {
            Self::Quit => {
                session.send("q").expect("send q");
                session.flush().expect("flush q");
            }
            Self::CtrlC => session.send_control('c').expect("send Ctrl+C"),
            Self::Signal => {
                let pid = read_pid(&home.join("pid")).expect("whetstone's process ID");
                let kill = Command::new("kill")
                    .args(["-TERM", &pid])
                    .status()
                    .expect("run kill");
                assert!(kill.success(), "kill -TERM {pid}");
            }
            // The crash trigger fires on its own, right after the first draw.
            Self::Panic => {}
        }
    }
}

impl Transcript {
    /// Asserts that `whetstone` turned every one of [`MODES`] on, and that
    /// each was off again by the time it exited, both in the guard's order.
    fn assert_modes_on_then_off(&self) {
        assert_modes_on_then_off_in(self.printed_by_whetstone());
    }

    /// Everything the terminal received from `whetstone`: the shell prints
    /// nothing between its first record of the modes and the exit code.
    fn printed_by_whetstone(&self) -> &str {
        let (_, run) = self
            .0
            .split_once("modes end")
            .expect("the modes are recorded before whetstone runs");
        let (run, _) = run.split_once("exit code:").expect("whetstone exits");
        run
    }
}

/// Asserts that `printed` turns every one of [`MODES`] on in the guard's
/// order, and then turns each off again in reverse order, as the guard
/// restores them.
pub(super) fn assert_modes_on_then_off_in(printed: &str) {
    for mode in MODES {
        assert!(
            printed.contains(mode.on),
            "{} is turned on:\n{printed:?}",
            mode.name
        );
        let since_on = printed.rsplit_once(mode.on).map_or("", |(_, rest)| rest);
        assert!(
            since_on.contains(mode.off),
            "{} is turned off again:\n{printed:?}",
            mode.name
        );
    }
    let mut since_outer_on = printed;
    for mode in MODES {
        let turned_on = since_outer_on.split_once(mode.on);
        assert!(
            turned_on.is_some(),
            "{} is turned on in the guard's order:\n{printed:?}",
            mode.name
        );
        since_outer_on = turned_on.map_or("", |(_, rest)| rest);
    }
    // Ratatui shows the cursor itself too: before the guard restores on the
    // way out, and after the panic message as a panic unwinds. So each
    // mode's last turning off is looked for before the last turning off of
    // the mode turned on before it.
    let mut until_outer_off = printed;
    for mode in MODES {
        let turned_off = until_outer_off.rsplit_once(mode.off);
        assert!(
            turned_off.is_some(),
            "{} is turned off in the guard's reverse order:\n{printed:?}",
            mode.name
        );
        until_outer_off = turned_off.map_or("", |(rest, _)| rest);
    }
}

#[test]
fn the_modes_are_on_while_whetstone_runs_and_off_after_every_exit() {
    for exit in Exit::ALL.into_iter().filter(|exit| exit.in_this_build()) {
        let transcript = exit.run();

        transcript.assert_exit_code(exit.code());
        transcript.assert_modes_on_then_off();
    }
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "release builds compile the crash trigger out"
)]
fn the_modes_are_on_while_whetstone_runs_and_off_before_a_real_panic_message_prints() {
    let transcript = Exit::Panic.run();

    transcript.assert_exit_code(Exit::Panic.code());
    // Raw mode writes no escape sequence: the shell's records of the modes
    // show it was turned off.
    transcript.assert_terminal_restored();
    let (before_message, _) = transcript
        .printed_by_whetstone()
        .split_once("panicked at")
        .expect("the panic message prints");
    assert_modes_on_then_off_in(before_message);
}

/// From home: open the picker on the showcase project, move past its five
/// areas onto "Preserve payment retry identity", open it in the viewer,
/// and open the search box.
pub(super) const SEARCH_RETRY_IDENTITY: &str = "\rjjjjjjjjj\r/";

/// What a terminal sends for a paste of `text` with bracketed paste on.
pub(super) fn bracketed(text: &str) -> String {
    format!("\x1b[200~{text}\x1b[201~")
}

/// Waits until the search box shows `text`. Ratatui moves the cursor over
/// cells that did not change, so `text` must be one word.
fn wait_for(session: &mut PtySession, text: &str) {
    let drawn = session.exp_string(text);
    assert!(drawn.is_ok(), "{text:?} is drawn: {drawn:?}");
}

#[test]
fn a_real_paste_with_a_line_break_neither_searches_nor_goes_back_and_is_never_logged() {
    let home = TempDir::new().expect("create a temporary home");
    let state = TempDir::new().expect("create a temporary state directory");
    let mut session = start(
        LAUNCH,
        &[("HOME", home.path()), ("XDG_STATE_HOME", state.path())],
    );
    wait_for(&mut session, "Quit");
    session
        .send(SEARCH_RETRY_IDENTITY)
        .expect("open the search");

    // Terminals send a pasted line break as `\r`, the same byte as Enter,
    // and `q` goes back from the viewer.
    session
        .send(&bracketed("alpha\rqj:omega"))
        .expect("paste into the search box");
    session.flush().expect("flush the paste");

    // Still in the search box, with all of the paste typed into it.
    wait_for(&mut session, "qj:omega");
    session.send("\r").expect("press Enter");
    session.flush().expect("flush Enter");
    // Searching for the whole paste names it, on the viewer.
    wait_for(&mut session, "qj:omega\"");
    session.send_control('c').expect("send Ctrl+C");
    let transcript = Transcript(session.exp_eof().expect("shell exits"));

    transcript.assert_exit_code(0);
    let log = std::fs::read_to_string(state.path().join("whetstone/whetstone.log"))
        .expect("read the log file");
    assert!(log.contains("whetstone quit"), "log:\n{log}");
    assert!(
        !log.contains("alpha") && !log.contains("omega"),
        "pasted text is never logged:\n{log}"
    );
}

//! Bracketed paste in a real terminal: `whetstone` turns it on at start and
//! off again at every exit, and a real paste goes into the search box as
//! text, never as keys.
//!
//! A panic turns the modes off through the same terminal guard, which
//! `tests/panic_restore.rs` tests through the guard's interface.
#![cfg(unix)]

use std::process::Command;

use rexpect::session::PtySession;
use tempfile::TempDir;

use crate::pty::{LAUNCH, LAUNCH_IN_BACKGROUND, Transcript, quit_with, read_pid, start};

/// A terminal mode the TUI turns on at start and must turn off at every
/// exit, with the escape sequences that turn it on and off.
#[derive(Debug, Clone, Copy)]
struct Mode {
    name: &'static str,
    on: &'static str,
    off: &'static str,
}

/// The modes these tests check.
const MODES: &[Mode] = &[Mode {
    name: "bracketed paste",
    on: "\x1b[?2004h",
    off: "\x1b[?2004l",
}];

/// A way `whetstone` ends. Each must turn every mode off.
#[derive(Debug, Clone, Copy)]
enum Exit {
    /// `q` on home.
    Quit,
    /// The reserved Ctrl+C.
    CtrlC,
    /// SIGTERM from outside the terminal.
    Signal,
}

impl Exit {
    /// Runs `whetstone` in a pseudo-terminal until it draws home, ends it
    /// this way, and returns everything the terminal received, with the
    /// exit code.
    fn run(self) -> Transcript {
        let home = TempDir::new().expect("create a temporary home");
        let launch = match self {
            Self::Quit | Self::CtrlC => LAUNCH,
            Self::Signal => LAUNCH_IN_BACKGROUND,
        };
        let mut session = start(launch, &[("HOME", home.path())]);
        quit_with(&mut session, |session| match self {
            Self::Quit => {
                session.send("q").expect("send q");
                session.flush().expect("flush q");
            }
            Self::CtrlC => session.send_control('c').expect("send Ctrl+C"),
            Self::Signal => {
                let pid = read_pid(&home.path().join("pid")).expect("whetstone's process ID");
                let kill = Command::new("kill")
                    .args(["-TERM", &pid])
                    .status()
                    .expect("run kill");
                assert!(kill.success(), "kill -TERM {pid}");
            }
        })
    }
}

impl Transcript {
    /// Asserts that `whetstone` turned every one of [`MODES`] on, and that
    /// each was off again by the time it exited.
    fn assert_modes_on_then_off(&self) {
        let (_, run) = self
            .0
            .split_once("modes end")
            .expect("the modes are recorded before whetstone runs");
        let (run, _) = run.split_once("exit code:").expect("whetstone exits");
        for mode in MODES {
            assert!(
                run.contains(mode.on),
                "{} is turned on at start:\n{run:?}",
                mode.name
            );
            let since_on = run.rsplit_once(mode.on).map_or("", |(_, rest)| rest);
            assert!(
                since_on.contains(mode.off),
                "{} is turned off before whetstone exits:\n{run:?}",
                mode.name
            );
        }
    }
}

#[test]
fn the_modes_are_on_while_whetstone_runs_and_off_after_q() {
    let transcript = Exit::Quit.run();

    transcript.assert_exit_code(0);
    transcript.assert_modes_on_then_off();
}

#[test]
fn the_modes_are_on_while_whetstone_runs_and_off_after_ctrl_c() {
    let transcript = Exit::CtrlC.run();

    transcript.assert_exit_code(0);
    transcript.assert_modes_on_then_off();
}

#[test]
fn the_modes_are_on_while_whetstone_runs_and_off_after_a_termination_signal() {
    let transcript = Exit::Signal.run();

    transcript.assert_exit_code(143);
    transcript.assert_modes_on_then_off();
}

/// From home: open the picker on the showcase project, move past its five
/// areas onto "Preserve payment retry identity", open it in the viewer,
/// and open the search box.
const SEARCH_RETRY_IDENTITY: &str = "\rjjjjjjjjj\r/";

/// What a terminal sends for a paste of `text` with bracketed paste on.
fn bracketed(text: &str) -> String {
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

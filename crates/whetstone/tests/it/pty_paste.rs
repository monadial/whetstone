//! Bracketed paste in a real terminal: `whetstone` turns it on at start and
//! off again at every exit, a real panic included, and a real paste goes
//! into the search box as text, never as keys.
#![cfg(unix)]

use std::process::Command;

use rexpect::session::PtySession;
use tempfile::TempDir;

use crate::pty::{LAUNCH, LAUNCH_IN_BACKGROUND, Transcript, quit_with, read_pid, start};
use crate::pty_terminal::{LAUNCH_AND_CRASH, without_pending_input};

/// A terminal mode the TUI turns on at start and must turn off at every
/// exit, with the escape sequences that turn it on and off.
#[derive(Debug, Clone, Copy)]
struct Mode {
    name: &'static str,
    on: &'static str,
    off: &'static str,
}

/// The modes these tests check, in the order the guard turns them on. The
/// alternate screen is here so the order shows: bracketed paste, turned on
/// last, is turned off first. Raw mode writes no escape sequence, and
/// Ratatui hides and shows the cursor on its own as well, so neither shows
/// the guard's order.
const MODES: &[Mode] = &[
    Mode {
        name: "the alternate screen",
        on: "\x1b[?1049h",
        off: "\x1b[?1049l",
    },
    Mode {
        name: "bracketed paste",
        on: "\x1b[?2004h",
        off: "\x1b[?2004l",
    },
];

/// A way `whetstone` ends. Each must turn every mode off.
#[derive(Debug, Clone, Copy)]
enum Exit {
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
    /// Runs `whetstone` in a pseudo-terminal until it draws home, ends it
    /// this way, and returns everything the terminal received, with the
    /// exit code.
    fn run(self) -> Transcript {
        let home = TempDir::new().expect("create a temporary home");
        let launch = match self {
            Self::Quit | Self::CtrlC => LAUNCH,
            Self::Signal => LAUNCH_IN_BACKGROUND,
            Self::Panic => LAUNCH_AND_CRASH,
        };
        let mut session = start(launch, &[("HOME", home.path())]);
        let transcript = quit_with(&mut session, |session| match self {
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
            // The crash trigger fires on its own, right after the first draw.
            Self::Panic => {}
        });
        match self {
            Self::Quit | Self::CtrlC | Self::Signal => transcript,
            Self::Panic => without_pending_input(&transcript.0),
        }
    }
}

impl Transcript {
    /// Asserts that `whetstone` turned every one of [`MODES`] on, in order,
    /// and that each was off again by the time it exited, turned off in
    /// reverse order, as the guard restores them.
    fn assert_modes_on_then_off(&self) {
        let run = self.printed_by_whetstone();
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
        let first_on = MODES.iter().map(|mode| run.find(mode.on));
        assert!(
            first_on.is_sorted(),
            "the modes are turned on in order:\n{run:?}"
        );
        let last_off = MODES.iter().rev().map(|mode| run.rfind(mode.off));
        assert!(
            last_off.is_sorted(),
            "the modes are turned off in reverse order:\n{run:?}"
        );
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

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "release builds compile the crash trigger out"
)]
fn the_modes_are_on_while_whetstone_runs_and_off_before_a_real_panic_message_prints() {
    let transcript = Exit::Panic.run();

    // Rust's exit code for a process whose main thread panicked.
    transcript.assert_exit_code(101);
    transcript.assert_modes_on_then_off();
    // Raw mode, which the guard turns off last, writes no escape sequence:
    // the shell's records of the modes show it.
    transcript.assert_terminal_restored();
    let printed = transcript.printed_by_whetstone();
    let message = printed
        .find("panicked at")
        .expect("the panic message prints");
    for mode in MODES {
        let off = printed.rfind(mode.off);
        assert!(
            off.is_some_and(|off| off < message),
            "{} is turned off before the panic message prints:\n{printed:?}",
            mode.name
        );
    }
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

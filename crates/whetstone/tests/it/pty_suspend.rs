//! Suspend and resume in a real terminal: Ctrl+Z, or SIGTSTP from outside,
//! gives the terminal back and stops `whetstone`, and `fg` brings back the
//! same screen, with every mode on again, at the window's current size.
//!
//! `whetstone` runs under job control, as in a learner's shell: once it
//! stops, the shell records the terminal's modes (`stty -a`) and brings it
//! back with `fg`. That shell is dash, which is `sh` on Linux and ships with
//! macOS, because macOS's `sh`, bash 3.2, does not go on with a script while
//! its foreground job is stopped.
#![cfg(unix)]

use std::process::Command;

use tempfile::TempDir;

use crate::pty::{Transcript, start};
use crate::pty_terminal::{Recording, recorded, resize};

/// Runs `whetstone` under job control, after writing the terminal's device
/// to `$HOME/tty` and its process ID to `$HOME/pid`, so the test can resize
/// the one and signal the other. Once `whetstone` stops, the shell records
/// the terminal's modes, runs `while_suspended`, and brings it back with
/// `fg`, whose exit code is `whetstone`'s.
fn launch_suspend_and_fg(while_suspended: &str) -> String {
    format!(
        r#"tty > "$HOME/tty"
cat > "$HOME/job" <<'JOB'
set -m
sh -c 'echo $$ > "$HOME/pid"; exec "$WHETSTONE"'
echo "modes while suspended:"; stty -a; echo "modes end"
{while_suspended}
fg > /dev/null
JOB
dash "$HOME/job""#
    )
}

/// Runs `whetstone` in the foreground, after writing its process ID to
/// `$HOME/pid`, so the test can signal it.
const LAUNCH_RECORDING_PID: &str = r#"sh -c 'echo $$ > "$HOME/pid"; exec "$WHETSTONE"'"#;

/// A terminal mode the TUI turns on with an escape sequence, with the
/// sequences that turn it on and off. Raw mode has none: the terminal's
/// settings show it.
struct Mode {
    name: &'static str,
    on: &'static str,
    off: &'static str,
}

const MODES: [Mode; 3] = [
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

/// Clears the whole screen, so the next frame is drawn in full.
const CLEAR: &str = "\x1b[2J";

/// From home: open the picker on the showcase project, move past its five
/// areas onto "Preserve payment retry identity", open it in the viewer,
/// and open the search box.
const SEARCH_RETRY_IDENTITY: &str = "\rjjjjjjjjj\r/";

/// What a terminal sends for a paste of `text` with bracketed paste on. A
/// paste goes in as one insertion, so it is drawn as one word.
fn bracketed(text: &str) -> String {
    format!("\x1b[200~{text}\x1b[201~")
}

impl Transcript {
    /// What `whetstone` wrote from its start until it stopped.
    fn until_suspended(&self) -> &str {
        let (_, run) = self
            .0
            .split_once("modes end")
            .expect("the modes are recorded before whetstone runs");
        let (run, _) = run
            .split_once("modes while suspended:")
            .expect("whetstone stops");
        run
    }

    /// What `whetstone` wrote from `fg` until it exited.
    fn since_resumed(&self) -> &str {
        let (_, suspended) = self
            .0
            .split_once("modes while suspended:")
            .expect("whetstone stops");
        let (_, resumed) = suspended
            .split_once("modes end")
            .expect("the modes are recorded while whetstone is stopped");
        let (resumed, _) = resumed.split_once("exit code:").expect("whetstone exits");
        resumed
    }

    /// Asserts that `whetstone` gave the terminal back before it stopped:
    /// its settings are those from before `whetstone` started, and every
    /// mode is off.
    fn assert_given_back_while_suspended(&self) {
        assert_eq!(
            self.section("modes before:"),
            self.section("modes while suspended:"),
            "the terminal's settings are restored while whetstone is stopped"
        );
        assert_modes_on_then_off(self.until_suspended(), "before whetstone stops");
    }

    /// Asserts that after `fg`, `whetstone` turned every mode back on and
    /// cleared the screen to draw it in full, then turned every mode off
    /// again before it exited. Returns what it wrote since `fg`.
    fn assert_taken_back_after_fg(&self) -> &str {
        let resumed = self.since_resumed();
        assert_modes_on_then_off(resumed, "after fg");
        let (_, entered) = resumed
            .split_once(MODES[0].on)
            .expect("the alternate screen is entered again");
        assert!(
            entered.contains(CLEAR),
            "the screen is cleared after fg, to be drawn in full:\n{resumed:?}"
        );
        resumed
    }
}

/// Asserts that every one of [`MODES`] is turned on in `output`, and is off
/// again by its end.
fn assert_modes_on_then_off(output: &str, when: &str) {
    for mode in &MODES {
        assert!(
            output.contains(mode.on),
            "{} is turned on {when}:\n{output:?}",
            mode.name
        );
        let since_on = output.rsplit_once(mode.on).map_or("", |(_, rest)| rest);
        assert!(
            since_on.contains(mode.off),
            "{} is turned off again {when}:\n{output:?}",
            mode.name
        );
    }
}

/// Sends `signal` to the process whose ID the launch wrote to `$HOME/pid`.
fn send_signal(home: &TempDir, signal: &str) {
    let pid = recorded(home.path(), "pid");
    let kill = Command::new("kill")
        .args([&format!("-{signal}"), &pid])
        .status()
        .expect("run kill");
    assert!(kill.success(), "kill -{signal} {pid}");
}

#[test]
fn ctrl_z_gives_the_terminal_back_and_fg_brings_back_the_same_screen() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(&launch_suspend_and_fg(""), &[("HOME", home.path())]);
    let mut recording = Recording::new(&mut session);

    recording.wait_for("Quit");
    recording.send(SEARCH_RETRY_IDENTITY);
    recording.send(&bracketed("omega"));
    recording.wait_for("omega");
    recording.send("\x1a");
    recording.wait_for("modes while suspended:");
    recording.wait_for("modes end");
    // The search box, still open with what was typed, drawn again.
    recording.wait_for("omega");
    recording.send("\x03");
    let transcript = recording.finish();

    transcript.assert_exit_code(0);
    transcript.assert_given_back_while_suspended();
    let resumed = transcript.assert_taken_back_after_fg();
    assert!(
        resumed.contains("Preserve payment retry identity"),
        "the viewer is drawn again after fg:\n{resumed:?}"
    );
    transcript.assert_terminal_restored();
}

#[test]
fn sigtstp_from_outside_gives_the_terminal_back_as_ctrl_z_does() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(&launch_suspend_and_fg(""), &[("HOME", home.path())]);
    let mut recording = Recording::new(&mut session);

    recording.wait_for("Quit");
    send_signal(&home, "TSTP");
    recording.wait_for("modes while suspended:");
    recording.wait_for("modes end");
    recording.wait_for("Quit");
    recording.send("q");
    let transcript = recording.finish();

    transcript.assert_exit_code(0);
    transcript.assert_given_back_while_suspended();
    transcript.assert_taken_back_after_fg();
    transcript.assert_terminal_restored();
}

#[test]
fn a_resize_while_suspended_is_honoured_after_fg() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(
        &launch_suspend_and_fg("stty rows 10 cols 40"),
        &[("HOME", home.path())],
    );
    let mut recording = Recording::new(&mut session);

    recording.wait_for("Quit");
    recording.send("\x1a");
    recording.wait_for("modes while suspended:");
    recording.wait_for("modes end");
    // Drawn at the size the window has now.
    recording.wait_for("40x10");
    // Under the notice keys do nothing: Enter on home would open Practice.
    recording.send("j\r");
    resize(&recorded(home.path(), "tty"), 80, 24);
    recording.wait_for("Quit");
    recording.send("q");
    let transcript = recording.finish();

    transcript.assert_exit_code(0);
    transcript.assert_given_back_while_suspended();
    transcript.assert_taken_back_after_fg();
    transcript.assert_terminal_restored();
}

#[test]
fn a_sigcont_with_no_stop_before_it_redraws_and_changes_nothing() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH_RECORDING_PID, &[("HOME", home.path())]);
    let mut recording = Recording::new(&mut session);

    recording.wait_for("Quit");
    recording.send("j");
    send_signal(&home, "CONT");
    // Drawn again in full, with Settings still focused.
    recording.wait_for(CLEAR);
    recording.wait_for("Quit");
    recording.send("\r");
    recording.wait_for("Generator");
    recording.send("\x03");
    let transcript = recording.finish();

    transcript.assert_exit_code(0);
    transcript.assert_terminal_restored();
}

#[test]
fn a_termination_signal_while_suspended_ends_whetstone_without_taking_the_terminal_back() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(
        &launch_suspend_and_fg(r#"kill -TERM "$(cat "$HOME/pid")""#),
        &[("HOME", home.path())],
    );
    let mut recording = Recording::new(&mut session);

    recording.wait_for("Quit");
    recording.send("\x1a");
    let transcript = recording.finish();

    // The shell convention for a process ended by SIGTERM: 128 + 15.
    transcript.assert_exit_code(143);
    transcript.assert_given_back_while_suspended();
    let resumed = transcript.since_resumed();
    assert!(
        !resumed.contains(MODES[0].on),
        "the alternate screen is not entered again after fg:\n{resumed:?}"
    );
    transcript.assert_terminal_restored();
}

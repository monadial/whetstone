//! What a real terminal sees beyond quitting: resizing, signals from
//! outside, a real panic, and the modes Whetstone never turns on. Each test
//! runs the real binary in a pseudo-terminal through the harness in
//! `pty.rs`.
#![cfg(unix)]

use std::fs::File;
use std::path::Path;
use std::process::Command;

use rexpect::session::PtySession;
use tempfile::TempDir;

use crate::pty::{Transcript, quit_with, start};

const LEAVE_ALTERNATE_SCREEN: &str = "\x1b[?1049l";

/// Runs `whetstone` in the foreground, attached to the terminal.
const LAUNCH: &str = r#""$WHETSTONE""#;

/// Runs `whetstone` in the foreground, attached to the terminal, as `exec`
/// from a shell that first writes the terminal's device to `$HOME/tty`, so
/// the test can resize it, and its process ID to `$HOME/pid`, so the test
/// can signal it.
const LAUNCH_RECORDING_TTY_AND_PID: &str = r#"tty > "$HOME/tty"
sh -c 'echo $$ > "$HOME/pid"; exec "$WHETSTONE"'"#;

/// Runs a debug build of `whetstone` with its crash trigger set, so it
/// panics right after its first draw.
const LAUNCH_AND_CRASH: &str = r#"WHETSTONE_CRASH_AFTER_FIRST_DRAW=1 "$WHETSTONE""#;

#[test]
fn below_50x12_the_notice_shows_and_growing_back_brings_the_screen_back() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH_RECORDING_TTY_AND_PID, &[("HOME", home.path())]);
    let mut recording = Recording::new(&mut session);

    // Ratatui moves the cursor between words, so wait for single words.
    // Settings, not the home screen the app starts on, so a screen that
    // came back from the start would not pass.
    recording.wait_for("Quit");
    recording.send("j\r");
    recording.wait_for("Generator");
    let tty = recorded(home.path(), "tty");
    resize(&tty, 40, 10);
    let notice = recording.wait_for("40x10");
    assert!(
        notice.contains("Window") && notice.contains("small"),
        "the window too small notice is drawn:\n{notice:?}"
    );
    resize(&tty, 80, 24);
    let settings_again = recording.wait_for("Generator");
    assert!(
        settings_again.contains("Provider"),
        "Settings is drawn again:\n{settings_again:?}"
    );
    recording.send("\x03");
    let transcript = recording.finish();

    transcript.assert_exit_code(0);
    transcript.assert_terminal_restored();
}

#[test]
fn sigint_from_outside_quits_and_restores_the_terminal() {
    // The shell convention for a process ended by SIGINT: 128 + 2.
    assert_signal_quits_and_restores("INT", 130);
}

#[test]
fn sighup_from_outside_quits_and_restores_the_terminal() {
    // The shell convention for a process ended by SIGHUP: 128 + 1.
    assert_signal_quits_and_restores("HUP", 129);
}

fn assert_signal_quits_and_restores(signal: &str, code: i32) {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH_RECORDING_TTY_AND_PID, &[("HOME", home.path())]);

    let transcript = quit_with(&mut session, |_| {
        let pid = recorded(home.path(), "pid");
        let kill = Command::new("kill")
            .args([&format!("-{signal}"), &pid])
            .status()
            .expect("run kill");
        assert!(kill.success(), "kill -{signal} {pid}");
    });

    transcript.assert_exit_code(code);
    transcript.assert_terminal_restored();
}

#[test]
fn no_mouse_mode_or_keyboard_enhancement_is_ever_turned_on() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH, &[("HOME", home.path())]);
    let mut recording = Recording::new(&mut session);

    // Visit every screen and overlay: the picker, a change in the viewer and
    // its search box, back home, then Settings with key help and the
    // palette over it.
    recording.wait_for("Quit");
    recording.send("\rjjjjjjjjj\r/zzqq\rqqj\r??:");
    recording.wait_for("Leave");
    recording.send("\x03");
    let transcript = recording.finish();

    transcript.assert_exit_code(0);
    transcript.assert_terminal_restored();
    for shown in [
        "Observability",
        "\"zzqq\"",
        "Generator",
        "Everywhere",
        "Leave",
    ] {
        assert!(
            transcript.0.contains(shown),
            "the tour drew {shown:?}:\n{:?}",
            transcript.0
        );
    }
    let sequences = control_sequences(&transcript.0);
    assert!(
        sequences.contains(&("?1049", 'h')),
        "the alternate screen's private mode is found: {sequences:?}"
    );
    let mouse: Vec<_> = sequences
        .iter()
        .filter(|sequence| sets_or_resets_a_mouse_mode(sequence))
        .collect();
    assert!(mouse.is_empty(), "mouse modes written: {mouse:?}");
    let keyboard: Vec<_> = sequences
        .iter()
        .filter(|sequence| enhances_the_keyboard(sequence))
        .collect();
    assert!(
        keyboard.is_empty(),
        "keyboard enhancements written: {keyboard:?}"
    );
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "release builds compile the crash trigger out"
)]
fn a_real_panic_restores_the_terminal_before_the_panic_message_prints() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH_AND_CRASH, &[("HOME", home.path())]);

    let transcript = without_pending_input(&session.exp_eof().expect("shell exits"));

    // Rust's exit code for a process whose main thread panicked.
    transcript.assert_exit_code(101);
    transcript.assert_terminal_restored();
    let restored = transcript
        .0
        .rfind(LEAVE_ALTERNATE_SCREEN)
        .expect("whetstone left the alternate screen");
    let message = transcript.0.find("panicked at");
    assert!(
        message.is_some_and(|message| message > restored),
        "the panic message prints after the terminal is restored:\n{:?}",
        transcript.0
    );
    // Out of raw mode the terminal turns each `\n` into `\r\n`; raw mode
    // would leave a bare `\n`.
    assert!(
        transcript.0.contains(
            "crashing after the first draw, as WHETSTONE_CRASH_AFTER_FIRST_DRAW asks\r\n"
        ),
        "the panic message prints out of raw mode:\n{:?}",
        transcript.0
    );
}

/// `transcript` as if the shell had read the terminal before its last
/// `stty -a`. When canonical mode comes back, macOS sets the `pendin` flag
/// until something next reads or polls the terminal. Whetstone polls it
/// while running, which clears the flag at once, but the crash trigger fires
/// before the first poll, so the flag waits for the shell's next read. It is
/// the kernel's, not a setting Whetstone changes or restores.
fn without_pending_input(transcript: &str) -> Transcript {
    Transcript(transcript.replace(" pendin ", " -pendin "))
}

/// What `$HOME/{name}` holds, written by the launch before `whetstone`
/// started.
fn recorded(home: &Path, name: &str) -> String {
    std::fs::read_to_string(home.join(name))
        .expect("the launch wrote the file before whetstone started")
        .trim()
        .to_owned()
}

/// Resizes the terminal at `tty` to `cols` by `rows`, as dragging the
/// window's edge does: the kernel tells the terminal's foreground processes
/// with SIGWINCH.
fn resize(tty: &str, cols: u16, rows: u16) {
    let terminal = File::options()
        .read(true)
        .write(true)
        .open(tty)
        .expect("open the terminal");
    let stty = Command::new("stty")
        .args(["rows", &rows.to_string(), "cols", &cols.to_string()])
        .stdin(terminal)
        .status()
        .expect("run stty");
    assert!(stty.success(), "stty rows {rows} cols {cols}");
}

/// A session that keeps everything the shell printed, including what each
/// wait read past, so a check can cover the whole session.
struct Recording<'a> {
    session: &'a mut PtySession,
    printed: String,
}

impl<'a> Recording<'a> {
    fn new(session: &'a mut PtySession) -> Self {
        Self {
            session,
            printed: String::new(),
        }
    }

    /// Waits for `needle` and returns what was printed since the last wait,
    /// up to and including it.
    fn wait_for(&mut self, needle: &str) -> String {
        let before = self
            .session
            .exp_string(needle)
            .expect("the awaited text is drawn");
        let read = format!("{before}{needle}");
        self.printed.push_str(&read);
        read
    }

    fn send(&mut self, keys: &str) {
        self.session.send(keys).expect("send keys");
        self.session.flush().expect("flush keys");
    }

    /// Reads until the shell exits.
    fn finish(self) -> Transcript {
        let rest = self.session.exp_eof().expect("shell exits");
        Transcript(self.printed + &rest)
    }
}

/// Every control sequence (`CSI …`) in `output`: its parameters, with any
/// private marker such as `?` or `>`, and its final byte.
fn control_sequences(output: &str) -> Vec<(&str, char)> {
    output
        .split("\x1b[")
        .skip(1)
        .filter_map(|sequence| {
            let end = sequence.find(|c: char| ('@'..='~').contains(&c))?;
            let (parameters, rest) = sequence.split_at(end);
            Some((parameters, rest.chars().next()?))
        })
        .collect()
}

/// The private modes that report the mouse: X10, normal, highlight,
/// button-event and any-event tracking, and the UTF-8, SGR, urxvt and
/// SGR-pixel encodings of its reports.
const MOUSE_MODES: [u16; 9] = [9, 1000, 1001, 1002, 1003, 1005, 1006, 1015, 1016];

/// Whether a control sequence sets (`CSI ? … h`) or resets (`CSI ? … l`) a
/// mouse mode.
fn sets_or_resets_a_mouse_mode(&(parameters, end): &(&str, char)) -> bool {
    matches!(end, 'h' | 'l')
        && parameters.strip_prefix('?').is_some_and(|modes| {
            modes
                .split(';')
                .filter_map(|mode| mode.parse::<u16>().ok())
                .any(|mode| MOUSE_MODES.contains(&mode))
        })
}

/// Whether a control sequence turns on a keyboard enhancement: the kitty
/// keyboard protocol's flags, pushed (`CSI > … u`) or set (`CSI = … u`), or
/// xterm's modifyOtherKeys (`CSI > 4 ; … m`).
fn enhances_the_keyboard(&(parameters, end): &(&str, char)) -> bool {
    match end {
        'u' => parameters.starts_with('>') || parameters.starts_with('='),
        'm' => parameters.starts_with(">4"),
        _ => false,
    }
}

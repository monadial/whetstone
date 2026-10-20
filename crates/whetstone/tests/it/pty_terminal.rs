//! What a real terminal sees beyond quitting: resizing, signals from
//! outside, a real panic, and the modes Whetstone never turns on. Each test
//! runs the real binary in a pseudo-terminal through the harness in
//! `pty.rs`.
#![cfg(unix)]

use crate::pty::{Transcript, start};

use tempfile::TempDir;

const LEAVE_ALTERNATE_SCREEN: &str = "\x1b[?1049l";

/// Runs a debug build of `whetstone` with its crash trigger set, so it
/// panics right after its first draw.
const LAUNCH_AND_CRASH: &str = r#"WHETSTONE_CRASH_AFTER_FIRST_DRAW=1 "$WHETSTONE""#;

#[test]
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

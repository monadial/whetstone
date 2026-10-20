//! A panic restores the terminal before the panic message prints. This file
//! holds a single test because it replaces the process-wide panic hook.

#[path = "it/support.rs"]
mod support;

use std::panic;

use whetstone::{TerminalGuard, TerminalMode};

use crate::support::{Log, RecordingSwitch};

#[test]
fn a_panic_restores_every_mode_before_the_panic_message_prints() {
    let log = Log::default();
    // Stands in for the default hook, which prints the panic message.
    let message_log = log.clone();
    panic::set_hook(Box::new(move |_| message_log.push("panic message printed")));

    let guard = TerminalGuard::enable(RecordingSwitch::new(&log), &TerminalMode::TUI)
        .expect("enable modes");
    let outcome = panic::catch_unwind(|| panic!("boom"));
    drop(guard);

    assert!(outcome.is_err(), "the closure panicked");
    assert_eq!(
        log.entries(),
        [
            "enable Raw",
            "enable AlternateScreen",
            "enable HiddenCursor",
            "enable BracketedPaste",
            "disable BracketedPaste",
            "disable HiddenCursor",
            "disable AlternateScreen",
            "disable Raw",
            "panic message printed",
        ],
        "modes are restored first, and only once"
    );
}

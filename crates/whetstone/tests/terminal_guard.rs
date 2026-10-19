//! The terminal guard restores every mode it enabled, through its own
//! interface. Panic restore lives in `panic_restore.rs`, because a panic hook
//! is global to the test process.

mod support;

use whetstone::{TerminalGuard, TerminalMode};

use crate::support::{Log, RecordingSwitch};

const ALL: [TerminalMode; 3] = [
    TerminalMode::Raw,
    TerminalMode::AlternateScreen,
    TerminalMode::HiddenCursor,
];

#[test]
fn dropping_the_guard_restores_every_mode_in_reverse_order() {
    let log = Log::default();

    let guard = TerminalGuard::enable(RecordingSwitch::new(&log), &ALL).expect("enable modes");
    drop(guard);

    assert_eq!(
        log.entries(),
        [
            "enable Raw",
            "enable AlternateScreen",
            "enable HiddenCursor",
            "disable HiddenCursor",
            "disable AlternateScreen",
            "disable Raw",
        ]
    );
}

#[test]
fn an_explicit_restore_happens_once() {
    let log = Log::default();

    let guard = TerminalGuard::enable(RecordingSwitch::new(&log), &ALL).expect("enable modes");
    guard.restore().expect("restore modes");

    assert_eq!(
        log.entries()
            .iter()
            .filter(|entry| entry.starts_with("disable"))
            .count(),
        3,
        "each mode is disabled exactly once: {:?}",
        log.entries()
    );
}

#[test]
fn a_mode_that_fails_to_enable_restores_the_modes_already_enabled() {
    let log = Log::default();
    let switch = RecordingSwitch {
        failing: Some(TerminalMode::HiddenCursor),
        ..RecordingSwitch::new(&log)
    };

    let error = TerminalGuard::enable(switch, &ALL).expect_err("enabling fails");

    assert_eq!(error.to_string(), "HiddenCursor unsupported");
    assert_eq!(
        log.entries(),
        [
            "enable Raw",
            "enable AlternateScreen",
            "disable AlternateScreen",
            "disable Raw",
        ]
    );
}

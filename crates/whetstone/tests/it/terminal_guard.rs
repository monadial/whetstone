//! The terminal guard restores every mode it enabled, and turns the same
//! modes off and back on around a suspend, through its own interface. Panic
//! restore lives in `tests/panic_restore.rs`, because a panic hook is global
//! to the test process.

use whetstone::{TerminalGuard, TerminalMode};

use crate::support::{Log, RecordingSwitch};

#[test]
fn dropping_the_guard_restores_every_mode_in_reverse_order() {
    let log = Log::default();

    let guard = TerminalGuard::enable(RecordingSwitch::new(&log), &TerminalMode::TUI)
        .expect("enable modes");
    drop(guard);

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
        ]
    );
}

#[test]
fn an_explicit_restore_happens_once() {
    let log = Log::default();

    let guard = TerminalGuard::enable(RecordingSwitch::new(&log), &TerminalMode::TUI)
        .expect("enable modes");
    guard.restore().expect("restore modes");

    assert_eq!(
        log.entries()
            .iter()
            .filter(|entry| entry.starts_with("disable"))
            .count(),
        4,
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

    let error = TerminalGuard::enable(switch, &TerminalMode::TUI).expect_err("enabling fails");

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

#[test]
fn suspend_turns_every_mode_off_and_resume_turns_the_same_modes_back_on() {
    let log = Log::default();

    let mut guard = TerminalGuard::enable(RecordingSwitch::new(&log), &TerminalMode::TUI)
        .expect("enable modes");
    guard.suspend().expect("suspend");
    guard.resume().expect("resume");
    drop(guard);

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
            "enable Raw",
            "enable AlternateScreen",
            "enable HiddenCursor",
            "enable BracketedPaste",
            "disable BracketedPaste",
            "disable HiddenCursor",
            "disable AlternateScreen",
            "disable Raw",
        ]
    );
}

#[test]
fn a_guard_dropped_while_suspended_turns_nothing_off_again() {
    let log = Log::default();

    let mut guard = TerminalGuard::enable(RecordingSwitch::new(&log), &TerminalMode::TUI)
        .expect("enable modes");
    guard.suspend().expect("suspend");
    drop(guard);

    assert_eq!(
        log.entries()
            .iter()
            .filter(|entry| entry.starts_with("disable"))
            .count(),
        4,
        "each mode is disabled exactly once: {:?}",
        log.entries()
    );
}

#[test]
fn resume_without_a_suspend_changes_nothing() {
    let log = Log::default();

    let mut guard = TerminalGuard::enable(RecordingSwitch::new(&log), &TerminalMode::TUI)
        .expect("enable modes");
    guard.resume().expect("resume");

    assert_eq!(
        log.entries()
            .iter()
            .filter(|entry| entry.starts_with("enable"))
            .count(),
        4,
        "each mode is enabled exactly once: {:?}",
        log.entries()
    );
}

//! Moving around from the keyboard: vim-like keys and arrows move, Tab and
//! Shift+Tab move focus, Enter and Space activate, `q` goes back or quits
//! from home, and Ctrl+C quits from anywhere.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use whetstone::Flow;

use crate::session::Session;

#[test]
fn j_and_k_move_focus_between_the_home_actions() {
    let mut session = Session::plain(80, 24);
    assert_eq!(
        session.row_with("Practice"),
        "> Practice on the showcase project (arrives with the change picker)"
    );

    session.press("j");
    assert_eq!(session.row_with("Settings"), "> Settings");

    session.press("j k");
    assert_eq!(session.row_with("Settings"), "> Settings");
    assert_eq!(session.row_with("Quit"), "Quit");
}

#[test]
fn arrows_move_like_j_and_k() {
    let mut session = Session::plain(80, 24);

    session.press("<Down> <Down>");
    assert_eq!(session.row_with("Quit"), "> Quit");

    session.press("<Up>");
    assert_eq!(session.row_with("Settings"), "> Settings");
}

#[test]
fn moving_stops_at_either_end_of_a_list() {
    let mut session = Session::plain(80, 24);

    session.press("k");
    assert!(session.row_with("Practice").starts_with("> Practice"));

    session.press("j j j j");
    assert_eq!(session.row_with("Quit"), "> Quit");
}

#[test]
fn tab_and_shift_tab_move_focus_and_wrap_around() {
    let mut session = Session::plain(80, 24);

    session.press("<Tab>");
    assert_eq!(session.row_with("Settings"), "> Settings");

    session.press("<Tab> <Tab>");
    assert!(session.row_with("Practice").starts_with("> Practice"));

    session.press("<S-Tab>");
    assert_eq!(session.row_with("Quit"), "> Quit");
}

#[test]
fn enter_and_space_activate_the_focused_control() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Settings", "Settings is open");

    let mut session = Session::plain(80, 24);
    session.press("j <Space>");
    assert_eq!(session.rows()[1].trim(), "Settings", "Settings is open");

    let mut session = Session::plain(80, 24);
    assert_eq!(session.press("j j <Enter>"), Flow::Quit, "Quit quits");
}

#[test]
fn a_disabled_action_does_nothing_when_activated() {
    let mut session = Session::plain(80, 24);
    let home = session.text();

    assert_eq!(session.press("<Enter> <Space>"), Flow::Continue);
    assert_eq!(session.text(), home);
}

#[test]
fn q_goes_back_to_home_with_focus_where_it_was_and_quits_from_there() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> j j");

    assert_eq!(session.press("q"), Flow::Continue);
    assert_eq!(session.row_with("Settings"), "> Settings");

    assert_eq!(session.press("q"), Flow::Quit);
}

#[test]
fn esc_goes_back_from_settings_but_never_quits_from_home() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");

    session.press("<Esc>");
    assert_eq!(session.row_with("Settings"), "> Settings");

    assert_eq!(session.press("<Esc>"), Flow::Continue);
}

#[test]
fn back_on_settings_returns_to_home() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");

    session.press("<S-Tab>");
    assert_eq!(session.row_with("Back"), "> Back");
    session.press("<Enter>");
    assert_eq!(session.row_with("Settings"), "> Settings");
}

#[test]
fn settings_moves_focus_through_its_settings_and_explains_the_focused_one() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");
    assert!(
        session
            .text()
            .contains("Where Practice will send its requests")
    );

    session.press("j");
    assert!(session.row_with("Jev model").starts_with("> Jev model"));
    assert!(
        session
            .text()
            .contains("Jev assesses your choice and each part of your reasoning"),
        "{}",
        session.text()
    );

    session.press("<Tab>");
    assert!(
        session
            .row_with("Generator model")
            .starts_with("> Generator model")
    );
    assert!(session.text().contains("The generator writes questions"));
}

#[test]
fn nothing_on_settings_is_editable() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");

    for focused in ["Provider", "Jev model", "Generator model", "OpenRouter key"] {
        let before = session.text();
        assert_eq!(session.press("<Enter> <Space>"), Flow::Continue);
        assert_eq!(
            session.text(),
            before,
            "activating {focused} changes nothing"
        );
        session.press("j");
    }
}

#[test]
fn ctrl_c_quits_from_settings() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");

    assert_eq!(session.press("<C-c>"), Flow::Quit);
}

#[test]
fn keys_with_other_modifiers_do_nothing() {
    let mut session = Session::plain(80, 24);
    let home = session.text();

    for event in [
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
    ] {
        assert_eq!(session.handle(&Event::Key(event)), Flow::Continue);
    }
    assert_eq!(session.text(), home);
}

//! Overlays: `?` shows the keys of the current screen, and `:` lists and
//! runs its actions. An overlay owns focus while it is open, and closing it
//! returns focus to the control that opened it.

use whetstone::Flow;

use crate::session::Session;

#[test]
fn key_help_lists_the_keys_of_home() {
    let mut session = Session::plain(80, 24);
    session.press("?");

    assert!(
        session.text().contains("Keys on Home"),
        "{}",
        session.text()
    );
    for (keys, does) in [
        ("j, Down, k, Up", "Move down or up"),
        ("Enter, Space", "Activate the focused control"),
        ("q", "Quit"),
        ("Tab, Shift+Tab", "Next or previous control"),
        ("?", "Show these keys"),
        (":", "List the actions"),
        ("Ctrl+C", "Quit from anywhere"),
        ("Ctrl+Z", "Suspend until fg"),
    ] {
        assert!(
            session
                .overlay_rows()
                .iter()
                .any(|row| row.starts_with(keys) && row.ends_with(does)),
            "{does:?} is {keys:?}:\n{}",
            session.text()
        );
    }
}

#[test]
fn key_help_on_settings_shows_that_q_and_esc_go_back() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> ?");

    assert!(
        session.text().contains("Keys on Settings"),
        "{}",
        session.text()
    );
    let rows = session.overlay_rows();
    assert!(
        rows.iter()
            .any(|row| row.starts_with("q, Esc") && row.ends_with("Back")),
        "{rows:#?}"
    );
    assert!(!rows.iter().any(|row| row.ends_with(" Quit")), "{rows:#?}");
}

#[test]
fn key_help_on_home_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press("?");

    insta::assert_debug_snapshot!("key_help_on_home_120x32", session.frame());
}

#[test]
fn key_help_on_home_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press("?");

    insta::assert_debug_snapshot!("key_help_on_home_80x24", session.frame());
}

#[test]
fn key_help_on_settings_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press("j <Enter> ?");

    insta::assert_debug_snapshot!("key_help_on_settings_120x32", session.frame());
}

#[test]
fn key_help_on_settings_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> ?");

    insta::assert_debug_snapshot!("key_help_on_settings_80x24", session.frame());
}

#[test]
fn esc_q_and_question_mark_each_close_key_help() {
    for close in ["<Esc>", "q", "?"] {
        let mut session = Session::plain(80, 24);
        let home = session.text();
        session.press("?");
        assert_ne!(session.text(), home, "key help is open");

        assert_eq!(
            session.press(close),
            Flow::Continue,
            "{close} closes, never quits"
        );
        assert_eq!(session.text(), home, "after {close}");
    }
}

#[test]
fn closing_key_help_returns_focus_to_the_control_that_opened_it() {
    let mut session = Session::plain(80, 24);
    session.press("j ?");

    // While key help is open it owns the keys: these move nothing on home.
    session.press("j j <Tab> <Enter> <Space>");
    session.press("<Esc>");

    assert_eq!(session.row_with("Settings"), "> Settings");
    session.press("<Enter>");
    assert_eq!(session.rows()[1].trim(), "Settings", "Settings is open");
}

#[test]
fn ctrl_c_quits_from_key_help() {
    let mut session = Session::plain(80, 24);
    session.press("?");

    assert_eq!(session.press("<C-c>"), Flow::Quit);
}

#[test]
fn the_palette_lists_home_practice_settings_help_and_quit() {
    let mut session = Session::plain(80, 24);
    session.press(":");

    assert!(
        session.text().contains("Actions on Home"),
        "{}",
        session.text()
    );
    let rows = session.overlay_rows();
    for (label, does) in [
        ("> Home", "Go to the home screen"),
        ("Practice", "Choose areas and a change"),
        ("Settings", "See what Practice will use"),
        ("Help", "Show the keys for this screen"),
        ("Quit", "Leave Whetstone"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.starts_with(label) && row.ends_with(does)),
            "{label:?}: {rows:#?}"
        );
    }
}

#[test]
fn the_palette_on_home_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press(":");

    insta::assert_debug_snapshot!("palette_on_home_120x32", session.frame());
}

#[test]
fn the_palette_on_home_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(":");

    insta::assert_debug_snapshot!("palette_on_home_80x24", session.frame());
}

#[test]
fn the_palette_on_settings_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press("j <Enter> : j");

    insta::assert_debug_snapshot!("palette_on_settings_120x32", session.frame());
}

#[test]
fn the_palette_on_settings_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> : j");

    insta::assert_debug_snapshot!("palette_on_settings_80x24", session.frame());
}

#[test]
fn the_palette_runs_the_selected_action() {
    // Settings.
    let mut session = Session::plain(80, 24);
    session.press(": j j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Settings", "Settings is open");

    // Home, from Settings, with Space: focus is where home left it.
    session.press(": <Space>");
    assert!(
        session.row_with("Practice").starts_with("> Practice"),
        "home is back:\n{}",
        session.text()
    );

    // Help: key help for the screen the palette was opened on.
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> : j j j j <Enter>");
    assert!(
        session.text().contains("Keys on Settings"),
        "{}",
        session.text()
    );

    // Quit.
    let mut session = Session::plain(80, 24);
    assert_eq!(session.press(": k <Up> <S-Tab> <Enter>"), Flow::Quit);
}

#[test]
fn esc_q_and_colon_each_close_the_palette() {
    for close in ["<Esc>", "q", ":"] {
        let mut session = Session::plain(80, 24);
        let home = session.text();
        session.press(": j");

        assert_eq!(
            session.press(close),
            Flow::Continue,
            "{close} closes, never quits"
        );
        assert_eq!(session.text(), home, "after {close}");
    }
}

#[test]
fn closing_the_palette_returns_focus_to_the_control_that_opened_it() {
    let mut session = Session::plain(80, 24);
    session.press("j j :");

    session.press("j <Tab>");
    session.press("<Esc>");

    assert_eq!(session.row_with("Quit"), "> Quit");
    session.press("k <Enter>");
    assert_eq!(session.rows()[1].trim(), "Settings", "Settings is open");
}

#[test]
fn help_run_from_the_palette_returns_focus_to_the_control_under_both() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> j j");
    let settings = session.text();

    session.press(": j j j j <Enter>");
    session.press("<Esc>");

    assert_eq!(session.text(), settings);
}

#[test]
fn ctrl_c_quits_from_the_palette() {
    let mut session = Session::plain(80, 24);
    session.press(":");

    assert_eq!(session.press("<C-c>"), Flow::Quit);
}

#[test]
fn the_palette_on_settings_offers_back_first() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> j :");

    let rows = session.overlay_rows();
    assert!(
        rows.first().is_some_and(
            |row| row.starts_with("> Back") && row.ends_with("Return to the screen before")
        ),
        "{rows:#?}"
    );
    session.press("<Enter>");
    assert_eq!(
        session.row_with("Settings"),
        "> Settings",
        "home, as it was left"
    );
}

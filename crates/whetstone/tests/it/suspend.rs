//! Ctrl+Z is a reserved key, like Ctrl+C: it asks the UI loop to suspend
//! Whetstone from anywhere, and changes nothing in the app, so `fg` brings
//! back the same screen. The stop itself can only be seen in a real terminal
//! (`tests/it/pty_suspend.rs`).

use whetstone::Flow;

use crate::session::Session;
use crate::viewer::{footer, status};

/// From home: open the picker, move past its five areas onto "Preserve
/// payment retry identity", open it in the viewer, scroll its diff down and
/// type a search without running it.
const SEARCHING_THE_SCROLLED_VIEWER: &str = "<Enter> j j j j j j j j j <Enter> l <C-d> / k e y";

#[test]
fn ctrl_z_suspends_from_home() {
    let mut session = Session::plain(80, 24);

    assert_eq!(session.press("<C-z>"), Flow::Suspend);
}

#[test]
fn ctrl_z_in_the_search_box_suspends_types_nothing_and_keeps_the_screen() {
    let mut session = Session::plain(80, 24);
    session.press(SEARCHING_THE_SCROLLED_VIEWER);
    let before = session.frame();

    assert_eq!(session.press("<C-z>"), Flow::Suspend);
    // On resume the loop reports the terminal's size again.
    session.resize(80, 24);

    assert_eq!(session.frame(), before, "{}", session.text());
    session.press("s");
    assert_eq!(
        status(&session),
        "/keys",
        "still typing into the search box"
    );
}

#[test]
fn ctrl_z_suspends_from_an_overlay_and_leaves_it_open() {
    let mut session = Session::plain(80, 24);
    session.press("j ?");
    let before = session.frame();

    assert_eq!(session.press("<C-z>"), Flow::Suspend);

    assert_eq!(session.frame(), before, "{}", session.text());
}

#[test]
fn ctrl_z_suspends_while_the_window_is_too_small() {
    let mut session = Session::plain(80, 24);
    session.press(SEARCHING_THE_SCROLLED_VIEWER);
    let before = session.frame();
    session.resize(40, 10);

    assert_eq!(session.press("<C-z>"), Flow::Suspend);
    session.resize(80, 24);

    assert_eq!(session.frame(), before, "{}", session.text());
    assert_eq!(footer(&session), "Enter search  Esc cancel");
}

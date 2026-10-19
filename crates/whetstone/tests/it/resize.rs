//! Window sizes: layouts are usable at 120x32, 100x28 and 80x24. Below 50x12
//! a "window too small" notice appears, and resizing back restores the
//! previous screen, selection, scroll position and focus.

use whetstone::Flow;

use crate::session::Session;

#[test]
fn below_50x12_a_window_too_small_notice_appears() {
    for (width, height) in [(49, 12), (50, 11), (30, 8)] {
        let mut session = Session::plain(80, 24);
        session.resize(width, height);

        assert!(
            session.text().contains("Window too small"),
            "at {width}x{height}:\n{}",
            session.text()
        );
    }
    let session = Session::plain(50, 12);
    assert!(
        !session.text().contains("Window too small"),
        "{}",
        session.text()
    );
}

#[test]
fn the_window_too_small_notice_at_40x10() {
    let mut session = Session::plain(80, 24);
    session.resize(40, 10);

    insta::assert_debug_snapshot!("window_too_small_40x10", session.frame());
}

#[test]
fn resizing_back_restores_the_screen_and_focus() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> j j");
    let before = session.frame();

    session.resize(40, 10);
    session.resize(80, 24);

    assert_eq!(session.frame(), before);
}

#[test]
fn resizing_back_restores_the_palette_and_its_selection() {
    let mut session = Session::plain(120, 32);
    session.press("j : j j");
    let before = session.frame();

    session.resize(20, 6);
    session.resize(120, 32);

    assert_eq!(session.frame(), before);
    session.press("<Esc>");
    assert_eq!(
        session.row_with("Settings"),
        "> Settings",
        "focus under it too"
    );
}

#[test]
fn resizing_back_restores_where_key_help_was_scrolled() {
    // At the smallest usable size key help does not fit, so it scrolls.
    let mut session = Session::plain(50, 12);
    session.press("?");
    let top = session.frame();
    session.press("j");
    let scrolled = session.frame();
    assert_ne!(scrolled, top, "key help scrolled:\n{}", session.text());

    session.resize(49, 11);
    session.resize(50, 12);

    assert_eq!(session.frame(), scrolled);
}

#[test]
fn key_help_scrolls_no_further_than_its_last_row() {
    let mut session = Session::plain(50, 12);
    session.press("?");
    session.press("j j j j j j j j j j j j");
    let end = session.frame();
    assert!(
        session.text().contains("Quit from anywhere"),
        "{}",
        session.text()
    );

    session.press("k");
    assert_ne!(session.frame(), end, "one k scrolls straight back up");
}

#[test]
fn resizing_between_usable_sizes_keeps_everything_too() {
    let mut session = Session::plain(120, 32);
    session.press("j <Enter> <Tab> <Tab> :");
    let before = session.frame();

    session.resize(80, 24);
    session.resize(100, 28);
    session.resize(120, 32);

    assert_eq!(session.frame(), before);
}

#[test]
fn keys_do_nothing_while_the_window_is_too_small() {
    let mut session = Session::plain(80, 24);
    let home = session.frame();

    session.resize(40, 10);
    assert_eq!(session.press("j <Enter> : ? q <Esc> <Tab>"), Flow::Continue);
    session.resize(80, 24);

    assert_eq!(session.frame(), home);
}

#[test]
fn ctrl_c_quits_while_the_window_is_too_small() {
    let mut session = Session::plain(80, 24);
    session.resize(40, 10);

    assert_eq!(session.press("<C-c>"), Flow::Quit);
}

/// Each screen and overlay, the keys that reach it from home, and text that
/// must all show when it fits whole: its first and last rows among them.
const VIEWS: [(&str, &str, &[&str]); 6] = [
    (
        "home",
        "",
        &[
            "█",
            "whetstone by Monadial",
            "> Practice on the showcase project (arrives with the change picker)",
            "Quit",
            "your credits.",
            "? keys  : actions  q quit",
        ],
    ),
    (
        "Settings",
        "j <Enter>",
        &[
            "No config found at ~/.config/whetstone/config.toml",
            "> Provider",
            "OpenRouter key   not set (OPENROUTER_API_KEY)",
            "Back",
            "In config.toml: provider = \"openrouter\"",
            "? keys  : actions  q back",
        ],
    ),
    (
        "key help on home",
        "?",
        &[
            "Keys on Home",
            "Move down or up",
            "Quit from anywhere",
            "Esc close",
        ],
    ),
    (
        "key help on Settings",
        "j <Enter> ?",
        &[
            "Keys on Settings",
            "Move down or up",
            "Quit from anywhere",
            "Esc close",
        ],
    ),
    (
        "the palette on home",
        ":",
        &["Actions on Home", "> Home", "Leave Whetstone", "Enter run"],
    ),
    (
        "the palette on Settings",
        "j <Enter> :",
        &[
            "Actions on Settings",
            "> Home",
            "Leave Whetstone",
            "Enter run",
        ],
    ),
];

#[test]
fn every_screen_and_overlay_fits_whole_at_120x32_100x28_and_80x24() {
    for (width, height) in [(120, 32), (100, 28), (80, 24)] {
        for (view, keys, shown) in VIEWS {
            let mut session = Session::plain(width, height);
            session.press(keys);

            for text in shown {
                assert!(
                    session.text().contains(text),
                    "{view} at {width}x{height} shows {text:?}:\n{}",
                    session.text()
                );
            }
        }
    }
}

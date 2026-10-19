//! App seam: build the app from injected inputs, feed it terminal events and
//! snapshot the frames it draws on ratatui's test backend.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use whetstone::Flow;

use crate::colors::{ansi256_token_colors, colors_used, dark_token_colors};
use crate::session::{Session, Setup};

/// A terminal that reports 24-bit colour.
const TRUECOLOR: &[(&str, &str)] = &[("COLORTERM", "truecolor")];
/// The same terminal with colour turned off, which wins over 24-bit colour.
const TRUECOLOR_TURNED_OFF: &[(&str, &str)] = &[("COLORTERM", "truecolor"), ("NO_COLOR", "1")];

/// The app as it starts, without a config file, in a `width` by `height`
/// terminal whose environment is `variables`.
fn app_in(variables: &[(&str, &str)], width: u16, height: u16) -> Session {
    Session::start(
        Setup {
            variables,
            ..Setup::default()
        },
        width,
        height,
    )
}

#[test]
fn home_in_colour_shows_the_mark_the_actions_and_what_practice_will_send_at_120x32() {
    let session = app_in(TRUECOLOR, 120, 32);

    insta::assert_debug_snapshot!("home_in_colour_120x32", session.frame());
}

#[test]
fn home_in_colour_shows_the_mark_the_actions_and_what_practice_will_send_at_80x24() {
    let session = app_in(TRUECOLOR, 80, 24);

    insta::assert_debug_snapshot!("home_in_colour_80x24", session.frame());
}

#[test]
fn home_without_colour_keeps_the_mark_and_marks_focus_with_a_marker_and_bold_at_120x32() {
    let session = app_in(TRUECOLOR_TURNED_OFF, 120, 32);

    insta::assert_debug_snapshot!("home_without_colour_120x32", session.frame());
}

#[test]
fn home_without_colour_keeps_the_mark_and_marks_focus_with_a_marker_and_bold_at_80x24() {
    let session = app_in(TRUECOLOR_TURNED_OFF, 80, 24);

    insta::assert_debug_snapshot!("home_without_colour_80x24", session.frame());
}

#[test]
fn q_quits_from_home() {
    let mut session = app_in(TRUECOLOR, 80, 24);

    assert_eq!(session.press("q"), Flow::Quit);
}

#[test]
fn ctrl_c_quits_from_home() {
    let mut session = app_in(TRUECOLOR, 80, 24);

    assert_eq!(session.press("<C-c>"), Flow::Quit);
}

#[test]
fn other_keys_keep_the_app_running() {
    let mut session = app_in(TRUECOLOR, 80, 24);

    assert_eq!(session.press("c x <Esc>"), Flow::Continue);
    for event in [
        Event::Resize(80, 24),
        // Only key presses act; a release of `q` is not a second quit.
        Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
    ] {
        assert_eq!(session.handle(&event), Flow::Continue, "after {event:?}");
    }
}

#[test]
fn home_in_24_bit_colour_uses_only_the_dark_brand_tokens() {
    let used = colors_used(&app_in(TRUECOLOR, 120, 32).frame());

    let tokens = dark_token_colors();
    assert!(used.is_subset(&tokens), "{used:?} within {tokens:?}");
}

#[test]
fn home_in_256_colours_uses_only_the_indexed_brand_tokens() {
    let used = colors_used(&app_in(&[("TERM", "xterm-256color")], 120, 32).frame());

    let tokens = ansi256_token_colors();
    assert!(used.is_subset(&tokens), "{used:?} within {tokens:?}");
}

/// The keys from home to each screen and overlay.
const EVERY_VIEW: [&str; 13] = [
    "",
    "j <Enter>",
    "?",
    "j <Enter> ?",
    ":",
    "j <Enter> : j",
    // The picker with an area deselected and a change chosen.
    "<Enter> j <Space> j j j j j <Space>",
    "<Enter> ?",
    "<Enter> :",
    // The viewer on the fifth change: searched, with a match current.
    "<Enter> j j j j j j j j j <Enter> l l / C h a r g e <Enter> n",
    // Its Files pane, and the search box open.
    "<Enter> j j j j j j j j j <Enter> h / x",
    "<Enter> j j j j j j j j j <Enter> ?",
    "<Enter> j j j j j j j j j <Enter> :",
];

#[test]
fn every_screen_and_overlay_in_24_bit_colour_uses_only_the_dark_brand_tokens() {
    let tokens = dark_token_colors();
    for keys in EVERY_VIEW {
        let mut session = app_in(TRUECOLOR, 80, 24);
        session.press(keys);

        let used = colors_used(&session.frame());
        assert!(
            used.is_subset(&tokens),
            "after {keys:?}: {used:?} within {tokens:?}"
        );
    }
}

#[test]
fn every_screen_and_overlay_in_256_colours_uses_only_the_indexed_brand_tokens() {
    let tokens = ansi256_token_colors();
    for keys in EVERY_VIEW {
        let mut session = app_in(&[("TERM", "xterm-256color")], 80, 24);
        session.press(keys);

        let used = colors_used(&session.frame());
        assert!(
            used.is_subset(&tokens),
            "after {keys:?}: {used:?} within {tokens:?}"
        );
    }
}

#[test]
fn settings_in_colour_at_120x32() {
    let mut session = app_in(TRUECOLOR, 120, 32);
    session.press("j <Enter>");

    insta::assert_debug_snapshot!("settings_in_colour_120x32", session.frame());
}

#[test]
fn the_palette_in_colour_at_80x24() {
    let mut session = app_in(TRUECOLOR, 80, 24);
    session.press("j :");

    insta::assert_debug_snapshot!("palette_in_colour_80x24", session.frame());
}

//! App seam: build the app from injected inputs, feed it terminal events and
//! snapshot the frames it draws on ratatui's test backend.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use whetstone::{App, ColorSupport, Flow, Theme};

use crate::colors::{ansi256_token_colors, colors_used, dark_token_colors, environment};

/// A terminal that reports 24-bit colour.
const TRUECOLOR: &[(&str, &str)] = &[("COLORTERM", "truecolor")];
/// The same terminal with colour turned off.
const NO_COLOR: &[(&str, &str)] = &[("COLORTERM", "truecolor"), ("NO_COLOR", "1")];

/// The app as it starts in a terminal whose environment is `variables`.
fn app_in(variables: &[(&str, &str)]) -> App {
    App::new(Theme::new(ColorSupport::detect(&environment(variables))))
}

/// Draws one frame. Its debug form lists every style change next to the
/// text, so snapshots show colour and attributes too.
fn draw(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("create a test terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("draw a frame");
    terminal.backend().buffer().clone()
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

#[test]
fn home_in_colour_shows_the_mark_the_actions_and_what_practice_will_send_at_120x32() {
    let app = app_in(TRUECOLOR);

    insta::assert_debug_snapshot!("home_in_colour_120x32", draw(&app, 120, 32));
}

#[test]
fn home_in_colour_shows_the_mark_the_actions_and_what_practice_will_send_at_80x24() {
    let app = app_in(TRUECOLOR);

    insta::assert_debug_snapshot!("home_in_colour_80x24", draw(&app, 80, 24));
}

#[test]
fn home_without_colour_keeps_the_mark_and_marks_focus_with_a_marker_and_bold_at_120x32() {
    let app = app_in(NO_COLOR);

    insta::assert_debug_snapshot!("home_without_colour_120x32", draw(&app, 120, 32));
}

#[test]
fn home_without_colour_keeps_the_mark_and_marks_focus_with_a_marker_and_bold_at_80x24() {
    let app = app_in(NO_COLOR);

    insta::assert_debug_snapshot!("home_without_colour_80x24", draw(&app, 80, 24));
}

#[test]
fn q_quits_from_home() {
    let mut app = app_in(TRUECOLOR);

    assert_eq!(
        app.handle(&key(KeyCode::Char('q'), KeyModifiers::NONE)),
        Flow::Quit
    );
}

#[test]
fn ctrl_c_quits_from_home() {
    let mut app = app_in(TRUECOLOR);

    assert_eq!(
        app.handle(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Flow::Quit
    );
}

#[test]
fn other_keys_keep_the_app_running() {
    let mut app = app_in(TRUECOLOR);

    for event in [
        key(KeyCode::Char('c'), KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::NONE),
        key(KeyCode::Esc, KeyModifiers::NONE),
        Event::Resize(80, 24),
        // Only key presses act; a release of `q` is not a second quit.
        Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
    ] {
        assert_eq!(app.handle(&event), Flow::Continue, "after {event:?}");
    }
}

#[test]
fn home_in_24_bit_colour_uses_only_the_dark_brand_tokens() {
    let used = colors_used(&draw(&app_in(TRUECOLOR), 120, 32));

    let tokens = dark_token_colors();
    assert!(used.is_subset(&tokens), "{used:?} within {tokens:?}");
}

#[test]
fn home_in_256_colours_uses_only_the_indexed_brand_tokens() {
    let used = colors_used(&draw(&app_in(&[("TERM", "xterm-256color")]), 120, 32));

    let tokens = ansi256_token_colors();
    assert!(used.is_subset(&tokens), "{used:?} within {tokens:?}");
}

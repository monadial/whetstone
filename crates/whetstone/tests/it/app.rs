//! App seam: build the app from injected inputs, feed it terminal events and
//! snapshot the frames it draws on ratatui's test backend.

use std::collections::BTreeSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::Color;
use whetstone::{App, ColorSupport, Environment, Flow, Theme};

/// A terminal that reports 24-bit colour.
const TRUECOLOR: &[(&str, &str)] = &[("COLORTERM", "truecolor")];
/// The same terminal with colour turned off.
const NO_COLOR: &[(&str, &str)] = &[("COLORTERM", "truecolor"), ("NO_COLOR", "1")];

/// The app as it starts in a terminal whose environment is `variables`.
fn app_in(variables: &[(&str, &str)]) -> App {
    let environment: Environment = variables.iter().copied().collect();
    App::new(Theme::new(ColorSupport::detect(&environment)))
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

/// The brand token file, the source of truth for the palette.
const BRAND_TOKENS: &str = include_str!("../../../../assets/brand/brand-tokens.json");

/// The brand token text from the key `start` up to the key `end`.
fn tokens_between(start: &str, end: &str) -> &'static str {
    let (_, after) = BRAND_TOKENS
        .split_once(start)
        .expect("the brand tokens have the start key");
    let (section, _) = after
        .split_once(end)
        .expect("the brand tokens have the end key after it");
    section
}

/// Every colour a frame uses, as foreground or background.
fn colours(frame: &Buffer) -> BTreeSet<String> {
    frame
        .content()
        .iter()
        .flat_map(|cell| [cell.fg, cell.bg])
        .map(|colour| format!("{colour:?}"))
        .collect()
}

#[test]
fn home_in_24_bit_colour_uses_only_the_dark_brand_tokens() {
    let tokens: BTreeSet<String> = tokens_between("\"dark\":", "\"ansi256Dark\"")
        .split('"')
        .filter_map(|text| text.strip_prefix('#'))
        .map(|hex| {
            let [_, red, green, blue] = u32::from_str_radix(hex, 16)
                .expect("a hex colour")
                .to_be_bytes();
            format!("{:?}", Color::Rgb(red, green, blue))
        })
        .collect();

    let used = colours(&draw(&app_in(TRUECOLOR), 120, 32));

    assert!(used.is_subset(&tokens), "{used:?} within {tokens:?}");
}

#[test]
fn home_in_256_colours_uses_only_the_indexed_brand_tokens() {
    let tokens: BTreeSet<String> = tokens_between("\"ansi256Dark\":", "\"ansi16\"")
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|number| number.parse().ok())
        .map(|index| format!("{:?}", Color::Indexed(index)))
        .collect();

    let used = colours(&draw(&app_in(&[("TERM", "xterm-256color")]), 120, 32));

    assert!(used.is_subset(&tokens), "{used:?} within {tokens:?}");
}
